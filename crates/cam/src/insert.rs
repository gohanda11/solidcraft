//! Rounded equilateral insert section and continuous linear sweeps, authored analytically.
//! The programmed point is the intersection of Z/R tangents of the active nose circle.
use crate::{Result, invalid, machining::ProfilePoint};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Insert {
    pub inscribed_diameter_mm: f64,
    pub thickness_mm: f64,
    /// Direction from the active corner towards the body, measured from +Z towards +R.
    pub orientation_rad: f64,
    /// Mirror the section in R for a boring tool. Independent of the currently playing phase.
    pub inner: bool,
}
impl Default for Insert {
    fn default() -> Self {
        Self { inscribed_diameter_mm: 6.35, thickness_mm: 2.38, orientation_rad: std::f64::consts::FRAC_PI_4, inner: false }
    }
}
pub type Point = [f64; 2];
impl Insert {
    pub fn validate(&self, radius: f64) -> Result<()> {
        if !self.inscribed_diameter_mm.is_finite()
            || !(1.0..=30.).contains(&self.inscribed_diameter_mm)
            || !self.thickness_mm.is_finite()
            || !(0.1..=15.).contains(&self.thickness_mm)
            || !self.orientation_rad.is_finite()
            || self.orientation_rad.abs() > std::f64::consts::PI
            || !radius.is_finite()
            || !(0.05..=2.).contains(&radius)
            || 2. * radius >= self.inscribed_diameter_mm
        {
            return invalid("invalid triangular insert size, orientation or nose radius");
        }
        Ok(())
    }
    /// Inscribed polygon approximation of a triangle offset by the nose circle.
    /// 128 directions bound circular chord error below 0.00061 mm for R <= 2 mm.
    pub fn outline(&self, radius: f64) -> Result<Vec<Point>> {
        self.validate(radius)?;
        let core = self.inscribed_diameter_mm - 2. * radius;
        let (s, c) = self.orientation_rad.sin_cos();
        let sign = if self.inner { -1. } else { 1. };
        let centers = [[0., 0.], [1.5 * core, 3f64.sqrt() * core * 0.5], [1.5 * core, -3f64.sqrt() * core * 0.5]];
        let mut points = Vec::with_capacity(384);
        for [z, r] in centers {
            for i in 0..128 {
                let a = i as f64 * std::f64::consts::TAU / 128.;
                points.push([radius + z * c - r * s + radius * a.cos(), sign * (radius + z * s + r * c + radius * a.sin())]);
            }
        }
        Ok(hull(points))
    }
}
fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn hull(mut points: Vec<Point>) -> Vec<Point> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    let mut result: Vec<Point> = Vec::with_capacity(points.len() * 2);
    let mut half = |iter: Box<dyn Iterator<Item = Point>>| {
        let start = result.len();
        for p in iter {
            while result.len() >= start + 2 {
                let Some(b) = result.last().copied() else { break };
                let Some(a) = result.get(result.len() - 2).copied() else { break };
                if cross(a, b, p) > 1e-12 {
                    break;
                }
                result.pop();
            }
            result.push(p);
        }
        result.pop();
    };
    half(Box::new(points.clone().into_iter()));
    half(Box::new(points.into_iter().rev()));
    result
}
pub(crate) fn sweep(outline: &[Point], a: ProfilePoint, b: ProfilePoint) -> Vec<Point> {
    hull([a, b].into_iter().flat_map(|p| outline.iter().map(move |q| [q[0] + p.z_mm, q[1] + p.diameter_mm * 0.5])).collect())
}
/// Intersection of a convex swept polygon with one axial sample plane.
pub(crate) fn section(polygon: &[Point], z: f64) -> Option<[f64; 2]> {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()) {
        if z < a[0].min(b[0]) - 1e-10 || z > a[0].max(b[0]) + 1e-10 {
            continue;
        }
        if (b[0] - a[0]).abs() < 1e-10 {
            low = low.min(a[1].min(b[1]));
            high = high.max(a[1].max(b[1]));
        } else {
            let r = a[1] + (b[1] - a[1]) * (z - a[0]) / (b[0] - a[0]);
            low = low.min(r);
            high = high.max(r);
        }
    }
    (low.is_finite() && high > low).then_some([low, high])
}
pub(crate) fn subtract(bands: &mut Vec<[f64; 2]>, cut: [f64; 2]) {
    let mut next = Vec::with_capacity(bands.len() + 1);
    for &[lo, hi] in bands.iter() {
        if cut[1] <= lo || cut[0] >= hi {
            next.push([lo, hi]);
            continue;
        }
        if cut[0] > lo {
            next.push([lo, cut[0].min(hi)]);
        }
        if cut[1] < hi {
            next.push([cut[1].max(lo), hi]);
        }
    }
    *bands = next;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rounded_triangle_has_tangent_reference_and_mirrored_bore() {
        let insert = Insert::default();
        let p = insert.outline(0.4).unwrap();
        let cut = section(&p, 0.2).unwrap();
        let analytic = 0.4 - (0.4f64.powi(2) - 0.2f64.powi(2)).sqrt();
        assert!((cut[0] - analytic).abs() < 0.001);
        assert!(section(&p, -0.001).is_none());
        let inner = Insert { inner: true, ..insert }.outline(0.4).unwrap();
        let mirrored = section(&inner, 0.2).unwrap();
        assert!((mirrored[1] + cut[0]).abs() < 1e-9);
        assert!(Insert::default().outline(f64::NAN).is_err());
    }
    #[test]
    fn continuous_sweep_keeps_disconnected_remaining_material() {
        let p = Insert::default().outline(0.4).unwrap();
        let swept = sweep(&p, ProfilePoint { z_mm: 0., diameter_mm: 10. }, ProfilePoint { z_mm: -10., diameter_mm: 10. });
        let cut = section(&swept, -5.).unwrap();
        assert!((cut[0] - 5.).abs() < 0.001);
        let mut bands = vec![[0., 20.]];
        subtract(&mut bands, [5., 7.]);
        assert_eq!(bands, vec![[0., 5.], [7., 20.]]);
    }
}
