//! Editable project trial presets, not manufacturer-approved machining data.
use crate::{Setup, machining::OdJob};
pub const MATERIALS: &[&str] = &["S45C", "SUS303", "SUS304", "HPM1", "樹脂", "真鍮", "銅"];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Level {
    Gentle,
    #[default]
    Standard,
    Productive,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conditions {
    pub depth: f64,
    pub face_depth: f64,
    pub rough_feed: f64,
    pub finish_feed: f64,
}
pub fn candidate(material: &str, level: Level, inner: bool) -> Option<Conditions> {
    let (depth, rough_feed, finish_feed) = match material {
        "S45C" => (1.0, 0.15, 0.08),
        "SUS303" => (0.8, 0.12, 0.06),
        "SUS304" => (0.5, 0.10, 0.05),
        "HPM1" => (0.4, 0.10, 0.05),
        "樹脂" => (0.8, 0.12, 0.08),
        "真鍮" => (1.0, 0.15, 0.08),
        "銅" => (0.6, 0.10, 0.06),
        _ => return None,
    };
    let (d, f) = match level {
        Level::Gentle => (0.6, 0.75),
        Level::Standard => (1., 1.),
        Level::Productive => (1.3, 1.15),
    };
    let depth = depth * d * if inner { 0.6 } else { 1. };
    Some(Conditions { depth, face_depth: depth.min(0.8), rough_feed: rough_feed * f * if inner { 0.8 } else { 1. }, finish_feed: finish_feed * f })
}
impl Conditions {
    pub fn apply(self, job: &mut OdJob) {
        job.radial_depth_mm = self.depth;
        job.face_depth_mm = self.face_depth;
        job.rough_feed_mm_rev = self.rough_feed;
        job.finish_feed_mm_rev = self.finish_feed;
    }
}
pub fn apply(setup: &mut Setup, material: &str, level: Level) {
    if let (Some(job), Some(c)) = (&mut setup.od_job, candidate(material, level, false)) {
        c.apply(job);
    }
    if let (Some(job), Some(c)) = (&mut setup.id_job, candidate(material, level, true)) {
        let face_depth = job.face_depth_mm;
        c.apply(job);
        job.face_depth_mm = face_depth;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels_are_ordered_and_applying_preserves_geometry_and_allowance() {
        for m in MATERIALS {
            let low = candidate(m, Level::Gentle, false).unwrap();
            let mid = candidate(m, Level::Standard, false).unwrap();
            let high = candidate(m, Level::Productive, false).unwrap();
            assert!(low.depth < mid.depth && mid.depth < high.depth && low.rough_feed < mid.rough_feed && mid.rough_feed < high.rough_feed);
            assert!(candidate(m, Level::Standard, true).unwrap().depth < mid.depth);
            let mut job = OdJob { stock_front_z_mm: 2., chuck_front_z_mm: -70., clearance_mm: 3., ..Default::default() };
            mid.apply(&mut job);
            assert_eq!((job.radial_stock_to_leave_mm, job.axial_stock_to_leave_mm, job.chuck_front_z_mm, job.clearance_mm), (0.1, 0.1, -70., 3.));
        }
        assert!(candidate("custom", Level::Standard, false).is_none());
    }
}
