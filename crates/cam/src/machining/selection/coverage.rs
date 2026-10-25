//! Exact containment of the saved piecewise-linear finished section in cylindrical stock.
//! This is independent of cutting conditions, toolpaths and the residual-stock sampling grid.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortageKind {
    OuterDiameter,
    InnerDiameter,
    Front,
    Back,
}

/// Missing material in one axial interval. Radial bounds are radii, not X diameters.
/// Both boundaries vary linearly from z0 to z1; a triangular region can have zero width at one end.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockShortage {
    pub kind: ShortageKind,
    pub z0_mm: f64,
    pub z1_mm: f64,
    pub inner0_mm: f64,
    pub inner1_mm: f64,
    pub outer0_mm: f64,
    pub outer1_mm: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StockCoverage {
    pub shortages: Vec<StockShortage>,
    /// Exact revolution volume of the reported polygonal regions, before any machining.
    pub missing_volume_mm3: f64,
    /// False for legacy OD-only intent: the original bore is retained, not assumed to be solid.
    pub inner_boundary_known: bool,
    /// False when no OD profile is available; no full-section containment claim can be made.
    pub outer_boundary_known: bool,
}

fn validate_profile(points: &[ProfilePoint]) -> Result<()> {
    if points.len() > 256 {
        return invalid("材料検証の輪郭点数が上限を超えました");
    }
    for p in points {
        bounded("profile Z", p.z_mm, -1e6, 1e6)?;
        bounded("profile diameter", p.diameter_mm, 0., 1e6)?;
    }
    if points.windows(2).any(|p| matches!(p, [a, b] if b.z_mm > a.z_mm + EPS)) {
        return invalid("材料検証の輪郭は前側から奥側の順に設定してください");
    }
    Ok(())
}

/// Evaluate from the segment containing the interval midpoint, so a shoulder's other side
/// cannot leak into the one-sided endpoint value used by the exact volume integral.
fn interval_radii(points: &[ProfilePoint], z0: f64, z1: f64) -> Option<[f64; 2]> {
    let mid = (z0 + z1) * 0.5;
    points.windows(2).find_map(|p| {
        let [a, b] = p else { return None };
        if a.z_mm - b.z_mm <= EPS || mid > a.z_mm || mid < b.z_mm {
            return None;
        }
        let radius = |z: f64| (a.diameter_mm + (b.diameter_mm - a.diameter_mm) * (a.z_mm - z) / (a.z_mm - b.z_mm)) * 0.5;
        Some([radius(z0), radius(z1)])
    })
}

fn add_crossings(zs: &mut Vec<f64>, points: &[ProfilePoint], diameters: [f64; 2]) {
    zs.extend(points.iter().map(|p| p.z_mm));
    for p in points.windows(2) {
        let [a, b] = p else { continue };
        for diameter in diameters {
            if (b.diameter_mm - a.diameter_mm).abs() > EPS {
                let t = (diameter - a.diameter_mm) / (b.diameter_mm - a.diameter_mm);
                if t > 0. && t < 1. {
                    zs.push(a.z_mm + t * (b.z_mm - a.z_mm));
                }
            }
        }
    }
}

fn add_region(out: &mut StockCoverage, kind: ShortageKind, z: [f64; 2], inner: [f64; 2], outer: [f64; 2]) {
    if (outer[0] - inner[0]).max(outer[1] - inner[1]) <= EPS {
        return;
    }
    let square_integral = |r: [f64; 2]| r[0] * r[0] + r[0] * r[1] + r[1] * r[1];
    out.missing_volume_mm3 += std::f64::consts::PI * (z[1] - z[0]) / 3. * (square_integral(outer) - square_integral(inner)).max(0.);
    out.shortages.push(StockShortage {
        kind,
        z0_mm: z[0],
        z1_mm: z[1],
        inner0_mm: inner[0],
        inner1_mm: inner[1],
        outer0_mm: outer[0],
        outer1_mm: outer[1],
    });
}

