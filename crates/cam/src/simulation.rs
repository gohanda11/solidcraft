//! Conservative holder-envelope checks against a replayed axisymmetric stock.
//! Optional rounded-triangle section sweeps; no automatic tool nose compensation or machine digital twin.
use crate::machining::{Motion, Phase, ProfilePoint, Toolpath};
use crate::{OperationKind, Result, Setup, invalid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolderKind {
    Round,
    Rectangular,
}

/// Coordinates relative to the programmed nominal tip: Z axial, R radial, T tangential.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Holder {
    pub kind: HolderKind,
    pub z_start_mm: f64,
    pub length_mm: f64,
    pub r_center_mm: f64,
    pub t_center_mm: f64,
    pub width_mm: f64,
    pub height_mm: f64,
}
impl Default for Holder {
    fn default() -> Self {
        Self { kind: HolderKind::Rectangular, z_start_mm: 0., length_mm: 0., r_center_mm: 0., t_center_mm: 0., width_mm: 0., height_mm: 0. }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ToolAssembly {
    pub triangular_insert: bool,
    pub nose_radius_mm: Option<f64>,
    pub holder: Option<Holder>,
    /// The owner confirmed dimensions, tip-relative placement and coverage of the holder/head.
    pub holder_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert: Option<crate::insert::Insert>,
}
impl ToolAssembly {
    pub fn validate(&self) -> Result<()> {
        if let Some(insert) = &self.insert {
            if !self.triangular_insert {
                return invalid("insert section requires a triangular insert");
            }
            insert.validate(self.nose_radius_mm.ok_or_else(|| crate::PlanError("insert nose radius is required".into()))?)?;
        }
        if self.nose_radius_mm.is_some_and(|r| !r.is_finite() || !(0.0..=10.).contains(&r)) {
            return invalid("invalid insert nose radius");
        }
        if let Some(h) = &self.holder {
            for v in [h.z_start_mm, h.r_center_mm, h.t_center_mm] {
                if !v.is_finite() || v.abs() > 1000. {
                    return invalid("holder coordinates must be finite and within 1000 mm");
                }
            }
            for v in [h.length_mm, h.width_mm, h.height_mm] {
                if !v.is_finite() || !(0.0..=1000.).contains(&v) {
                    return invalid("holder dimensions must be 0..1000 mm");
                }
            }
        }
        if self.holder_verified && self.holder.as_ref().is_none_or(|h| !h.ready()) {
            return invalid("holder dimensions must be entered before confirming them");
        }
        Ok(())
    }
}
impl Holder {
    pub fn ready(&self) -> bool {
        self.length_mm > 0. && self.width_mm > 0. && (self.kind == HolderKind::Round || self.height_mm > 0.)
    }
    /// Encloses the entire continuous translational sweep, including between sample endpoints.
    pub fn swept_bounds(&self, a: ProfilePoint, b: ProfilePoint) -> (f64, f64, f64, f64) {
        let z0 = a.z_mm.min(b.z_mm) + self.z_start_mm;
        let z1 = a.z_mm.max(b.z_mm) + self.z_start_mm + self.length_mm;
        let r0 = a.diameter_mm.min(b.diameter_mm) * 0.5 + self.r_center_mm;
        let r1 = a.diameter_mm.max(b.diameter_mm) * 0.5 + self.r_center_mm;
        let near = |lo: f64, hi: f64| if lo <= 0. && hi >= 0. { 0. } else { lo.abs().min(hi.abs()) };
        let (low, high) = match self.kind {
            HolderKind::Round => (
                (near(r0, r1).hypot(self.t_center_mm) - self.width_mm * 0.5).max(0.),
                r0.abs().max(r1.abs()).hypot(self.t_center_mm) + self.width_mm * 0.5,
            ),
            HolderKind::Rectangular => {
                let (lo, hi) = (r0 - self.width_mm * 0.5, r1 + self.width_mm * 0.5);
                let (t0, t1) = (self.t_center_mm - self.height_mm * 0.5, self.t_center_mm + self.height_mm * 0.5);
                (near(lo, hi).hypot(near(t0, t1)), lo.abs().max(hi.abs()).hypot(t0.abs().max(t1.abs())))
            }
        };
        (z0, z1, low, high)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Slab {
    pub z0: f64,
    pub z1: f64,
    pub inner: f64,
    pub outer: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StockEdit {
    pub after_move: usize,
    pub slab: usize,
    pub inner: f64,
    pub outer: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BandEdit {
    pub after_move: usize,
    pub slab: usize,
    pub bands: Vec<[f64; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub move_index: usize,
    pub station: u8,
    pub target: String,
    pub tip: ProfilePoint,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Simulation {
    pub resolution_mm: f64,
    pub initial: Vec<Slab>,
    pub edits: Vec<StockEdit>,
    pub contacts: Vec<Contact>,
    pub missing_stations: Vec<u8>,
    pub checked_stations: Vec<u8>,
    pub unchecked_moves: usize,
    pub contact_list_truncated: bool,
    pub machine_ready: bool,
    pub scope: String,
    pub band_edits: Vec<BandEdit>,
    pub insert_stations: Vec<u8>,
    pub nominal_stations: Vec<u8>,
}
pub fn operation(phase: Phase) -> Option<OperationKind> {
    Some(match phase {
        Phase::CenterDrill => OperationKind::CenterDrill,
        Phase::Drill => OperationKind::Drill,
        Phase::OdGrooveRough => OperationKind::OdGrooveRough,
        Phase::OdGrooveFinish => OperationKind::OdGrooveFinish,
        Phase::FaceRough => OperationKind::FaceRough,
        Phase::FaceFinish => OperationKind::FaceFinish,
        Phase::OdRough | Phase::RoughContour => OperationKind::OdRough,
        Phase::OdFinish => OperationKind::OdFinish,
        Phase::IdRough | Phase::IdRoughContour => OperationKind::IdRough,
        Phase::IdFinish => OperationKind::IdFinish,
        Phase::Transfer => return None,
    })
}
pub fn station(setup: &Setup, phase: Phase) -> Option<u8> {
    let kind = operation(phase)?;
    setup.operations.iter().find(|o| o.kind == kind)?.station
}

/// Grid cells are removed only when the whole axial slab has been cut. Partial cells remain,
/// so the grid overestimates the nominal-tip stock. Actual insert cutting is outside this model.
pub(crate) fn remove(stock: &mut [Slab], a: ProfilePoint, b: ProfilePoint, phase: Phase) {
    let za = a.z_mm.min(b.z_mm);
    let zb = a.z_mm.max(b.z_mm);
    let ra = a.diameter_mm * 0.5;
    let rb = b.diameter_mm * 0.5;
    for s in stock {
        if s.outer <= s.inner {
            continue;
        }
        if matches!(phase, Phase::FaceRough | Phase::FaceFinish) {
            if (a.z_mm - b.z_mm).abs() < 1e-9 && s.z0 >= za - 1e-9 && ra.max(rb) >= s.outer - 1e-9 {
                s.outer = s.outer.min(ra.min(rb).max(s.inner));
            }
        } else if zb - za > 1e-9 && s.z0 >= za - 1e-9 && s.z1 <= zb + 1e-9 {
            let r = |z: f64| ra + (rb - ra) * (z - a.z_mm) / (b.z_mm - a.z_mm);
            let (r0, r1) = (r(s.z0), r(s.z1));
            match phase {
                Phase::OdRough | Phase::RoughContour | Phase::OdFinish => s.outer = s.outer.min(r0.max(r1).max(s.inner)),
                Phase::IdRough | Phase::IdRoughContour | Phase::IdFinish => s.inner = s.inner.max(r0.min(r1).min(s.outer)),
                _ => {}
            }
        }
    }
}

fn profile_radius(points: &[ProfilePoint], z: f64) -> Option<f64> {
    points.windows(2).find_map(|w| {
        let [a, b] = w else { return None };
        if (b.z_mm - a.z_mm).abs() < 1e-10 || z < a.z_mm.min(b.z_mm) || z > a.z_mm.max(b.z_mm) {
            return None;
        }
        Some((a.diameter_mm + (b.diameter_mm - a.diameter_mm) * (z - a.z_mm) / (b.z_mm - a.z_mm)) * 0.5)
    })
}
fn finished_band(setup: &Setup, path: &Toolpath, z: f64) -> Option<[f64; 2]> {
    if z > 0. {
        return None;
    }
    let model = crate::machining::sequence::model_linked(setup);
    // Whole-profile planning clips its job at the requested cutting end. The cutter
    // can extend beyond that end, so collision targets must use the complete saved
    // section; only the nominal path, not the finished part, has been shortened.
    let (mut od, mut id) = if path.inner {
        (path.stock.outer_diameter_mm * 0.5, profile_radius(setup.id_job.as_ref().map_or(path.job.profile.as_slice(), |j| j.profile.as_slice()), z)?)
    } else {
        (
            profile_radius(setup.od_job.as_ref().map_or(path.job.profile.as_slice(), |j| j.profile.as_slice()), z)?,
            setup
                .id_job
                .as_ref()
                .map(|j| j.profile.as_slice())
                .or_else(|| path.id_path.as_ref().map(|p| p.job.profile.as_slice()))
                .and_then(|points| profile_radius(points, z))
                .unwrap_or_else(|| {
                    if model {
                        setup.drawing.as_ref().and_then(|d| profile_radius(&d.inner, z)).unwrap_or(0.)
                    } else {
                        path.stock.inner_diameter_mm * 0.5
                    }
                }),
        )
    };
    if !model {
        // Legacy manual local dimensions are an explicit target amendment. Model-linked
        // targets are never changed by an operation that would cut beyond the model.
        for c in setup.operations.iter().filter_map(|o| o.cutting.as_ref()) {
            match c {
                crate::machining::sequence::Cutting::Drill(d) => id = id.max(d.diameter_at(z) * 0.5),
                crate::machining::sequence::Cutting::OdGroove(g) if z <= g.front_z_mm && z >= g.front_z_mm - g.width_mm => {
                    od = od.min(g.bottom_diameter_mm * 0.5);
                }
                _ => {}
            }
        }
    }
    Some([id, od])
}

/// Continuous nominal cutter section swept by a local axis-aligned motion.
/// Groove height is unknown: only the material-facing radial cutting half-line is tested.
fn local_section(cutting: &crate::machining::sequence::Cutting, a: ProfilePoint, b: ProfilePoint, z: f64) -> Option<[f64; 2]> {
    use crate::machining::sequence::Cutting;
    match cutting {
        Cutting::Drill(d) if z >= a.z_mm.min(b.z_mm) => {
            Some([0., ((z - a.z_mm.min(b.z_mm)) * (d.point_angle_rad * 0.5).tan()).min(d.diameter_mm * 0.5)])
        }
        Cutting::OdGroove(g) if z >= a.z_mm.min(b.z_mm) - g.tool_width_mm && z <= a.z_mm.max(b.z_mm) => {
            Some([a.diameter_mm.min(b.diameter_mm) * 0.5, f64::INFINITY])
        }
        _ => None,
    }
}

fn local_contact_pose(cutting: &crate::machining::sequence::Cutting, a: ProfilePoint, b: ProfilePoint, z: f64, band: [f64; 2]) -> ProfilePoint {
    let at = |t: f64| ProfilePoint { z_mm: a.z_mm + (b.z_mm - a.z_mm) * t, diameter_mm: a.diameter_mm + (b.diameter_mm - a.diameter_mm) * t };
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..14 {
        let mid = (lo + hi) * 0.5;
        if local_section(cutting, a, at(mid), z).is_some_and(|cut| band[1].min(cut[1]) - band[0].max(cut[0]) > 1e-8) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    at(hi)
}

/// Locate a candidate within this move at one sampled axial section. The returned pose is
/// a contact in the approximated model, not necessarily the first contact over every slab.
fn insert_contact_pose(outline: &[crate::insert::Point], a: ProfilePoint, b: ProfilePoint, z: f64, band: [f64; 2]) -> ProfilePoint {
    let at = |t: f64| ProfilePoint { z_mm: a.z_mm + (b.z_mm - a.z_mm) * t, diameter_mm: a.diameter_mm + (b.diameter_mm - a.diameter_mm) * t };
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..14 {
        let mid = (lo + hi) * 0.5;
        let poly = crate::insert::sweep(outline, a, at(mid));
        let hit = crate::insert::section(&poly, z).is_some_and(|[r0, r1]| {
            let min = if r0 <= 0. && r1 >= 0. { 0. } else { r0.abs().min(r1.abs()) };
            band[1].min(r0.abs().max(r1.abs())) - band[0].max(min) > 1e-8
        });
        if hit {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    at(hi)
}

pub fn simulate(setup: &Setup, path: &Toolpath, resolution: f64) -> Result<Simulation> {
    if path.moves.len() > 20_000 || setup.turret.len() > 12 || setup.operations.len() > 256 {
        return invalid("simulation input size limit");
    }
    for operation in &setup.operations {
        if let Some(cutting) = &operation.cutting {
            cutting.validate_draft(operation.kind)?;
        }
    }
    path.job.validate_draft()?;
    if let Some(id) = &path.id_path {
        id.job.validate_draft()?;
    }
    for job in setup.od_job.iter().chain(&setup.id_job) {
        job.validate_draft()?;
    }
    if !path.stock.length_mm.is_finite()
        || path.stock.length_mm <= 0.
        || !path.stock.outer_diameter_mm.is_finite()
        || path.stock.outer_diameter_mm <= 0.
        || !path.stock.inner_diameter_mm.is_finite()
        || path.stock.inner_diameter_mm < 0.
        || path.stock.inner_diameter_mm >= path.stock.outer_diameter_mm
    {
        return invalid("invalid simulation stock");
    }
    if path.moves.iter().any(|m| {
        [m.from, m.to]
            .iter()
            .any(|p| !p.z_mm.is_finite() || p.z_mm.abs() > 1e6 || !p.diameter_mm.is_finite() || !(0.0..=1e6).contains(&p.diameter_mm))
    }) {
        return invalid("invalid simulation move");
    }
    if !resolution.is_finite() || !(0.1..=2.).contains(&resolution) {
        return invalid("simulation resolution must be 0.1..2 mm");
    }
    let count = (path.stock.length_mm / resolution).ceil() as usize;
    if count == 0 || count > 2048 {
        return invalid("stock too long for simulation resolution (limit 2048 slabs)");
    }
    let front = path.job.stock_front_z_mm;
    let back = front - path.stock.length_mm;
    let mut zs: Vec<_> = (0..=count).map(|i| (back + i as f64 * resolution).min(front)).collect();
    zs.push(0.);
    zs.extend(path.job.profile.iter().map(|p| p.z_mm));
    if let Some(id) = &path.id_path {
        zs.extend(id.job.profile.iter().map(|p| p.z_mm));
    }
    for job in setup.od_job.iter().chain(&setup.id_job) {
        zs.extend(job.profile.iter().map(|p| p.z_mm));
    }
    if let Some(drawing) = &setup.drawing {
        drawing.validate()?;
        zs.extend(drawing.inner.iter().map(|p| p.z_mm));
    }
    for c in setup.operations.iter().filter_map(|o| o.cutting.as_ref()) {
        match c {
            crate::machining::sequence::Cutting::OdGroove(g) => {
                zs.extend([g.front_z_mm, g.front_z_mm - g.width_mm, g.front_z_mm - g.axial_leave_mm, g.front_z_mm - g.width_mm + g.axial_leave_mm])
            }
            crate::machining::sequence::Cutting::Drill(d) => zs.extend([d.tip_end_z_mm, d.full_diameter_z()]),
            _ => {}
        }
    }
    zs.retain(|z| z.is_finite() && *z >= back && *z <= front);
    zs.sort_by(f64::total_cmp);
    zs.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
    if zs.len() > 4097 {
        return invalid("simulation section boundary limit");
    }
    let mut stock: Vec<_> = zs
        .windows(2)
        .filter_map(|w| {
            let [a, b] = w else { return None };
            Some(Slab { z0: *a, z1: *b, inner: path.stock.inner_diameter_mm * 0.5, outer: path.stock.outer_diameter_mm * 0.5 })
        })
        .collect();
    let count = stock.len();
    let mut bands: Vec<Vec<[f64; 2]>> = stock.iter().map(|s| vec![[s.inner, s.outer]]).collect();
    let mut report=Simulation{band_edits:vec![],insert_stations:vec![],nominal_stations:vec![],resolution_mm:resolution,initial:stock.clone(),edits:vec![],contacts:vec![],missing_stations:vec![],checked_stations:vec![],unchecked_moves:0,contact_list_truncated:false,machine_ready:false,scope:"axial midpoint slices; nominal drill cone/cylinder and rectangular groove cutting width (not verified physical cutter geometry); optional rounded-triangle swept section without nose compensation; holder checks use pre-move stock envelopes; assumed insert dimensions, flank clearance, turret, jaws, tool changes and machine axes are not verified".into()};
    let mut work = 0usize;
    let mut section_work = 0usize;
    for (index, m) in path.moves.iter().enumerate() {
        let before = stock.clone();
        let local_op = crate::machining::sequence::operation_at(path, setup, index);
        let cutting = local_op.and_then(|o| o.cutting.as_ref()).filter(|c| !matches!(c, crate::machining::sequence::Cutting::Contour(_)));
        let station = local_op.map(|o| o.station).unwrap_or_else(|| station(setup, m.phase));
        let tool = station.and_then(|n| setup.turret.iter().find(|t| t.station == n)).and_then(|t| t.simulation.as_ref());
        if let Some(t) = tool {
            t.validate()?;
        }
        let outline =
            tool.filter(|_| cutting.is_none()).and_then(|t| t.insert.as_ref().zip(t.nose_radius_mm)).map(|(i, r)| i.outline(r)).transpose()?;
        if let Some(n) = station {
            let stations = if outline.is_some() { &mut report.insert_stations } else { &mut report.nominal_stations };
            if !stations.contains(&n) {
                stations.push(n);
            }
        }
        let holder = tool.filter(|t| t.holder_verified).and_then(|t| t.holder.as_ref());
        if holder.is_none() {
            report.unchecked_moves += 1;
            if let Some(n) = station
                && !report.missing_stations.contains(&n)
            {
                report.missing_stations.push(n);
            }
        } else if let Some(n) = station
            && !report.checked_stations.contains(&n)
        {
            report.checked_stations.push(n);
        }
        let length = (m.to.z_mm - m.from.z_mm).abs().max((m.to.diameter_mm - m.from.diameter_mm).abs() * 0.5);
        // Insert cutting uses one continuous swept polygon. With no checked holder there
        // is no sampled work here; smaller cutting depths must not waste the work budget.
        let steps = if (outline.is_some() || cutting.is_some()) && holder.is_none() { 1 } else { (length / resolution).ceil().max(1.) as usize };
        work = work.saturating_add(steps.saturating_mul(count));
        if steps > 100_000 || work > 20_000_000 {
            return invalid("simulation work limit; use a coarser resolution or a shorter range");
        }
        let mut previous = m.from;
        let mut hit_stock = false;
        let mut hit_chuck = false;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let current = ProfilePoint {
                z_mm: m.from.z_mm + t * (m.to.z_mm - m.from.z_mm),
                diameter_mm: m.from.diameter_mm + t * (m.to.diameter_mm - m.from.diameter_mm),
            };
            if let (Some(h), Some(n)) = (holder, station) {
                let (z0, z1, r0, r1) = h.swept_bounds(previous, current);
                let candidates = [
                    (
                        !hit_stock && stock.iter().any(|s| s.outer > s.inner && z1 >= s.z0 && z0 <= s.z1 && r1 >= s.inner && r0 <= s.outer),
                        "remaining_stock",
                    ),
                    (!hit_chuck && z0 <= path.job.chuck_front_z_mm, "chuck_front_zone"),
                ];
                for (hit, target) in candidates {
                    if hit {
                        if target == "remaining_stock" {
                            hit_stock = true;
                        } else {
                            hit_chuck = true;
                        }
                        if report.contacts.len() < 256 {
                            report.contacts.push(Contact { move_index: index, station: n, target: target.into(), tip: current });
                        } else {
                            report.contact_list_truncated = true;
                        }
                    }
                }
            }
            if m.motion == Motion::Feed && outline.is_none() {
                if let Some(cutting) = cutting {
                    crate::machining::sequence::remove_local(&mut stock, m.from, current, cutting);
                } else {
                    remove(&mut stock, m.from, current, m.phase);
                }
            }
            previous = current;
        }
        let polygon = outline.as_ref().map(|p| crate::insert::sweep(p, m.from, m.to));
        section_work = section_work.saturating_add(polygon.as_ref().map_or(0, |p| p.len().saturating_mul(count)));
        if section_work > 100_000_000 {
            return invalid("insert sweep work limit; shorten the machining range");
        }
        let mut insert_rapid = None;
        let mut gouge = None;
        for (slab, (s, remaining)) in stock.iter_mut().zip(&mut bands).enumerate() {
            let old = remaining.clone();
            if let Some(poly) = &polygon {
                let z = (s.z0 + s.z1) * 0.5;
                if let Some([lo, hi]) = crate::insert::section(poly, z) {
                    // The material rotates: a section crossing the axis sweeps both radial sides.
                    let cut = [if lo <= 0. && hi >= 0. { 0. } else { lo.abs().min(hi.abs()) }, lo.abs().max(hi.abs())];
                    let intersects = remaining.iter().any(|b| b[1].min(cut[1]) - b[0].max(cut[0]) > 1e-8);
                    if intersects
                        && m.motion == Motion::Rapid
                        && insert_rapid.is_none()
                        && let (Some(outline), Some(band)) = (outline.as_ref(), remaining.iter().find(|b| b[1].min(cut[1]) - b[0].max(cut[0]) > 1e-8))
                    {
                        insert_rapid = Some(insert_contact_pose(outline, m.from, m.to, z, *band));
                    }
                    if m.motion == Motion::Feed {
                        if gouge.is_none()
                            && let Some(target) = finished_band(setup, path, z)
                            && let (Some(outline), Some(band)) = (
                                outline.as_ref(),
                                remaining.iter().find(|b| b[1].min(cut[1]).min(target[1]) - b[0].max(cut[0]).max(target[0]) > 0.002),
                            )
                        {
                            gouge =
                                Some(insert_contact_pose(outline, m.from, m.to, z, [band[0].max(target[0]) + 0.001, band[1].min(target[1]) - 0.001]));
                        }
                        crate::insert::subtract(remaining, cut);
                    }
                }
            } else if let Some(cutting) = cutting {
                let z = (s.z0 + s.z1) * 0.5;
                if let Some(cut) = local_section(cutting, m.from, m.to, z) {
                    if m.motion == Motion::Rapid {
                        if insert_rapid.is_none()
                            && let Some(band) = remaining.iter().find(|b| b[1].min(cut[1]) - b[0].max(cut[0]) > 1e-8)
                        {
                            insert_rapid = Some(local_contact_pose(cutting, m.from, m.to, z, *band));
                        }
                    } else {
                        if gouge.is_none()
                            && let Some(target) = finished_band(setup, path, z)
                            && let Some(band) = remaining.iter().find(|b| b[1].min(cut[1]).min(target[1]) - b[0].max(cut[0]).max(target[0]) > 0.002)
                        {
                            gouge =
                                Some(local_contact_pose(cutting, m.from, m.to, z, [band[0].max(target[0]) + 0.001, band[1].min(target[1]) - 0.001]));
                        }
                        crate::insert::subtract(remaining, cut);
                    }
                }
            } else {
                remaining.retain_mut(|b| {
                    b[0] = b[0].max(s.inner);
                    b[1] = b[1].min(s.outer);
                    b[1] > b[0]
                });
            }
            if remaining.len() > 16 {
                return invalid("insert section radial band limit");
            }
            s.inner = remaining.first().map_or(0., |b| b[0]);
            s.outer = remaining.last().map_or(0., |b| b[1]);
            if *remaining != old {
                if report.band_edits.len() >= 250_000 {
                    return invalid("insert replay history limit");
                }
                report.band_edits.push(BandEdit { after_move: index + 1, slab, bands: remaining.clone() });
            }
        }
        if bands.iter().map(Vec::len).sum::<usize>() > 4096 {
            return invalid("insert replay display size limit");
        }
        if let Some(n) = station {
            let (rapid_target, gouge_target) =
                if cutting.is_some() { ("cutter_rapid_stock", "cutter_gouge") } else { ("insert_rapid_stock", "insert_gouge") };
            for (hit, target) in [(insert_rapid, rapid_target), (gouge, gouge_target)] {
                if let Some(tip) = hit {
                    if report.contacts.len() < 256 {
                        report.contacts.push(Contact { move_index: index, station: n, target: target.into(), tip });
                    } else {
                        report.contact_list_truncated = true;
                    }
                }
            }
        }
        for (slab, (a, b)) in before.iter().zip(&stock).enumerate() {
            if a.inner != b.inner || a.outer != b.outer {
                if report.edits.len() >= 250_000 {
                    return invalid("simulation history limit; shorten the machining range");
                }
                report.edits.push(StockEdit { after_move: index + 1, slab, inner: b.inner, outer: b.outer });
            }
        }
    }
    report.insert_stations.sort_unstable();
    report.nominal_stations.sort_unstable();
    report.checked_stations.sort_unstable();
    report.missing_stations.sort_unstable();
    Ok(report)
}

impl Simulation {
    pub fn stock_at(&self, after_move: usize) -> Vec<Slab> {
        let mut bands: Vec<Vec<[f64; 2]>> = self.initial.iter().map(|s| vec![[s.inner, s.outer]]).collect();
        for e in self.band_edits.iter().take_while(|e| e.after_move <= after_move) {
            if let Some(b) = bands.get_mut(e.slab) {
                *b = e.bands.clone();
            }
        }
        self.initial
            .iter()
            .zip(bands)
            .flat_map(|(s, bands)| bands.into_iter().map(|b| Slab { z0: s.z0, z1: s.z1, inner: b[0], outer: b[1] }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Operation, Stock, TurningPlan,
        machining::{Move, OdJob},
    };

    #[test]
    fn nominal_drill_and_groove_detect_finished_gouge_and_rapid_stock_contact() {
        use crate::machining::sequence::{Cutting, Drill, Groove, OperationSpan};
        for drill in [false, true] {
            let (mut setup, mut path) = fixture();
            let source = crate::machining::ModelSource {
                body: "analytic solid target".into(),
                reverse: false,
                origin: [0.; 3],
                axis: [0., 0., 1.],
                approximation_mm: 0.,
            };
            setup.od_job.as_mut().unwrap().model_source = Some(source.clone());
            path.job.model_source = Some(source);
            let (kind, phase, cutting, from, to) = if drill {
                (
                    OperationKind::Drill,
                    Phase::Drill,
                    Cutting::Drill(Drill { diameter_mm: 8., tip_end_z_mm: -5., feed_mm_rev: 0.1, ..Default::default() }),
                    ProfilePoint { z_mm: 2., diameter_mm: 0. },
                    ProfilePoint { z_mm: -5., diameter_mm: 0. },
                )
            } else {
                (
                    OperationKind::OdGrooveRough,
                    Phase::OdGrooveRough,
                    Cutting::OdGroove(Groove {
                        front_z_mm: -2.,
                        width_mm: 2.,
                        bottom_diameter_mm: 10.,
                        tool_width_mm: 2.,
                        radial_depth_mm: 1.,
                        feed_mm_rev: 0.1,
                        ..Default::default()
                    }),
                    ProfilePoint { z_mm: -2., diameter_mm: 22. },
                    ProfilePoint { z_mm: -2., diameter_mm: 10. },
                )
            };
            setup.operations = vec![Operation {
                id: "local".into(),
                name: "local".into(),
                kind,
                station: kind.suggested_station(),
                cutting: Some(cutting),
                spindle: None,
                manual_separation: None,
            }];
            path.operation_spans = vec![OperationSpan { operation_id: "local".into(), start: 0, end: 1 }];
            path.moves = vec![Move { from, to, phase, motion: Motion::Feed, feed_mm_rev: Some(0.1) }];
            let report = simulate(&setup, &path, 0.5).unwrap();
            let contact = report.contacts.iter().find(|c| c.target == "cutter_gouge").unwrap();
            assert_eq!(contact.move_index, 0);
            assert!(contact.tip.z_mm >= to.z_mm && contact.tip.z_mm <= from.z_mm);
            assert!(contact.tip.diameter_mm >= to.diameter_mm && contact.tip.diameter_mm <= from.diameter_mm);
            assert!(report.scope.contains("nominal drill"));
            assert!(report.insert_stations.is_empty());
            path.moves[0].motion = Motion::Rapid;
            let rapid = simulate(&setup, &path, 0.5).unwrap();
            assert!(rapid.contacts.iter().any(|c| c.target == "cutter_rapid_stock"));
            assert!(rapid.band_edits.is_empty());
        }
    }

    #[test]
    fn nominal_groove_gouge_checks_include_model_features_narrower_than_resolution() {
        use crate::machining::sequence::{Cutting, Groove, OperationSpan};
        let (mut setup, mut path) = fixture();
        path.job.profile = [(0., 16.), (-2., 16.), (-2., 12.), (-2.03, 12.), (-2.03, 16.), (-10., 16.)]
            .into_iter()
            .map(|(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm })
            .collect();
        path.job.model_source = Some(crate::machining::ModelSource {
            body: "analytic narrow groove".into(),
            reverse: false,
            origin: [0.; 3],
            axis: [0., 0., 1.],
            approximation_mm: 0.,
        });
        setup.od_job = Some(path.job.clone());
        setup.operations = vec![Operation {
            id: "narrow".into(),
            name: "narrow".into(),
            kind: OperationKind::OdGrooveRough,
            station: Some(11),
            spindle: None,
            manual_separation: None,
            cutting: Some(Cutting::OdGroove(Groove {
                front_z_mm: -2.,
                width_mm: 0.03,
                tool_width_mm: 0.03,
                bottom_diameter_mm: 10.,
                radial_depth_mm: 1.,
                feed_mm_rev: 0.1,
                ..Default::default()
            })),
        }];
        path.operation_spans = vec![OperationSpan { operation_id: "narrow".into(), start: 0, end: 1 }];
        path.moves = vec![Move {
            from: ProfilePoint { z_mm: -2., diameter_mm: 22. },
            to: ProfilePoint { z_mm: -2., diameter_mm: 10. },
            phase: Phase::OdGrooveRough,
            motion: Motion::Feed,
            feed_mm_rev: Some(0.1),
        }];
        let report = simulate(&setup, &path, 2.).unwrap();
        assert!(report.initial.iter().any(|s| (s.z0 + 2.03).abs() < 1e-8 && (s.z1 + 2.).abs() < 1e-8));
        assert!(report.contacts.iter().any(|c| c.target == "cutter_gouge"));
        // A different explicit manual target remains compatible, and is not a model check.
        setup.od_job.as_mut().unwrap().model_source = None;
        path.job.model_source = None;
        assert!(simulate(&setup, &path, 2.).unwrap().contacts.iter().all(|c| c.target != "cutter_gouge"));
    }
    fn fixture() -> (Setup, Toolpath) {
        let mut setup = TurningPlan::nl2500().setups.remove(0);
        let stock = Stock { outer_diameter_mm: 20., inner_diameter_mm: 4., length_mm: 12. };
        let job = OdJob {
            profile: vec![ProfilePoint { z_mm: 0., diameter_mm: 16. }, ProfilePoint { z_mm: -10., diameter_mm: 16. }],
            stock_front_z_mm: 1.,
            chuck_front_z_mm: -30.,
            clearance_mm: 1.,
            radial_depth_mm: 1.,
            face_depth_mm: 0.5,
            rough_feed_mm_rev: 0.2,
            finish_feed_mm_rev: 0.1,
            finish: true,
            ..Default::default()
        };
        setup.stock = Some(stock.clone());
        setup.od_job = Some(job.clone());
        for (i, kind) in
            [OperationKind::FaceRough, OperationKind::OdRough, OperationKind::FaceFinish, OperationKind::OdFinish].into_iter().enumerate()
        {
            setup.operations.push(Operation {
                spindle: None,
                cutting: None,
                id: format!("op-{i}"),
                name: "test".into(),
                kind,
                station: kind.suggested_station(),
                manual_separation: None,
            });
        }
        let path = crate::machining::interior::calculate_setup(Some(&job), None, &stock).unwrap();
        (setup, path)
    }
    #[test]
    fn missing_or_unconfirmed_holders_are_never_reported_as_checked() {
        let (mut setup, path) = fixture();
        let mut tool = ToolAssembly {
            triangular_insert: true,
            nose_radius_mm: Some(0.4),
            holder: Some(Holder { kind: HolderKind::Round, width_mm: 6., length_mm: 20., ..Default::default() }),
            holder_verified: false,
            insert: None,
        };
        setup.turret[11].simulation = Some(tool.clone());
        let r = simulate(&setup, &path, 0.5).unwrap();
        assert_eq!(r.missing_stations, vec![6, 12]);
        assert!(r.checked_stations.is_empty());
        assert_eq!(r.unchecked_moves, path.moves.len());
        assert!(!r.machine_ready);
        tool.holder_verified = true;
        tool.holder.as_mut().unwrap().width_mm = 0.;
        assert!(tool.validate().is_err());
        assert!(simulate(&setup, &path, f64::NAN).is_err());
    }

    #[test]
    fn clipped_toolpaths_check_insert_gouge_against_the_complete_od_and_id_target() {
        for inner in [false, true] {
            let (mut setup, _) = fixture();
            let mut job = setup.od_job.take().unwrap();
            let (front_diameter, rear_diameter) = if inner { (40., 10.) } else { (20., 50.) };
            job.profile = [(0., front_diameter), (-20.03, front_diameter), (-20.03, rear_diameter), (-60., rear_diameter)]
                .into_iter()
                .map(|(z_mm, diameter_mm)| ProfilePoint { z_mm, diameter_mm })
                .collect();
            job.stock_front_z_mm = 2.;
            job.chuck_front_z_mm = -75.;
            job.radial_depth_mm = 2.;
            job.finish = false;
            setup.stock = Some(Stock { outer_diameter_mm: 60., inner_diameter_mm: 4., length_mm: 82. });
            setup.cut_end_z_mm = Some(-19.);
            if inner {
                setup.id_job = Some(job);
            } else {
                setup.od_job = Some(job);
            }
            let kinds = if inner { vec![OperationKind::IdRough] } else { vec![OperationKind::FaceRough, OperationKind::OdRough] };
            setup.operations = kinds
                .into_iter()
                .enumerate()
                .map(|(index, kind)| Operation {
                    id: format!("clipped-{index}"),
                    name: "partial cut".into(),
                    kind,
                    station: kind.suggested_station(),
                    cutting: None,
                    spindle: None,
                    manual_separation: None,
                })
                .collect();
            let station = if inner { 5 } else { 12 };
            setup.turret.iter_mut().find(|slot| slot.station == station).unwrap().simulation = Some(ToolAssembly {
                triangular_insert: true,
                nose_radius_mm: Some(0.4),
                insert: Some(crate::insert::Insert { orientation_rad: std::f64::consts::FRAC_PI_2, inner, ..Default::default() }),
                ..Default::default()
            });
            let path = crate::machining::sequence::calculate(&setup).unwrap();
            assert_eq!(path.job.profile.last().unwrap().z_mm, -19.);
            assert!(path.moves.iter().all(|m| m.from.z_mm >= -19. - 1e-8 && m.to.z_mm >= -19. - 1e-8));
            let target = finished_band(&setup, &path, -20.5).unwrap();
            assert_eq!(target, if inner { [5., 30.] } else { [2., 25.] });
            let report = simulate(&setup, &path, 0.5).unwrap();
            assert!(report.initial.iter().any(|slab| (slab.z0 + 20.03).abs() < 1e-8));
            assert!(report.contacts.iter().any(|c| c.station == station && c.target == "insert_gouge"));
            // Keeping the clipped path job preserves legacy NC regeneration and line mapping.
            let nc = crate::machining::review_program(&path).unwrap();
            assert_eq!(nc.move_lines.len(), path.moves.len());
            assert!(nc.move_lines.iter().all(Option::is_some));
        }
    }

    #[test]
    fn continuous_sweep_detects_crossing_with_both_endpoints_clear() {
        let (mut setup, mut path) = fixture();
        let holder = Holder { kind: HolderKind::Round, length_mm: 1., width_mm: 6., ..Default::default() };
        setup.turret[11].simulation = Some(ToolAssembly { holder: Some(holder), holder_verified: true, ..Default::default() });
        path.moves = vec![Move {
            from: ProfilePoint { z_mm: 4., diameter_mm: 0. },
            to: ProfilePoint { z_mm: -15., diameter_mm: 0. },
            motion: Motion::Rapid,
            phase: Phase::OdRough,
            feed_mm_rev: None,
        }];
        let r = simulate(&setup, &path, 2.).unwrap();
        assert_eq!(r.contacts.len(), 1);
        assert_eq!(r.contacts[0].target, "remaining_stock");
        assert!(r.edits.is_empty());
        assert!(r.contacts[0].tip.z_mm < 4. && r.contacts[0].tip.z_mm > -15.);
    }
    #[test]
    fn stock_replay_preserves_partial_cells_and_finishing_allowance() {
        let mut cells = vec![Slab { z0: -1., z1: 0., inner: 2., outer: 10. }];
        remove(&mut cells, ProfilePoint { z_mm: 0., diameter_mm: 16.2 }, ProfilePoint { z_mm: -0.5, diameter_mm: 16.2 }, Phase::OdRough);
        assert_eq!(cells[0].outer, 10.);
        remove(&mut cells, ProfilePoint { z_mm: 0., diameter_mm: 16.2 }, ProfilePoint { z_mm: -1., diameter_mm: 16.2 }, Phase::OdRough);
        assert!((cells[0].outer - 8.1).abs() < 1e-10);
        let (setup, path) = fixture();
        let report = simulate(&setup, &path, 0.5).unwrap();
        let rough_end = path.moves.iter().position(|m| crate::machining::interior::is_finish(m.phase)).unwrap();
        let rough = report.stock_at(rough_end);
        let finished = report.stock_at(path.moves.len());
        let at = |s: &Vec<Slab>| s.iter().find(|s| s.z0 >= -5. && s.z1 <= -4.).unwrap().outer;
        assert!((at(&rough) - 8.1).abs() < 1e-8);
        assert!((at(&finished) - 8.).abs() < 1e-8);
        assert_eq!(report.stock_at(0)[0].outer, 10.);
    }
    #[test]
    fn bore_and_face_replay_keep_the_point_one_finishing_allowances() {
        let mut bore = vec![Slab { z0: -1., z1: 0., inner: 2., outer: 10. }];
        let point = |z, d| ProfilePoint { z_mm: z, diameter_mm: d };
        remove(&mut bore, point(0., 15.8), point(-1., 15.8), Phase::IdRough);
        assert!((bore[0].inner - 7.9).abs() < 1e-9);
        remove(&mut bore, point(0., 16.), point(-1., 16.), Phase::IdFinish);
        assert!((bore[0].inner - 8.).abs() < 1e-9);
        let mut face = vec![Slab { z0: 0., z1: 0.1, inner: 2., outer: 10. }, Slab { z0: 0.1, z1: 0.2, inner: 2., outer: 10. }];
        remove(&mut face, point(0.1, 22.), point(0.1, 4.), Phase::FaceRough);
        assert_eq!(face[0].outer, 10.);
        assert_eq!(face[1].outer, face[1].inner);
        remove(&mut face, point(0., 22.), point(0., 4.), Phase::FaceFinish);
        assert_eq!(face[0].outer, face[0].inner);
    }
    #[test]
    fn insert_radius_changes_cut_corner_and_rapid_never_removes_stock() {
        let (mut setup, mut path) = fixture();
        setup.turret[11].simulation = Some(ToolAssembly {
            triangular_insert: true,
            nose_radius_mm: Some(0.4),
            insert: Some(crate::insert::Insert::default()),
            ..Default::default()
        });
        path.moves = vec![Move {
            from: ProfilePoint { z_mm: 0., diameter_mm: 16. },
            to: ProfilePoint { z_mm: -10., diameter_mm: 16. },
            motion: Motion::Feed,
            phase: Phase::OdRough,
            feed_mm_rev: Some(0.1),
        }];
        let r04 = simulate(&setup, &path, 0.1).unwrap();
        setup.turret[11].simulation.as_mut().unwrap().nose_radius_mm = Some(0.2);
        let r02 = simulate(&setup, &path, 0.1).unwrap();
        let corner = |r: &Simulation| r.stock_at(1).iter().find(|s| (s.z0 + 9.9).abs() < 1e-8).unwrap().outer;
        assert!(corner(&r04) > corner(&r02) + 0.02, "R0.4 must leave a larger corner than R0.2");
        assert_eq!(r04.insert_stations, vec![12]);
        assert!(r04.checked_stations.is_empty(), "insert cutting must not certify holders");
        path.moves[0].motion = Motion::Rapid;
        let rapid = simulate(&setup, &path, 0.1).unwrap();
        assert!(rapid.band_edits.is_empty());
        assert!(rapid.contacts.iter().any(|c| c.target == "insert_rapid_stock"));
    }
    #[test]
    fn reversed_insert_reports_gouging_instead_of_clamping_to_the_target() {
        let (mut setup, mut path) = fixture();
        setup.turret[11].simulation = Some(ToolAssembly {
            triangular_insert: true,
            nose_radius_mm: Some(0.4),
            insert: Some(crate::insert::Insert { orientation_rad: -std::f64::consts::FRAC_PI_4, ..Default::default() }),
            ..Default::default()
        });
        path.moves = vec![Move {
            from: ProfilePoint { z_mm: 0., diameter_mm: 16. },
            to: ProfilePoint { z_mm: -10., diameter_mm: 16. },
            motion: Motion::Feed,
            phase: Phase::OdRough,
            feed_mm_rev: Some(0.1),
        }];
        let report = simulate(&setup, &path, 0.1).unwrap();
        assert!(report.contacts.iter().any(|c| c.target == "insert_gouge"));
        assert!(report.stock_at(1).iter().any(|s| s.z0 < 0. && s.z1 > -10. && s.outer < 8. - 0.01));
    }
    #[test]
    fn tangential_offset_changes_radial_clearance() {
        let p = ProfilePoint { z_mm: 0., diameter_mm: 0. };
        let h = Holder { kind: HolderKind::Round, length_mm: 10., width_mm: 2., t_center_mm: 10., ..Default::default() };
        let (_, _, low, high) = h.swept_bounds(p, p);
        assert_eq!((low, high), (9., 11.));
    }
}
