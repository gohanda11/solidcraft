//! Turning process intent, independent of CAD topology, the UI and machine code.
//! This first slice stores plans; it does not calculate paths or certify machining.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub mod catalog;
pub mod drawing;
pub mod holder_cad;
pub mod insert;
pub mod machining;
pub mod post;
pub mod presets;
pub mod simulation;

#[derive(Debug, thiserror::Error)]
#[error("invalid turning plan: {0}")]
pub struct PlanError(pub String);

pub type Result<T> = std::result::Result<T, PlanError>;

/// Millimetres throughout. Diameter fields are explicitly named; they are not radial X.
/// Each setup owns a tool snapshot so edits to a template cannot change an older job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurningPlan {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_cad_library: Vec<holder_cad::LibraryEntry>,
    pub machine: String,
    pub material: String,
    pub setups: Vec<Setup>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing: Option<drawing::Drawing>,
    #[serde(default)]
    pub ending: drawing::Ending,
    /// Optional machine dialect; absent in older plans, which keep their review fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<post::NlPost>,
    pub id: String,
    pub name: String,
    /// Planning dependency only. Stock transfer and a re-clamping transform are not computed.
    pub previous_setup: Option<String>,
    /// None means stock still needs defining; dimensions are never silently invented.
    pub stock: Option<Stock>,
    pub turret: Vec<ToolSlot>,
    pub operations: Vec<Operation>,
    /// Explicit manual OD profile and cutting conditions; paths are always regenerated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub od_job: Option<machining::OdJob>,
    /// Inner through-bore profile, using the same Z0 and stock as the OD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_job: Option<machining::OdJob>,
    /// Shared OD/ID end plane, measured from the finished front face. None uses the full model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_end_z_mm: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stock {
    pub outer_diameter_mm: f64,
    pub inner_diameter_mm: f64,
    pub length_mm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRole {
    CenterDrill,
    Drill,
    Vacant,
    IdRough,
    OdFinish,
    IdFinish,
    GrooveOrThread,
    OdGrooveRough,
    OdRough,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSlot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_cad: Option<std::sync::Arc<holder_cad::HolderCad>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder_cad: Option<std::sync::Arc<holder_cad::HolderCad>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<catalog::Selection>,
    pub station: u8,
    pub role: ToolRole,
    /// Actual tool/holder identification, not a claim that its cutting geometry is known.
    pub tool_name: Option<String>,
    /// Must be supplied from the machine; not assumed equal to the station number.
    pub offset_number: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation: Option<simulation::ToolAssembly>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// Legacy undivided facing intent; retained for reading earlier plans.
    Face,
    FaceRough,
    FaceFinish,
    CenterDrill,
    Drill,
    OdRough,
    OdFinish,
    IdRough,
    IdFinish,
    OdGroove,
    OdGrooveRough,
    OdGrooveFinish,
    IdGroove,
    FaceGroove,
    OdThread,
    IdThread,
    PartOff,
    PartingFinish,
}

