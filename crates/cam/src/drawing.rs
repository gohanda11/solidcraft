//! Saved section annotations. Nominal dimensions remain linked to the source model.
use crate::{Result, invalid, machining::ProfilePoint};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    #[default]
    Clamped,
    Reverse,
    PartOff,
    PartingFinish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Outer,
    Inner,
    Front,
    Back,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub side: Side,
    pub a: ProfilePoint,
    pub b: ProfilePoint,
    pub lower_mm: f64,
    pub upper_mm: f64,
    /// Annotation only; thread toolpaths are not implemented.
    pub thread_pitch_mm: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drawing {
    pub segments: Vec<Segment>,
    /// Includes the axis in solid sections; blind holes are display-only for now.
    pub inner: Vec<ProfilePoint>,
}

impl Drawing {
    pub fn new(outer: &[ProfilePoint], inner: Vec<ProfilePoint>) -> Self {
        let mut segments = Vec::new();
        let mut add = |side, a, b| {
            if a != b {
                segments.push(Segment { side, a, b, lower_mm: 0., upper_mm: 0., thread_pitch_mm: None });
            }
        };
        for p in outer.windows(2) {
            if let [a, b] = p {
                add(Side::Outer, *a, *b);
            }
        }
        if let Some((a, b)) = outer.last().zip(inner.last()) {
            add(Side::Back, *a, *b);
        }
        for p in inner.windows(2).rev() {
            if let [a, b] = p {
                add(Side::Inner, *b, *a);
            }
        }
        if let Some((a, b)) = inner.first().zip(outer.first()) {
            add(Side::Front, *a, *b);
        }
        Self { segments, inner }
    }
    pub fn validate(&self) -> Result<()> {
        if self.segments.len() > 1024 || self.inner.len() > 256 {
            return invalid("section point budget exceeded");
        }
        for p in self.inner.iter().chain(self.segments.iter().flat_map(|s| [&s.a, &s.b])) {
            if !p.z_mm.is_finite() || p.z_mm.abs() > 1e6 || !p.diameter_mm.is_finite() || !(0.0..=1e6).contains(&p.diameter_mm) {
                return invalid("invalid section coordinate");
            }
        }
        for s in &self.segments {
            if !s.lower_mm.is_finite()
                || !s.upper_mm.is_finite()
                || s.lower_mm > s.upper_mm
                || s.lower_mm.abs().max(s.upper_mm.abs()) > 100.
                || s.thread_pitch_mm.is_some_and(|p| !p.is_finite() || !(0.01..=100.).contains(&p))
            {
                return invalid("invalid section tolerance or thread pitch");
            }
        }
        Ok(())
    }
}

/// Splitting a straight segment keeps the original geometry and source linkage.
pub fn simplified(points: &[ProfilePoint]) -> Vec<ProfilePoint> {
    let mut result: Vec<ProfilePoint> = Vec::new();
    for p in points {
        while result.len() >= 2 {
            let (Some(a), Some(b)) = (result.get(result.len() - 2), result.last()) else { break };
            let dz = p.z_mm - a.z_mm;
            let dd = p.diameter_mm - a.diameter_mm;
            // Equal endpoints can enclose a real out-and-back edge, not a redundant midpoint.
            if dz == 0. && dd == 0. {
                break;
            }
            let cross = (b.z_mm - a.z_mm) * dd - (b.diameter_mm - a.diameter_mm) * dz;
            let dot = (b.z_mm - a.z_mm) * dz + (b.diameter_mm - a.diameter_mm) * dd;
            if cross.abs() > 1e-8 * dz.hypot(dd).max(1.) || dot < 0. || dot > dz * dz + dd * dd {
                break;
            }
            result.pop();
        }
        result.push(*p);
    }
    result
}
