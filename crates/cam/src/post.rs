//! A disarmed NL conversational-reference dialect, based on owner-supplied NC examples.
//! Geometric paths are regenerated; example part geometry and opaque M-code meanings are not imported.
use serde::{Deserialize, Serialize};

use crate::machining::{Motion, sequence};
use crate::{OperationKind as K, Result, Setup, invalid};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XSide {
    Positive,
    #[default]
    Negative,
}
impl XSide {
    pub fn coordinate(self, diameter_mm: f64) -> f64 {
        if diameter_mm == 0. {
            0.
        } else if self == Self::Negative {
            -diameter_mm
        } else {
            diameter_mm
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedMode {
    SurfaceSpeed,
    Rpm,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spindle {
    pub mode: SpeedMode,
    /// m/min for G96, rpm for G97. Feed remains in the actual cutting operation.
    pub speed: f64,
    pub max_rpm: u32,
}
impl Spindle {
    pub fn validate(&self) -> Result<()> {
        let minimum = if self.mode == SpeedMode::Rpm { 1. } else { 0.001 };
        if !(1..=60_000).contains(&self.max_rpm) || !self.speed.is_finite() || self.speed < minimum {
            return invalid("NC spindle speed and RPM limit must be positive and finite");
        }
        let cap = if self.mode == SpeedMode::SurfaceSpeed { 10_000. } else { 60_000. };
        if self.speed > cap {
            return invalid("NC spindle speed exceeds the supported bound");
        }
        Ok(())
    }
}

/// Defaults are editable example values, not material/tool recommendations or machine certification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NlPost {
    pub program_number: u16,
    pub x_side: XSide,
    pub work_offset: u8,
    pub max_rpm: u32,
    pub rough_surface_m_min: f64,
    pub finish_surface_m_min: f64,
    pub drill_rpm: u32,
    pub groove_rpm: u32,
    pub coolant: bool,
    pub optional_stop: bool,
    /// Only a review fallback. A slot's actual offset_number takes precedence.
    pub example_same_number_offsets: bool,
    /// The example's simultaneous G53 X0 Z0 block is never enabled by inference.
    pub machine_zero_return: bool,
    /// Exact opaque pair from the examples; no claim about their meaning or another machine.
    pub example_m86_m85: bool,
}
impl Default for NlPost {
    fn default() -> Self {
        Self {
            program_number: 9001,
            x_side: XSide::Negative,
            work_offset: 54,
            max_rpm: 2000,
            rough_surface_m_min: 150.,
            finish_surface_m_min: 200.,
            drill_rpm: 1000,
            groove_rpm: 700,
            coolant: true,
            optional_stop: true,
            example_same_number_offsets: true,
            machine_zero_return: false,
            example_m86_m85: false,
        }
    }
}
impl NlPost {
    pub fn validate(&self) -> Result<()> {
        if !(1..=9999).contains(&self.program_number) || !(54..=59).contains(&self.work_offset) {
            return invalid("NC program number must be 1..9999 and work offset G54..G59");
        }
        for kind in [K::OdRough, K::OdFinish, K::Drill, K::OdGrooveRough] {
            self.spindle(kind).validate()?;
        }
        Ok(())
    }
    pub fn spindle(&self, kind: K) -> Spindle {
        let (mode, speed) = match kind {
            K::CenterDrill | K::Drill => (SpeedMode::Rpm, f64::from(self.drill_rpm)),
            K::OdGroove | K::OdGrooveRough | K::OdGrooveFinish => (SpeedMode::Rpm, f64::from(self.groove_rpm)),
            K::FaceFinish | K::OdFinish | K::IdFinish => (SpeedMode::SurfaceSpeed, self.finish_surface_m_min),
            _ => (SpeedMode::SurfaceSpeed, self.rough_surface_m_min),
        };
        Spindle { mode, speed, max_rpm: self.max_rpm }
    }
}

fn operation_label(kind: K) -> &'static str {
    match kind {
        K::Face | K::FaceRough => "TANMEN,ARA",
        K::FaceFinish => "TANMEN,SHIAGE",
        K::OdRough => "GAIKEI,ARA",
        K::OdFinish => "GAIKEI,SHIAGE",
        K::IdRough => "NAIKEI,ARA",
        K::IdFinish => "NAIKEI,SHIAGE",
        K::CenterDrill => "CENTER DRILL",
        K::Drill => "DRILL",
        K::OdGroove | K::OdGrooveRough => "MIZO,ARA",
        K::OdGrooveFinish => "MIZO,SHIAGE",
        _ => "UNSUPPORTED",
    }
}

/// Always begins with M30 before any state change, including tool calls and auxiliary blocks.
/// Export never trusts a cached path and never copies coordinates from the reference NC.
pub fn review_nc(setup: &Setup, post: &NlPost) -> Result<String> {
    post.validate()?;
    let path = sequence::calculate(setup)?;
    let mut text = format!(
        "%\nO{:04} (SOLIDCRAFT NL SAMPLE REVIEW)\nM30\n(DISARMED - NOT MACHINE READY)\n(NOMINAL TIP - NO NOSE COMPENSATION)\n(X SIGN {} / DIAMETER MM / ABSOLUTE X Z / F MM PER REV)\n(TOOL CHANGE APPROACH AND MACHINE RETURN ARE NOT VERIFIED)\n",
        post.program_number,
        if post.x_side == XSide::Negative { "NEGATIVE" } else { "POSITIVE" }
    );
    if post.example_m86_m85 {
        text.push_str("M86\n");
    } else {
        text.push_str("(EXAMPLE M86 OMITTED - MACHINE FUNCTION UNCONFIRMED)\n");
    }
    let mut covered = vec![false; path.moves.len()];
    for (number, operation) in setup.operations.iter().enumerate() {
        // Older whole-profile jobs predate operation spans. Their phase-to-kind mapping is
        // unambiguous; selected contours/drilling/grooves use the planner's explicit spans.
        let indices: Vec<usize> = if path.operation_spans.is_empty() {
            path.moves.iter().enumerate().filter_map(|(i, m)| (crate::simulation::operation(m.phase) == Some(operation.kind)).then_some(i)).collect()
        } else {
            let Some(span) = path.operation_spans.iter().find(|span| span.operation_id == operation.id) else {
                return invalid("NC operation has no matching motion span");
            };
            if span.start > span.end || span.end > path.moves.len() {
                return invalid("NC operation range is invalid");
            }
            (span.start..span.end).collect()
        };
        let mut moves = Vec::with_capacity(indices.len());
        for index in indices {
            let Some(seen) = covered.get_mut(index) else { return invalid("NC move index is invalid") };
            if *seen {
                return invalid("NC motion belongs to more than one operation");
            }
            *seen = true;
            let Some(movement) = path.moves.get(index) else { return invalid("NC move index is invalid") };
            moves.push(movement);
        }
        if moves.is_empty() {
            text.push_str(&format!("\n(N{} {} - NO MATERIAL TO REMOVE)\n", number + 1, operation_label(operation.kind)));
            continue;
        }
        let Some(slot) = operation.station.and_then(|n| setup.turret.iter().find(|s| s.station == n)) else {
            return invalid("NL output requires an assigned tool for every operation");
        };
        if !(1..=99).contains(&slot.station) {
            return invalid("NL T-code station must be 1..99");
        }
        let offset = match slot.offset_number {
            Some(n) if (1..=99).contains(&n) => n,
            Some(_) => return invalid("NL T-code offset must be 1..99"),
            None if post.example_same_number_offsets => u16::from(slot.station),
            None => return invalid("set each tool offset or explicitly select the example's same-number review mapping"),
        };
        let spindle = operation.spindle.clone().unwrap_or_else(|| post.spindle(operation.kind));
        spindle.validate()?;
        text.push_str(&format!("\nN{}\n({}, T{:02}{:02})\n", number + 1, operation_label(operation.kind), slot.station, offset));
        if slot.offset_number.is_none() {
            text.push_str("(OFFSET USES EXAMPLE SAME-NUMBER MAPPING - VERIFY)\n");
        }
        text.push_str(&format!("G99G18\nG50S{}\nG{}\nG0T{:02}{:02}\n", spindle.max_rpm, post.work_offset, slot.station, offset));
        if post.coolant {
            text.push_str("M8\n");
        }
        match spindle.mode {
            SpeedMode::SurfaceSpeed => text.push_str(&format!("G96S{:.3}M3\n", spindle.speed)),
            SpeedMode::Rpm => text.push_str(&format!("G97S{:.0}M3\n", spindle.speed.min(f64::from(spindle.max_rpm)))),
        }
        let Some(first) = moves.first() else { return invalid("NC operation is empty") };
        text.push_str(&format!(
            "(REQUIRED SAFE APPROACH - VERIFY)\nG0X{:.9}Z{:.9}\n",
            post.x_side.coordinate(first.from.diameter_mm),
            first.from.z_mm
        ));
        for movement in moves {
            let x = post.x_side.coordinate(movement.to.diameter_mm);
            let z = movement.to.z_mm;
            match movement.motion {
                Motion::Rapid => text.push_str(&format!("G0X{x:.9}Z{z:.9}\n")),
                Motion::Feed => {
                    let Some(feed) = movement.feed_mm_rev.filter(|f| f.is_finite() && *f > 0.) else {
                        return invalid("NC cutting move is missing its feed");
                    };
                    text.push_str(&format!("G1X{x:.9}Z{z:.9}F{feed:.9}\n"));
                }
            }
        }
        if post.machine_zero_return {
            text.push_str("G53X0Z0\n");
        } else {
            text.push_str("(EXAMPLE G53 X0 Z0 OMITTED - RETURN NOT VERIFIED)\n");
        }
        if post.coolant {
            text.push_str("M9\n");
        }
        if post.optional_stop {
            text.push_str("M01\n");
        }
    }
    if path.moves.iter().zip(&covered).any(|(m, seen)| !seen && m.phase != crate::machining::Phase::Transfer) {
        return invalid("NC output did not cover every planned cutting/retract motion");
    }
    text.push_str("\nM9\nM5\n");
    if post.example_m86_m85 {
        text.push_str("M85\n");
    } else {
        text.push_str("(EXAMPLE M85 OMITTED - MACHINE FUNCTION UNCONFIRMED)\n");
    }
    text.push_str("M30\n%\n");
    Ok(text)
}

#[cfg(test)]
mod tests;