impl OperationKind {
    /// Suggestions from the owner's standard turret, never a forced tool assignment.
    pub fn suggested_station(self) -> Option<u8> {
        match self {
            Self::CenterDrill => Some(1),
            Self::Drill => Some(2),
            Self::IdRough => Some(5),
            Self::OdFinish | Self::FaceFinish => Some(6),
            Self::IdFinish => Some(7),
            Self::OdGrooveRough => Some(11),
            Self::OdGrooveFinish => Some(8),
            Self::OdRough | Self::FaceRough => Some(12),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    /// Optional explicit override for the configured NC dialect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spindle: Option<post::Spindle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutting: Option<machining::sequence::Cutting>,
    pub id: String,
    pub name: String,
    pub kind: OperationKind,
    pub station: Option<u8>,
    /// Present only for PartingFinish, which ends with a separate manual removal step.
    pub manual_separation: Option<ManualSeparation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualSeparation {
    /// Operator-specified finished bore at the separation plane, not the raw stock bore.
    /// A later slice will bind this to the preceding bore operation and its revision.
    pub finished_bore_diameter_mm: f64,
    /// Initial suggestion equals the finished bore. No residual wall thickness is assumed.
    pub stop_diameter_mm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedOutcome {
    RemainsClamped,
    CutOff,
    ManualSeparationRequired,
}

impl Setup {
    /// A planned process state, not a simulation result.
    pub fn planned_outcome(&self) -> PlannedOutcome {
        match self.operations.last().map(|operation| operation.kind) {
            Some(OperationKind::PartOff) => PlannedOutcome::CutOff,
            Some(OperationKind::PartingFinish) => PlannedOutcome::ManualSeparationRequired,
            _ => PlannedOutcome::RemainsClamped,
        }
    }
}

pub fn nl2500_turret() -> Vec<ToolSlot> {
    use ToolRole::*;
    [CenterDrill, Drill, Vacant, Vacant, IdRough, OdFinish, IdFinish, GrooveOrThread, GrooveOrThread, GrooveOrThread, OdGrooveRough, OdRough]
        .into_iter()
        .zip(1..=12)
        .map(|(role, station)| ToolSlot {
            station,
            role,
            tool_name: None,
            offset_number: None,
            simulation: None,
            catalog: None,
            holder_cad: None,
            insert_cad: None,
        })
        .collect()
}

impl TurningPlan {
    pub fn nl2500() -> Self {
        Self {
            tool_cad_library: Vec::new(),
            machine: "NL2500".into(),
            material: String::new(),
            setups: vec![Setup {
                drawing: None,
                ending: drawing::Ending::default(),
                post: Some(post::NlPost::default()),
                id: "setup-1".into(),
                name: "Setup 1".into(),
                previous_setup: None,
                stock: None,
                turret: nl2500_turret(),
                operations: Vec::new(),
                od_job: None,
                id_job: None,
                cut_end_z_mm: None,
            }],
        }
    }

    /// Structural validation only: a valid draft is NOT ready to generate NC.
    pub fn validate(&self) -> Result<()> {
        text("machine", &self.machine, false)?;
        text("material", &self.material, true)?;
        if self.setups.is_empty() || self.setups.len() > 32 {
            return invalid("expected 1 to 32 setups");
        }
        let mut setups = BTreeSet::new();
        let mut operations = BTreeSet::new();
        let mut cad_triangles = 0usize;
        if self.tool_cad_library.len() > 128 {
            return invalid("tool CAD library may contain at most 128 models");
        }
        let mut models = BTreeSet::new();
        for entry in &self.tool_cad_library {
            entry.cad.validate()?;
            if !models.insert((entry.kind, entry.cad.model.as_str())) {
                return invalid("duplicate tool CAD library model");
            }
            cad_triangles = cad_triangles.saturating_add(entry.cad.triangles.len());
        }
        if cad_triangles > 300_000 {
            return invalid("total tool CAD display mesh budget exceeded");
        }
        for setup in &self.setups {
            if let Some(drawing) = &setup.drawing {
                drawing.validate()?;
            }
            if let Some(post) = &setup.post {
                post.validate()?;
            }
            if let Some(end) = setup.cut_end_z_mm {
                if !end.is_finite() || !(-1e6..=-0.001).contains(&end) {
                    return invalid("cut end must be a finite negative Z between -1000000 and -0.001 mm");
                }
                for job in setup.od_job.iter().chain(&setup.id_job) {
                    job.clipped(end)?;
                }
            }
            for job in setup.od_job.iter().chain(&setup.id_job) {
                job.validate_draft()?;
            }
            text("setup id", &setup.id, false)?;
            text("setup name", &setup.name, false)?;
            if let Some(previous) = &setup.previous_setup
                && (previous == &setup.id || !setups.contains(previous))
            {
                return invalid("previous_setup must name an earlier setup");
            }
            if !setups.insert(setup.id.clone()) {
                return invalid("duplicate setup id");
            }
            if let Some(stock) = &setup.stock {
                positive("stock OD", stock.outer_diameter_mm)?;
                positive("stock length", stock.length_mm)?;
                if !stock.inner_diameter_mm.is_finite() || stock.inner_diameter_mm < 0.0 || stock.inner_diameter_mm >= stock.outer_diameter_mm {
                    return invalid("stock ID must be finite, nonnegative and smaller than OD");
                }
            }
            if setup.turret.len() != 12 {
                return invalid("expected exactly 12 turret stations");
            }
            let mut stations = BTreeSet::new();
            for slot in &setup.turret {
                if let Some(catalog) = &slot.catalog {
                    catalog.validate()?;
                }
                for (cad, model) in [
                    (&slot.holder_cad, slot.catalog.as_ref().and_then(|c| c.holder.as_deref())),
                    (&slot.insert_cad, slot.catalog.as_ref().and_then(|c| c.insert.as_deref())),
                ] {
                    let Some(cad) = cad else { continue };
                    cad.validate()?;
                    cad_triangles = cad_triangles.saturating_add(cad.triangles.len());
                    if cad_triangles > 300_000 {
                        return invalid("total holder CAD display mesh budget exceeded");
                    }
                    if model != Some(cad.model.as_str()) {
                        return invalid("tool CAD does not match the registered model");
                    }
                }
                if let Some(tool) = &slot.simulation {
                    tool.validate()?;
                }
                if !(1..=12).contains(&slot.station) || !stations.insert(slot.station) {
                    return invalid("turret stations must be unique numbers from 1 to 12");
                }
                if let Some(name) = &slot.tool_name {
                    text("tool name", name, false)?;
                    if slot.role == ToolRole::Vacant {
                        return invalid("a vacant station cannot have a mounted tool");
                    }
                }
                if slot.offset_number == Some(0) {
                    return invalid("offset number must be positive when supplied");
                }
            }
            if setup.operations.len() > 256 {
                return invalid("at most 256 operations per setup");
            }
            for (index, operation) in setup.operations.iter().enumerate() {
                if let Some(spindle) = &operation.spindle {
                    spindle.validate()?;
                }
                if let Some(cutting) = &operation.cutting {
                    cutting.validate_draft(operation.kind)?;
                }
                text("operation id", &operation.id, false)?;
                text("operation name", &operation.name, false)?;
                if !operations.insert(operation.id.clone()) {
                    return invalid("operation ids must be unique across the plan");
                }
                if let Some(station) = operation.station
                    && !setup.turret.iter().any(|slot| slot.station == station && slot.role != ToolRole::Vacant)
                {
                    return invalid("operation references an absent or vacant station");
                }
                let separates = matches!(operation.kind, OperationKind::PartOff | OperationKind::PartingFinish);
                if separates && index + 1 != setup.operations.len() {
                    return invalid("separation must be the last operation of a setup");
                }
                match (&operation.kind, &operation.manual_separation) {
                    (OperationKind::PartingFinish, Some(manual)) => {
                        positive("finished bore diameter", manual.finished_bore_diameter_mm)?;
                        positive("stop diameter", manual.stop_diameter_mm)?;
                        if let Some(stock) = &setup.stock
                            && (manual.finished_bore_diameter_mm >= stock.outer_diameter_mm || manual.stop_diameter_mm >= stock.outer_diameter_mm)
                        {
                            return invalid("finished bore and stop diameter must be smaller than stock OD");
                        }
                    }
                    (OperationKind::PartingFinish, None) => return invalid("parting finish requires finished bore and stop diameters"),
                    (_, Some(_)) => return invalid("manual separation belongs only to parting finish"),
                    (_, None) => {}
                }
            }
        }
        Ok(())
    }
}

fn invalid<T>(message: &str) -> Result<T> {
    Err(PlanError(message.into()))
}

fn text(field: &str, value: &str, empty_allowed: bool) -> Result<()> {
    if (!empty_allowed && value.trim().is_empty()) || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(PlanError(format!("{field} must be text of at most 256 bytes")));
    }
    Ok(())
}

fn positive(field: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value <= 0.0 || value > 1_000_000.0 {
        return Err(PlanError(format!("{field} must be finite and in (0, 1000000] mm")));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
