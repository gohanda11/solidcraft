//! User-supplied STEP display snapshots. Never used as certified collision geometry.
use serde::{Deserialize, Serialize};

use crate::{Result, invalid};

pub const MAX_TRIANGLES: usize = 100_000;
pub const MAX_VERTICES: usize = 150_000;

/// Unmounted display models saved with the job, independent of turret assignments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryEntry {
    pub kind: Kind,
    pub cad: std::sync::Arc<HolderCad>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Holder,
    Insert,
}

pub fn find<'a>(library: &'a [LibraryEntry], holder: bool, model: &str) -> Option<&'a HolderCad> {
    let kind = if holder { Kind::Holder } else { Kind::Insert };
    library.iter().find(|e| e.kind == kind && e.cad.model == model).map(|e| e.cad.as_ref())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolderCad {
    pub model: String,
    /// Basename only; the saved preview does not depend on the original file location.
    pub file_name: String,
    pub tessellation_mm: f64,
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl HolderCad {
    pub fn validate(&self) -> Result<()> {
        if self.model.is_empty()
            || self.model.len() > 100
            || !self.model.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || self.file_name.is_empty()
            || self.file_name.len() > 255
            || self.file_name.chars().any(|c| c.is_control() || c == '/' || c == '\\')
        {
            return invalid("invalid holder CAD identity");
        }
        if !self.tessellation_mm.is_finite()
            || !(0.01..=0.5).contains(&self.tessellation_mm)
            || self.positions.len() < 3
            || self.positions.len() > MAX_VERTICES
            || self.triangles.is_empty()
            || self.triangles.len() > MAX_TRIANGLES
            || self.positions.iter().flatten().any(|v| !v.is_finite() || v.abs() > 10_000.)
            || self.triangles.iter().flatten().any(|i| *i as usize >= self.positions.len())
        {
            return invalid("invalid or oversized holder CAD mesh");
        }
        Ok(())
    }

    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let first = *self.positions.first()?;
        Some(self.positions.iter().fold((first, first), |(mut lo, mut hi), p| {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
            (lo, hi)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_holder_meshes_are_rejected() {
        let mut cad = HolderCad {
            model: "H-1".into(),
            file_name: "holder.stp".into(),
            tessellation_mm: 0.1,
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            triangles: vec![[0, 1, 2]],
        };
        assert!(cad.validate().is_ok());
        cad.triangles.push([0, 1, u32::MAX]);
        assert!(cad.validate().is_err());
        cad.triangles.pop();
        cad.positions[0][0] = f64::NAN;
        assert!(cad.validate().is_err());
        cad.positions[0][0] = 0.;
        cad.file_name = "../holder.stp".into();
        assert!(cad.validate().is_err());
    }
}
