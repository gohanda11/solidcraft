//! Bounded recognition of coaxial, piecewise-linear surfaces of revolution.
//! No display mesh or screen projection is used for machining dimensions.
use crate::body::from_p3;
use crate::offset::{Surf, surf_of};
use crate::{Body, KernelError, Result, guard};
use solidcraft_geom::Vec3;

#[derive(Clone, Debug)]
pub struct TurningOutline {
    pub origin: Vec3,
    pub axis: Vec3,
    /// (Z relative to the frontmost face, diameter), ordered towards the chuck.
    pub points: Vec<(f64, f64)>,
    pub approximation_mm: f64,
}

/// A circumscribed polyline for an outer circular branch, inscribed for an inner
/// branch: both leave material outside the true meridian. Error is measured normal
/// to the circle, not radially at a vertical tangent. Bound: 0.001 mm.
fn circular_spans(lo: f64, hi: f64, center_z: f64, center_r: f64, radius: f64, sign: f64, inner: bool) -> Result<Vec<Span>> {
    if radius <= 0.0 || !radius.is_finite() || (lo - center_z).abs() > radius + 1e-6 || (hi - center_z).abs() > radius + 1e-6 {
        return Err(fail("invalid circular meridian"));
    }
    let a = ((lo - center_z) / radius).clamp(-1., 1.).asin();
    let b = ((hi - center_z) / radius).clamp(-1., 1.).asin();
    let step = 2.0 * (radius / (radius + 0.001)).acos();
    let count = ((b - a) / step).ceil().max(1.0);
    if !count.is_finite() || count > 240.0 {
        return Err(fail("circular meridian exceeds the point budget"));
    }
    let count = count as usize;
    let at = |t: f64, scale: f64| (center_z + radius * scale * t.sin(), center_r + sign * radius * scale * t.cos());
    let mut p = vec![at(a, 1.0)];
    for i in 0..count {
        let t0 = a + (b - a) * i as f64 / count as f64;
        let t1 = a + (b - a) * (i + 1) as f64 / count as f64;
        if (sign > 0.0) != inner {
            p.push(at((t0 + t1) * 0.5, 1.0 / ((t1 - t0) * 0.5).cos()));
        } else {
            p.push(at(t1, 1.0));
        }
    }
    if (sign > 0.0) != inner {
        p.push(at(b, 1.0));
    }
    let mut spans = Vec::new();
    for pair in p.windows(2) {
        let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else { continue };
        if b.0 - a.0 <= 1e-9 {
            continue;
        }
        let slope = (b.1 - a.1) / (b.0 - a.0);
        spans.push(Span { lo: a.0, hi: b.0, slope, intercept: a.1 - slope * a.0 });
    }
    Ok(spans)
}

fn fail(message: &str) -> KernelError {
    KernelError::Invalid(format!("turning profile: {message}"))
}

#[derive(Clone, Copy)]
struct Span {
    lo: f64,
    hi: f64,
    slope: f64,
    intercept: f64,
}
impl Span {
    fn radius(self, z: f64) -> f64 {
        self.slope * z + self.intercept
    }
}

/// Recognize all faces, then take the exterior meridian. Planes must be normal to
/// the one common axis. Flats, cross holes, mesh bodies and unsupported curves fail
/// as a whole instead of silently omitting faces. Reverse selects the opposite end.
pub fn turning_outline(body: &Body, reverse: bool) -> Result<TurningOutline> {
    outline(body, reverse, false, false)
}

/// Extract only a continuous through bore. Blind holes and multiple radial walls are refused.
pub fn turning_inner_outline(body: &Body, reverse: bool) -> Result<TurningOutline> {
    outline(body, reverse, true, false)
}

/// Inner boundary for a section drawing, including the axis in solid regions.
/// This does not claim that a blind bore is reachable by the through-bore planner.
pub fn turning_section_inner_outline(body: &Body, reverse: bool) -> Result<TurningOutline> {
    outline(body, reverse, true, true)
}