/// Inspect the whole finished section before machining. Stock crossings and axial planes
/// split every interval analytically, including shortages narrower than the simulation grid.
/// Legacy OD-only setups preserve their unspecified original bore and report that limitation.
pub fn stock_coverage(setup: &Setup) -> Result<StockCoverage> {
    let stock = setup.stock.as_ref().ok_or_else(|| PlanError("材料を入力してください".into()))?;
    bounded("stock diameter", stock.outer_diameter_mm, 0.001, 1e6)?;
    bounded("stock bore", stock.inner_diameter_mm, 0., stock.outer_diameter_mm - 0.001)?;
    bounded("stock length", stock.length_mm, 0.001, 1e6)?;
    for job in setup.od_job.iter().chain(&setup.id_job) {
        job.validate_draft()?;
        validate_profile(&job.profile)?;
    }
    if let Some(drawing) = &setup.drawing {
        drawing.validate()?;
        validate_profile(&drawing.inner)?;
    }
    let outer = setup.od_job.as_ref().map(|j| j.profile.as_slice()).unwrap_or_default();
    let inner = setup
        .drawing
        .as_ref()
        .filter(|d| d.inner.len() >= 2)
        .map(|d| d.inner.as_slice())
        .or_else(|| setup.id_job.as_ref().map(|j| j.profile.as_slice()))
        .unwrap_or_default();
    let mut out = StockCoverage { inner_boundary_known: inner.len() >= 2, ..StockCoverage::default() };
    if outer.len() < 2 {
        return Ok(out);
    }
    // An explicitly saved through-bore or closed section must cover the whole OD.
    // Treat only an absent boundary as legacy unknown intent; truncated geometry must
    // not silently use the stock bore or hide material shortages beyond the OD range.
    for known_inner in
        setup.drawing.iter().map(|d| d.inner.as_slice()).chain(setup.id_job.iter().map(|j| j.profile.as_slice())).filter(|p| p.len() >= 2)
    {
        let endpoints = outer.first().zip(outer.last()).zip(known_inner.first().zip(known_inner.last()));
        if endpoints.is_none_or(|((a, b), (c, d))| (a.z_mm - c.z_mm).abs() > EPS || (b.z_mm - d.z_mm).abs() > EPS) {
            return invalid("完成輪郭の外径と内径の前端・後端が一致しません。閉じた断面を再取得してください");
        }
    }
    let front = setup.od_job.as_ref().map_or(0., |j| j.stock_front_z_mm);
    let back = front - stock.length_mm;
    let mut zs = vec![front, back];
    let diameters = [stock.inner_diameter_mm, stock.outer_diameter_mm];
    add_crossings(&mut zs, outer, diameters);
    add_crossings(&mut zs, inner, diameters);
    zs.sort_by(f64::total_cmp);
    zs.dedup_by(|a, b| (*a - *b).abs() <= EPS);
    let stock_inner = stock.inner_diameter_mm * 0.5;
    let stock_outer = stock.outer_diameter_mm * 0.5;
    for pair in zs.windows(2) {
        let [z0, z1] = pair else { continue };
        if z1 - z0 <= EPS {
            continue;
        }
        let Some(outer) = interval_radii(outer, *z0, *z1) else { continue };
        out.outer_boundary_known = true;
        let inner = match interval_radii(inner, *z0, *z1) {
            Some(inner) => inner,
            None => {
                out.inner_boundary_known = false;
                // No saved inner section means the original bore is unspecified by the
                // finished geometry. Do not invent a solid part or change legacy OD intent.
                [stock_inner.min(outer[0]), stock_inner.min(outer[1])]
            }
        };
        if inner.iter().zip(outer).any(|(a, b)| *a > b + EPS) {
            return invalid("完成輪郭の内径が外径より大きいため材料を検証できません");
        }
        let z = [*z0, *z1];
        let mid = (z0 + z1) * 0.5;
        if mid > front {
            add_region(&mut out, ShortageKind::Front, z, inner, outer);
        } else if mid < back {
            add_region(&mut out, ShortageKind::Back, z, inner, outer);
        } else {
            add_region(&mut out, ShortageKind::OuterDiameter, z, inner.map(|r| r.max(stock_outer)), outer.map(|r| r.max(stock_outer)));
            add_region(&mut out, ShortageKind::InnerDiameter, z, inner.map(|r| r.min(stock_inner)), outer.map(|r| r.min(stock_inner)));
        }
    }
    Ok(out)
}

pub(in crate::machining) fn require_covered(setup: &Setup) -> Result<()> {
    let coverage = stock_coverage(setup)?;
    if coverage.shortages.is_empty() {
        return Ok(());
    }
    let first = coverage.shortages.first().ok_or_else(|| PlanError("材料不足を確認してください".into()))?;
    let cause = match first.kind {
        ShortageKind::OuterDiameter => "材料外径が完成外径より小さい",
        ShortageKind::InnerDiameter => "材料の下穴径が完成内径より大きい",
        ShortageKind::Front => "材料前端より前に完成形状がある",
        ShortageKind::Back => "材料の長さが完成形状に足りない",
    };
    invalid(&format!("材料不足: {cause}（Z {:.3} ～ {:.3}）。②材料で寸法と配置を修正してください", first.z0_mm, first.z1_mm))
}