fn outline(body: &Body, reverse: bool, inner: bool, section: bool) -> Result<TurningOutline> {
    guard("turning outline", || {
        const TOL: f64 = 1e-6;
        if body.is_mesh() || body.shell_count() != 1 || body.face_count() > 512 || body.edge_count() > 4096 || !body.validity().is_empty() {
            return Err(fail("requires one closed B-rep solid, at most 512 faces / 4096 edges"));
        }
        let surfaces: Vec<_> = body.solid.face_iter().map(|f| surf_of(f, TOL)).collect();
        let (o, mut axis) = surfaces
            .iter()
            .find_map(|s| match s {
                Some(Surf::Cylinder { o, a, .. }) => Some((*o, *a)),
                Some(Surf::Cone { v, a, .. }) => Some((*v, *a)),
                _ => None,
            })
            .ok_or_else(|| fail("no cylinder or cone axis found"))?;
        let largest = [axis.x, axis.y, axis.z].into_iter().max_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap_or(1.0);
        if (largest < 0.0) != reverse {
            axis = -axis;
        }
        let aligned = |p: Vec3, a: Vec3| a.cross(axis).len() <= 1e-8 && ((p - o) - axis * (p - o).dot(axis)).len() <= TOL;
        let mut spans = Vec::new();
        let mut knots = Vec::new();
        let mut approximation_mm: f64 = 0.0;
        for (face, surface) in body.solid.face_iter().zip(surfaces) {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            let mut boundary = Vec::new();
            for edge in face.boundaries().iter().flat_map(|wire| wire.iter()) {
                for v in [edge.front(), edge.back()] {
                    let p = from_p3(v.point());
                    if !p.is_finite() || p.len() > 1e6 {
                        return Err(fail("coordinates exceed the supported range"));
                    }
                    let z = (p - o).dot(axis);
                    boundary.push((z, ((p - o) - axis * z).len()));
                    lo = lo.min(z);
                    hi = hi.max(z);
                }
            }
            if !lo.is_finite() || !hi.is_finite() {
                return Err(fail("face has no finite boundary"));
            }
            let (slope, intercept) = match surface {
                Some(Surf::Plane { n, .. }) if n.cross(axis).len() <= 1e-8 && hi - lo <= TOL => continue,
                Some(Surf::Cylinder { o: p, a, r, .. }) if aligned(p, a) => (0.0, r),
                Some(Surf::Cone { v, a, half, .. }) if aligned(v, a) => {
                    let slope = a.dot(axis) * half.tan();
                    (slope, -slope * (v - o).dot(axis))
                }
                Some(Surf::Torus { o: p, a, big, h, r, .. }) if aligned(p, a) => {
                    let z = (p + a * h - o).dot(axis);
                    let delta = boundary.iter().map(|(_, r)| r - big).max_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap_or(0.0);
                    if delta.abs() < TOL || boundary.iter().any(|(_, radius)| (radius - big) * delta < -TOL) {
                        return Err(fail("torus face crosses both radial branches"));
                    }
                    let arc = circular_spans(lo, hi, z, big, r, delta.signum(), inner)?;
                    for span in arc {
                        knots.extend([span.lo, span.hi]);
                        spans.push(span);
                    }
                    approximation_mm = 0.001;
                    continue;
                }
                _ => {
                    return Err(fail(
                        "all faces must be coaxial cylinders/cones/circular fillets or perpendicular planes; freeform faces and non-turning features are not supported",
                    ));
                }
            };
            let span = Span { lo, hi, slope, intercept };
            if hi - lo <= TOL || !slope.is_finite() || !intercept.is_finite() || span.radius(lo) < -TOL || span.radius(hi) < -TOL {
                return Err(fail("degenerate cylindrical or conical face"));
            }
            spans.push(span);
            knots.extend([lo, hi]);
        }
        knots.sort_by(|a, b| b.total_cmp(a));
        knots.dedup_by(|a, b| (*a - *b).abs() <= TOL);
        let front = *knots.first().ok_or_else(|| fail("no exterior profile"))?;
        let mut points: Vec<(f64, f64)> = Vec::new();
        for pair in knots.windows(2) {
            let (Some(&hi), Some(&lo)) = (pair.first(), pair.get(1)) else { continue };
            let mid = (hi + lo) * 0.5;
            let mut active: Vec<_> = spans.iter().filter(|s| s.lo < mid && s.hi > mid).copied().collect();
            active.sort_by(|a, b| a.radius(mid).total_cmp(&b.radius(mid)));
            active.dedup_by(|a, b| (a.radius(lo) - b.radius(lo)).abs() <= TOL && (a.radius(hi) - b.radius(hi)).abs() <= TOL);
            if inner && !section && (active.len() != 2 || active.first().is_none_or(|s| s.radius(lo).min(s.radius(hi)) <= TOL)) {
                return Err(fail(
                    "inner profile requires one continuous through bore; blind holes, solid sections and multiple radial walls are unsupported",
                ));
            }
            if section && !(1..=2).contains(&active.len()) {
                return Err(fail("section requires a single radial material band"));
            }
            let outer = active.iter().max_by(|a, b| a.radius(mid).total_cmp(&b.radius(mid))).ok_or_else(|| fail("disconnected axial profile"))?;
            if active.iter().any(|s| s.radius(lo) > outer.radius(lo) + TOL || s.radius(hi) > outer.radius(hi) + TOL) {
                return Err(fail("ambiguous intersecting surfaces"));
            }
            let selected = if inner { active.first().ok_or_else(|| fail("no through bore"))? } else { outer };
            for z in [hi, lo] {
                let p = (z - front, if section && active.len() == 1 { 0.0 } else { 2.0 * selected.radius(z).max(0.0) });
                if points.last().is_some_and(|q| (q.0 - p.0).abs() < TOL && (q.1 - p.1).abs() < TOL) {
                    continue;
                }
                // Collapse redundant collinear stations, keeping both sides of shoulders.
                while points.len() >= 2 {
                    let (Some(a), Some(b)) = (points.get(points.len() - 2), points.last()) else { break };
                    let cross = (b.0 - a.0) * (p.1 - b.1) - (b.1 - a.1) * (p.0 - b.0);
                    if cross.abs() > TOL * ((p.0 - a.0).hypot(p.1 - a.1)).max(1.0) {
                        break;
                    }
                    points.pop();
                }
                points.push(p);
                if points.len() > 256 {
                    return Err(fail("profile exceeds 256 points"));
                }
            }
        }
        if points.len() < 2 {
            return Err(fail("profile has no axial length"));
        }
        Ok(TurningOutline { origin: o + axis * front, axis, points, approximation_mm })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};

    #[test]
    fn section_closes_solid_and_blind_regions_on_the_axis() {
        let solid = turning_section_inner_outline(&stepped(), false).unwrap();
        assert!(solid.points.iter().all(|p| p.1 == 0.));
        let p = [(0., 10.), (0., 20.), (-60., 20.), (-60., 0.), (-40., 0.), (-40., 10.)];
        let region = Region2 { outer: Loop2::polygon(&p.map(|(x, y)| Vec2::new(x, y))), holes: vec![] };
        let bodies = crate::revolve(&Plane::XY, &[region], Vec2::ZERO, Vec2::new(1., 0.), std::f64::consts::TAU).unwrap();
        let body = &bodies[0];
        assert!(turning_inner_outline(body, false).is_err());
        let boundary = turning_section_inner_outline(body, false).unwrap();
        assert_eq!(boundary.points.len(), 4);
        for (actual, expected) in boundary.points.iter().zip([(0., 20.), (-40., 20.), (-40., 0.), (-60., 0.)]) {
            assert!((actual.0 - expected.0).abs() < 1e-6 && (actual.1 - expected.1).abs() < 1e-6);
        }
        let reversed = turning_section_inner_outline(body, true).unwrap();
        assert!(reversed.points.first().unwrap().1.abs() < 1e-6);
        assert!((reversed.points.last().unwrap().1 - 20.).abs() < 1e-6);
    }

    fn stepped() -> Body {
        let p = [(0., 0.), (0., 10.), (-20., 10.), (-20., 20.), (-40., 20.), (-40., 25.), (-60., 25.), (-60., 0.)];
        let region = Region2 { outer: Loop2::polygon(&p.map(|(x, y)| Vec2::new(x, y))), holes: vec![] };
        crate::revolve(&Plane::XY, &[region], Vec2::ZERO, Vec2::new(1., 0.), std::f64::consts::TAU).unwrap().remove(0)
    }
    #[test]
    fn turning_outline_preserves_shoulder_pairs_and_supports_reversal() {
        let b = stepped();
        let outline = turning_outline(&b, false).unwrap();
        let expected = [(0., 20.), (-20., 20.), (-20., 40.), (-40., 40.), (-40., 50.), (-60., 50.)];
        assert_eq!(outline.points.len(), expected.len());
        for (a, b) in outline.points.iter().zip(expected) {
            assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6, "{a:?} != {b:?}");
        }
        let reverse = turning_outline(&b, true).unwrap();
        assert!((reverse.points[0].1 - 50.).abs() < 1e-6);
        assert!(reverse.origin.dist(Vec3::new(-60., 0., 0.)) < 1e-6);
        let placed = crate::transform(&b, Vec3::new(6., 9., 11.), Vec3::ZERO, Vec3::Z, 0.37).unwrap();
        let moved = turning_outline(&placed, false).unwrap();
        for (a, b) in moved.points.iter().zip(expected) {
            assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
        }
    }
    #[test]
    fn turning_outline_rejects_non_rotational_geometry() {
        let b = crate::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
        assert!(turning_outline(&b, false).is_err());
    }

    #[test]
    fn turning_outline_circular_approximation_leaves_material_within_normal_tolerance() {
        for sign in [-1.0, 1.0] {
            let spans = circular_spans(-1.0, 0.0, 0.0, 20.0, 1.0, sign, false).unwrap();
            for s in spans {
                for i in 0..=100 {
                    let z = s.lo + (s.hi - s.lo) * i as f64 / 100.0;
                    let r = s.radius(z);
                    let true_r = 20.0 + sign * (1.0 - z * z).max(0.0).sqrt();
                    assert!(r >= true_r - 1e-8, "must not gouge the circle");
                    let normal_error = ((r - 20.0).hypot(z) - 1.0).abs();
                    assert!(normal_error <= 0.001 + 1e-8, "{normal_error}");
                }
            }
        }
        let rounded = crate::fillet(&stepped(), &[Vec3::new(-40., 25., 0.)], 1.0).unwrap();
        let profile = turning_outline(&rounded, false).unwrap();
        assert_eq!(profile.approximation_mm, 0.001);
        assert!(profile.points.len() > 6);
    }

    #[test]
    fn turning_outline_reads_step_roundtrip_and_hollow_taper_without_using_bore_as_od() {
        let p = [(0., 5.), (0., 10.), (-20., 20.), (-40., 20.), (-40., 5.)];
        let region = Region2 { outer: Loop2::polygon(&p.map(|(x, y)| Vec2::new(x, y))), holes: vec![] };
        let body = crate::revolve(&Plane::XY, std::slice::from_ref(&region), Vec2::ZERO, Vec2::new(1., 0.), std::f64::consts::TAU).unwrap().remove(0);
        let step = crate::step_export(&[&body], "SolidCraft test").unwrap();
        let imported = crate::step_import(&step).unwrap();
        for b in [&body, &imported.bodies[0].body] {
            let outline = turning_outline(b, false).unwrap();
            let expected = [(0., 20.), (-20., 40.), (-40., 40.)];
            assert_eq!(outline.points.len(), expected.len(), "{:?}", outline.points);
            for (a, b) in outline.points.iter().zip(expected) {
                assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
            }
        }
        let partial = crate::revolve(&Plane::XY, &[region], Vec2::ZERO, Vec2::new(1., 0.), std::f64::consts::PI).unwrap().remove(0);
        assert!(turning_outline(&partial, false).is_err(), "a half revolution must not be read as a full solid");
    }
    #[test]
    fn turning_outline_inner_stepped_taper_step_roundtrip_and_blind_rejection() {
        let p = [(0., 15.), (0., 20.), (-20., 20.), (-20., 23.), (-60., 23.), (-60., 8.), (-40., 10.), (-20., 10.), (-20., 15.)];
        let region = Region2 { outer: Loop2::polygon(&p.map(|(x, y)| Vec2::new(x, y))), holes: vec![] };
        let body = crate::revolve(&Plane::XY, &[region], Vec2::ZERO, Vec2::new(1., 0.), std::f64::consts::TAU).unwrap().remove(0);
        let step = crate::step_export(&[&body], "SolidCraft ID test").unwrap();
        let imported = crate::step_import(&step).unwrap();
        for b in [&body, &imported.bodies[0].body] {
            let outline = turning_inner_outline(b, false).unwrap();
            let expected = [(0., 30.), (-20., 30.), (-20., 20.), (-40., 20.), (-60., 16.)];
            assert_eq!(outline.points.len(), expected.len(), "{:?}", outline.points);
            for (p, q) in outline.points.iter().zip(expected) {
                assert!((p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6);
            }
            let reverse = turning_inner_outline(b, true).unwrap();
            assert!((reverse.points[0].1 - 16.).abs() < 1e-6);
        }
        let p = [(0., 10.), (0., 20.), (-60., 20.), (-60., 0.), (-40., 0.), (-40., 10.)];
        let region = Region2 { outer: Loop2::polygon(&p.map(|(x, y)| Vec2::new(x, y))), holes: vec![] };
        let blind = crate::revolve(&Plane::XY, &[region], Vec2::ZERO, Vec2::new(1., 0.), std::f64::consts::TAU).unwrap().remove(0);
        assert!(turning_inner_outline(&blind, false).is_err());
        assert!(turning_inner_outline(&stepped(), false).is_err());
        // A real bore fillet must be recognized without using the outer meridian.
        let filleted = crate::fillet(&body, &[Vec3::new(-20., 10., 0.)], 1.).unwrap();
        let inner = turning_inner_outline(&filleted, false).unwrap();
        assert!(inner.points.len() > 5);
        assert_eq!(inner.approximation_mm, 0.001);
        for sign in [-1., 1.] {
            for span in circular_spans(-1., 0., 0., 20., 1., sign, true).unwrap() {
                for n in 0..=100 {
                    let z = span.lo + (span.hi - span.lo) * n as f64 / 100.;
                    let r = span.radius(z);
                    assert!(r <= 20. + sign * (1. - z * z).max(0.).sqrt() + 1e-8);
                    assert!(((r - 20.).hypot(z) - 1.).abs() <= 0.001 + 1e-8);
                }
            }
        }
    }
}
