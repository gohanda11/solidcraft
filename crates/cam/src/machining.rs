//! Bounded nominal-tip planning for a front face and an accessible, monotone OD.
//! This is not insert/holder simulation. In particular, no nose compensation is implied.
use serde::{Deserialize, Serialize};

use crate::{PlanError, Result, Stock, invalid};

pub mod interior;
mod range;
pub mod selection;
pub mod sequence;

const EPS: f64 = 1e-8;
const MAX_MOVES: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePoint {
    pub z_mm: f64,
    pub diameter_mm: f64,
}

/// Front finished face is Z0. Z decreases towards the chuck. X is a diameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OdJob {
    pub profile: Vec<ProfilePoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_source: Option<ModelSource>,
    pub stock_front_z_mm: f64,
    pub chuck_front_z_mm: f64,
    pub radial_stock_to_leave_mm: f64,
    pub axial_stock_to_leave_mm: f64,
    pub radial_depth_mm: f64,
    pub face_depth_mm: f64,
    pub clearance_mm: f64,
    pub rough_feed_mm_rev: f64,
    pub finish_feed_mm_rev: f64,
    pub finish: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSource {
    pub body: String,
    pub reverse: bool,
    pub origin: [f64; 3],
    pub axis: [f64; 3],
    #[serde(default)]
    pub approximation_mm: f64,
}

impl Default for OdJob {
    fn default() -> Self {
        // The owner's finishing allowance preference; cutting conditions remain unconfigured.
        Self {
            profile: Vec::new(),
            model_source: None,
            stock_front_z_mm: 0.0,
            chuck_front_z_mm: 0.0,
            radial_stock_to_leave_mm: 0.1,
            axial_stock_to_leave_mm: 0.1,
            radial_depth_mm: 0.0,
            face_depth_mm: 0.0,
            clearance_mm: 0.0,
            rough_feed_mm_rev: 0.0,
            finish_feed_mm_rev: 0.0,
            finish: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    CenterDrill,
    Drill,
    OdGrooveRough,
    OdGrooveFinish,
    FaceRough,
    OdRough,
    RoughContour,
    FaceFinish,
    OdFinish,
    IdRough,
    IdRoughContour,
    IdFinish,
    Transfer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    Rapid,
    Feed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Move {
    pub from: ProfilePoint,
    pub to: ProfilePoint,
    pub motion: Motion,
    pub phase: Phase,
    pub feed_mm_rev: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllowanceRow {
    pub z_mm: f64,
    pub finished_diameter_mm: f64,
    pub rough_diameter_mm: f64,
    pub radial_remaining_mm: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub job: OdJob,
    pub rough_profile: Vec<ProfilePoint>,
    pub allowance_audit: Vec<AllowanceRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Toolpath {
    #[serde(default)]
    pub source_setup: Option<Box<crate::Setup>>,
    #[serde(default)]
    pub operation_spans: Vec<sequence::OperationSpan>,
    #[serde(default)]
    pub inner: bool,
    #[serde(default)]
    pub id_path: Option<ProfileSummary>,
    pub basis: String,
    pub machine_ready: bool,
    pub stock: Stock,
    pub job: OdJob,
    pub rough_profile: Vec<ProfilePoint>,
    pub moves: Vec<Move>,
    pub allowance_audit: Vec<AllowanceRow>,
    pub rough_removed_volume_mm3: f64,
    pub finish_removed_volume_mm3: f64,
    pub warnings: Vec<String>,
}

fn bounded(name: &str, n: f64, minimum: f64, maximum: f64) -> Result<()> {
    if !n.is_finite() || n < minimum || n > maximum {
        return Err(PlanError(format!("{name}: expected {minimum}..={maximum}")));
    }
    Ok(())
}

impl OdJob {
    /// Draft validation; zero-valued cutting conditions are allowed until calculation.
    pub fn validate_draft(&self) -> Result<()> {
        if let Some(source) = &self.model_source
            && (source.body.is_empty()
                || source.body.len() > 512
                || source.body.chars().any(char::is_control)
                || source.origin.iter().chain(&source.axis).any(|v| !v.is_finite() || v.abs() > 1e6)
                || (source.axis.iter().map(|v| v * v).sum::<f64>() - 1.0).abs() > 1e-6
                || !source.approximation_mm.is_finite()
                || !(0.0..=0.001).contains(&source.approximation_mm))
        {
            return invalid("invalid model profile source");
        }
        if self.profile.len() > 256 {
            return invalid("at most 256 OD profile points");
        }
        for p in &self.profile {
            bounded("profile Z", p.z_mm, -1e6, 1e6)?;
            bounded("profile diameter", p.diameter_mm, 0.0, 1e6)?;
        }
        for (name, value) in [
            ("stock front Z", self.stock_front_z_mm),
            ("radial allowance", self.radial_stock_to_leave_mm),
            ("axial allowance", self.axial_stock_to_leave_mm),
            ("radial depth", self.radial_depth_mm),
            ("face depth", self.face_depth_mm),
            ("clearance", self.clearance_mm),
            ("rough feed", self.rough_feed_mm_rev),
            ("finish feed", self.finish_feed_mm_rev),
        ] {
            bounded(name, value, 0.0, 1e6)?;
        }
        bounded("chuck front Z", self.chuck_front_z_mm, -1e6, 0.0)
    }

    fn validate(&self, stock: &Stock) -> Result<()> {
        self.validate_draft()?;
        bounded("stock OD", stock.outer_diameter_mm, 0.001, 1e6)?;
        bounded("stock ID", stock.inner_diameter_mm, 0.0, stock.outer_diameter_mm - 0.001)?;
        bounded("stock length", stock.length_mm, 0.001, 1e6)?;
        bounded("radial depth", self.radial_depth_mm, 0.001, 100.0)?;
        bounded("face depth", self.face_depth_mm, 0.001, 100.0)?;
        bounded("clearance", self.clearance_mm, 0.01, 1000.0)?;
        bounded("rough feed", self.rough_feed_mm_rev, 0.001, 10.0)?;
        if self.finish {
            bounded("finish feed", self.finish_feed_mm_rev, 0.001, 10.0)?;
        }
        let (Some(first), Some(last)) = (self.profile.first(), self.profile.last()) else {
            return invalid("OD profile is missing");
        };
        if self.profile.len() < 2 || first.z_mm.abs() > EPS || last.z_mm >= -EPS {
            return invalid("OD profile must start at finished face Z0 and end at negative Z");
        }
        for p in &self.profile {
            if p.diameter_mm <= stock.inner_diameter_mm + EPS {
                return invalid("finished OD must exceed the stock bore");
            }
            if p.diameter_mm + 2.0 * self.radial_stock_to_leave_mm > stock.outer_diameter_mm + EPS {
                return invalid("stock is too small for the finished OD plus twice the radial allowance");
            }
        }
        for pair in self.profile.windows(2) {
            let [a, b] = pair else { continue };
            if b.z_mm > a.z_mm + EPS || b.diameter_mm < a.diameter_mm - EPS {
                return invalid("unsupported OD undercut/reversed profile: split grooves and back-facing into separate operations");
            }
            if (a.z_mm - b.z_mm).abs() <= EPS && (a.diameter_mm - b.diameter_mm).abs() <= EPS {
                return invalid("duplicate profile points");
            }
        }
        if self.stock_front_z_mm < self.axial_stock_to_leave_mm {
            return invalid("front stock is smaller than the axial finishing allowance");
        }
        if last.z_mm < self.stock_front_z_mm - stock.length_mm - EPS {
            return invalid("profile extends behind the stock");
        }
        if last.z_mm < self.chuck_front_z_mm + self.clearance_mm - EPS {
            return invalid("profile enters the chuck clearance zone");
        }
        let passes = (stock.outer_diameter_mm / 2.0 / self.radial_depth_mm).ceil() + (self.stock_front_z_mm / self.face_depth_mm).ceil();
        if passes > 2000.0 {
            return invalid("too many passes: increase depth or reduce stock");
        }
        if self.finish && (self.radial_stock_to_leave_mm > self.radial_depth_mm || self.axial_stock_to_leave_mm > self.face_depth_mm) {
            return invalid("finishing allowance exceeds the configured pass depth");
        }
        Ok(())
    }

    pub fn rough_profile(&self) -> Vec<ProfilePoint> {
        let mut points: Vec<_> = self
            .profile
            .iter()
            .map(|p| ProfilePoint { z_mm: p.z_mm + self.axial_stock_to_leave_mm, diameter_mm: p.diameter_mm + 2.0 * self.radial_stock_to_leave_mm })
            .collect();
        // Keep machining the rear extent. Shifting the final Z would leave an unroughed ring
        // for the finishing pass to hit with far more than the requested radial allowance.
        if self.axial_stock_to_leave_mm > 0.0
            && let (Some(last), Some(rough_last)) = (self.profile.last(), points.last().copied())
        {
            points.push(ProfilePoint { z_mm: last.z_mm, ..rough_last });
        }
        points
    }
}

/// Diameter on a piecewise linear profile. At a shoulder, choose the larger side.
pub fn diameter_at(profile: &[ProfilePoint], z: f64) -> Option<f64> {
    let mut result: Option<f64> = None;
    for pair in profile.windows(2) {
        let [a, b] = pair else { continue };
        if z > a.z_mm + EPS || z < b.z_mm - EPS {
            continue;
        }
        let diameter = if (a.z_mm - b.z_mm).abs() < EPS {
            a.diameter_mm.max(b.diameter_mm)
        } else {
            a.diameter_mm + (b.diameter_mm - a.diameter_mm) * (a.z_mm - z) / (a.z_mm - b.z_mm)
        };
        result = Some(result.map_or(diameter, |d| d.max(diameter)));
    }
    result
}

fn stop_z(profile: &[ProfilePoint], diameter: f64) -> Result<f64> {
    for pair in profile.windows(2) {
        let [a, b] = pair else { continue };
        if a.diameter_mm >= diameter - EPS {
            return Ok(a.z_mm);
        }
        if b.diameter_mm > diameter + EPS {
            let t = (diameter - a.diameter_mm) / (b.diameter_mm - a.diameter_mm);
            return Ok(a.z_mm + t * (b.z_mm - a.z_mm));
        }
    }
    profile.last().map(|p| p.z_mm).ok_or_else(|| PlanError("missing profile".into()))
}

struct Builder {
    moves: Vec<Move>,
    current: ProfilePoint,
    safe: ProfilePoint,
    clearance: f64,
    phase: Phase,
    feed: f64,
}

impl Builder {
    fn go(&mut self, to: ProfilePoint, motion: Motion) -> Result<()> {
        if self.moves.len() >= MAX_MOVES {
            return invalid("toolpath move limit exceeded");
        }
        if (to.z_mm - self.current.z_mm).abs() < EPS && (to.diameter_mm - self.current.diameter_mm).abs() < EPS {
            return Ok(());
        }
        if motion == Motion::Rapid && (to.z_mm - self.current.z_mm).abs() > EPS && (to.diameter_mm - self.current.diameter_mm).abs() > EPS {
            return invalid("internal error: diagonal rapid forbidden");
        }
        self.moves.push(Move { from: self.current, to, motion, phase: self.phase, feed_mm_rev: (motion == Motion::Feed).then_some(self.feed) });
        self.current = to;
        Ok(())
    }
    fn return_safe(&mut self) -> Result<()> {
        self.go(ProfilePoint { z_mm: (self.current.z_mm + self.clearance).min(self.safe.z_mm), ..self.current }, Motion::Feed)?;
        self.go(ProfilePoint { diameter_mm: self.safe.diameter_mm, ..self.current }, Motion::Rapid)?;
        self.go(self.safe, Motion::Rapid)
    }
    fn face(&mut self, z: f64, inner_diameter: f64) -> Result<()> {
        self.go(ProfilePoint { z_mm: z, ..self.safe }, Motion::Rapid)?;
        self.go(ProfilePoint { z_mm: z, diameter_mm: inner_diameter }, Motion::Feed)?;
        self.return_safe()
    }
    fn contour(&mut self, points: &[ProfilePoint]) -> Result<()> {
        let Some(first) = points.first() else {
            return invalid("empty contour");
        };
        self.go(ProfilePoint { diameter_mm: first.diameter_mm, ..self.safe }, Motion::Rapid)?;
        for p in points {
            self.go(*p, Motion::Feed)?;
        }
        self.return_safe()
    }
}

/// Exact revolution volume of the piecewise linear outer boundary (cylinders/frusta).
fn material_volume(profile: &[ProfilePoint], inner_diameter: f64) -> f64 {
    profile
        .windows(2)
        .map(|pair| {
            let [a, b] = pair else { return 0.0 };
            let ra = a.diameter_mm / 2.0;
            let rb = b.diameter_mm / 2.0;
            std::f64::consts::PI * (a.z_mm - b.z_mm) * ((ra * ra + ra * rb + rb * rb) / 3.0 - (inner_diameter / 2.0).powi(2))
        })
        .sum()
}

pub fn calculate(job: &OdJob, stock: &Stock) -> Result<Toolpath> {
    job.validate(stock)?;
    let rough = job.rough_profile();
    let first = rough.first().ok_or_else(|| PlanError("empty rough contour".into()))?;
    let last = job.profile.last().ok_or_else(|| PlanError("empty profile".into()))?;
    let safe = ProfilePoint { diameter_mm: stock.outer_diameter_mm + 2.0 * job.clearance_mm, z_mm: job.stock_front_z_mm + job.clearance_mm };
    let mut builder =
        Builder { moves: Vec::new(), current: safe, safe, clearance: job.clearance_mm, phase: Phase::FaceRough, feed: job.rough_feed_mm_rev };
    let face_passes = ((job.stock_front_z_mm - job.axial_stock_to_leave_mm) / job.face_depth_mm).ceil() as usize;
    for n in 1..=face_passes {
        let z = (job.stock_front_z_mm - n as f64 * job.face_depth_mm).max(job.axial_stock_to_leave_mm);
        builder.face(z, stock.inner_diameter_mm)?;
    }
    builder.phase = Phase::OdRough;
    let passes = ((stock.outer_diameter_mm - first.diameter_mm) / (2.0 * job.radial_depth_mm)).ceil() as usize;
    for n in 1..=passes {
        let diameter = (stock.outer_diameter_mm - n as f64 * 2.0 * job.radial_depth_mm).max(first.diameter_mm);
        builder.go(ProfilePoint { diameter_mm: diameter, ..safe }, Motion::Rapid)?;
        builder.go(ProfilePoint { diameter_mm: diameter, z_mm: stop_z(&rough, diameter)? }, Motion::Feed)?;
        builder.return_safe()?;
    }
    builder.phase = Phase::RoughContour;
    builder.contour(&rough)?;
    if job.finish {
        builder.feed = job.finish_feed_mm_rev;
        if job.axial_stock_to_leave_mm > 0.0 {
            builder.phase = Phase::FaceFinish;
            builder.face(0.0, stock.inner_diameter_mm)?;
        }
        builder.phase = Phase::OdFinish;
        builder.contour(&job.profile)?;
    }
    let audit = audit(job, &rough)?;
    let initial_volume = std::f64::consts::PI
        * (job.stock_front_z_mm - last.z_mm)
        * ((stock.outer_diameter_mm / 2.0).powi(2) - (stock.inner_diameter_mm / 2.0).powi(2));
    let rough_volume = material_volume(&rough, stock.inner_diameter_mm);
    let finish_volume = material_volume(&job.profile, stock.inner_diameter_mm);
    verify_nominal_moves(job, stock, &builder.moves)?;
    Ok(Toolpath {
        source_setup: None,
        operation_spans: vec![],
        inner: false,
        id_path: None,
        basis: "nominal_tip_no_nose_or_holder_compensation".into(),
        machine_ready: false,
        stock: stock.clone(),
        job: job.clone(),
        rough_profile: rough,
        moves: builder.moves,
        allowance_audit: audit,
        rough_removed_volume_mm3: initial_volume - rough_volume,
        finish_removed_volume_mm3: if job.finish { rough_volume - finish_volume } else { 0.0 },
        warnings: vec![
            "Nominal-tip paths only; insert nose, holder and chuck geometry are not simulated.".into(),
            "Verify the X-diameter/Z profile against the CAD body and drawing.".into(),
            "Only explicitly configured face, OD and through-ID profiles are output.".into(),
        ],
    })
}

fn audit(job: &OdJob, rough: &[ProfilePoint]) -> Result<Vec<AllowanceRow>> {
    let last = job.profile.last().ok_or_else(|| PlanError("empty profile".into()))?;
    let mut stations = vec![0.0, last.z_mm];
    for p in job.profile.iter().chain(rough) {
        if p.z_mm <= 0.0 && p.z_mm >= last.z_mm {
            stations.push(p.z_mm);
        }
    }
    stations.sort_by(|a, b| b.total_cmp(a));
    stations.dedup_by(|a, b| (*a - *b).abs() < EPS);
    // Include both one-sided shoulder limits and interval midpoints. A uniform sampling grid
    // could miss a narrow axial allowance band entirely.
    let mut sample = stations.clone();
    for w in stations.windows(2) {
        let [a, b] = w else { continue };
        sample.extend([a - (a - b) * 1e-5, (a + b) / 2.0, b + (a - b) * 1e-5]);
    }
    sample.sort_by(|a, b| b.total_cmp(a));
    sample.dedup_by(|a, b| (*a - *b).abs() < EPS);
    let audit: Vec<_> = sample
        .into_iter()
        .filter_map(|z| {
            let target = diameter_at(&job.profile, z)?;
            let remaining = diameter_at(rough, z)?;
            Some(AllowanceRow {
                z_mm: z,
                finished_diameter_mm: target,
                rough_diameter_mm: remaining,
                radial_remaining_mm: (remaining - target) / 2.0,
            })
        })
        .collect();
    if audit.iter().any(|r| r.radial_remaining_mm < -EPS) {
        return invalid("internal error: rough contour gouges the finished profile");
    }
    Ok(audit)
}

/// Independently check emitted feed segments against every linear finished-profile span.
/// Since both are linear, checking each overlapping interval's ends proves that interval.
fn verify_nominal_moves(job: &OdJob, stock: &Stock, moves: &[Move]) -> Result<()> {
    let safe_z = job.stock_front_z_mm + job.clearance_mm;
    let safe_x = stock.outer_diameter_mm + 2.0 * job.clearance_mm;
    for (index, m) in moves.iter().enumerate() {
        for point in [m.from, m.to] {
            if !point.z_mm.is_finite()
                || !point.diameter_mm.is_finite()
                || point.diameter_mm < -EPS
                || point.z_mm < job.chuck_front_z_mm + job.clearance_mm - EPS
            {
                return invalid("generated move is invalid or enters chuck clearance");
            }
        }
        if m.motion == Motion::Rapid {
            let axial = (m.from.z_mm - m.to.z_mm).abs() > EPS;
            let radial = (m.from.diameter_mm - m.to.diameter_mm).abs() > EPS;
            if axial && radial {
                return invalid("diagonal rapid rejected");
            }
            if axial && m.from.diameter_mm < safe_x - EPS {
                return invalid("axial rapid is not outside stock");
            }
            if radial && m.from.z_mm < safe_z - EPS {
                let previous = index.checked_sub(1).and_then(|i| moves.get(i));
                if m.to.diameter_mm < m.from.diameter_mm
                    || !previous
                        .is_some_and(|p| p.motion == Motion::Feed && p.to.z_mm > p.from.z_mm && (p.from.diameter_mm - p.to.diameter_mm).abs() < EPS)
                {
                    return invalid("radial rapid lacks a cleared forward withdrawal");
                }
            }
        }
        // Radial moves may touch the lower side of a shoulder, never go beneath it.
        if (m.to.z_mm - m.from.z_mm).abs() <= EPS && m.to.z_mm < -EPS {
            let mut exposed: Option<f64> = None;
            for pair in job.profile.windows(2) {
                let [a, b] = pair else { continue };
                let z = m.to.z_mm;
                if z > a.z_mm + EPS || z < b.z_mm - EPS {
                    continue;
                }
                let diameter = if a.z_mm - b.z_mm <= EPS {
                    a.diameter_mm.min(b.diameter_mm)
                } else {
                    a.diameter_mm + (b.diameter_mm - a.diameter_mm) * (a.z_mm - z) / (a.z_mm - b.z_mm)
                };
                exposed = Some(exposed.map_or(diameter, |v| v.min(diameter)));
            }
            if exposed.is_some_and(|d| m.from.diameter_mm.min(m.to.diameter_mm) < d - EPS) {
                return invalid("generated radial move gouges the finished OD");
            }
        }
        // Nonzero axial intervals must stay above the OD line.
        if (m.to.z_mm - m.from.z_mm).abs() > EPS {
            for pair in job.profile.windows(2) {
                let [a, b] = pair else { continue };
                if a.z_mm - b.z_mm <= EPS {
                    continue;
                }
                let lo = b.z_mm.max(m.from.z_mm.min(m.to.z_mm));
                let hi = a.z_mm.min(m.from.z_mm.max(m.to.z_mm));
                if hi - lo <= EPS {
                    continue;
                }
                for z in [lo, hi] {
                    let tool = m.from.diameter_mm + (m.to.diameter_mm - m.from.diameter_mm) * (z - m.from.z_mm) / (m.to.z_mm - m.from.z_mm);
                    let target = a.diameter_mm + (b.diameter_mm - a.diameter_mm) * (a.z_mm - z) / (a.z_mm - b.z_mm);
                    if tool < target - EPS {
                        return invalid("generated nominal-tip segment gouges the finished OD");
                    }
                }
            }
        }
    }
    Ok(())
}

fn outward(value: f64) -> f64 {
    let scaled = value * 10_000.0;
    // Remove sub-1e-11 mm floating point noise at a representable NC coordinate.
    let nearest = scaled.round();
    if (scaled - nearest).abs() < 1e-7 { nearest / 10_000.0 } else { scaled.ceil() / 10_000.0 }
}

/// Review motion fragment, deliberately disarmed with M30 before any moves.
/// No machine-dependent tool change, work offset, spindle or nose compensation is invented.
pub fn review_nc(path: &Toolpath) -> Result<String> {
    review_program(path).map(|program| program.text)
}

pub fn review_program(path: &Toolpath) -> Result<crate::post::ReviewProgram> {
    if let Some(setup) = &path.source_setup {
        return sequence::review_program(setup);
    }
    let mut validated = if path.inner {
        interior::calculate(&path.job, &path.stock)?
    } else {
        interior::calculate_setup(Some(&path.job), path.id_path.as_ref().map(|p| &p.job), &path.stock)?
    };
    let mut previous_point = None;
    for m in &mut validated.moves {
        m.from = interior::rounded(m.from, interior::is_inner(m.phase));
        m.to = interior::rounded(m.to, interior::is_inner(m.phase));
        if let Some(previous) = previous_point {
            m.from = previous;
        }
        previous_point = Some(m.to);
    }
    interior::verify_combined(&validated)?;
    let mut text = crate::post::ReviewWriter::new(
        "%\nO9001 (SOLIDCRAFT REVIEW ONLY)\nM30\n(NOT A MACHINE READY PROGRAM - MOTIONS BELOW ARE DISARMED)\n(NOMINAL TIP - NO NOSE OR HOLDER COMPENSATION)\n(PROFILE - VERIFY AGAINST CAD AND DRAWING)\n(X DIAMETER MM - Z ABSOLUTE MM - F MM PER REV)\n(TOOL CALLS WORK OFFSET SPINDLE AND FEED MODE MUST BE PROVIDED)\n(FACE OD AND THROUGH ID AS PLANNED - NO DRILLING GROOVES OR THREADS)\n",
        validated.moves.len(),
    );
    text.push_str("(X ROUNDED TOWARD AIR: OD UP / ID DOWN; Z UP TO 0.0001 MM)\n");
    text.push_str(&format!(
        "(ROUGH ALLOWANCE RADIAL {:.4} - DIAMETER {:.4} - AXIAL {:.4})\n",
        path.job.radial_stock_to_leave_mm,
        2.0 * path.job.radial_stock_to_leave_mm,
        path.job.axial_stock_to_leave_mm
    ));
    if let Some(job) = if path.inner { Some(&path.job) } else { path.id_path.as_ref().map(|p| &p.job) } {
        text.push_str(&format!(
            "(ID PREBORE {:.4} - RADIAL LEAVE {:.4} - AXIAL LEAVE {:.4})\n(THROUGH BORE REQUIRED - BORING BAR DIAMETER AND REACH NOT SIMULATED)\n",
            path.stock.inner_diameter_mm, job.radial_stock_to_leave_mm, job.axial_stock_to_leave_mm
        ));
    }
    if let Some(first) = validated.moves.first() {
        text.push_str(&format!("(REQUIRED START POSITION X{:.4} Z{:.4})\n", first.from.diameter_mm, first.from.z_mm));
    }
    let mut previous = None;
    for (index, m) in validated.moves.iter().enumerate() {
        if previous != Some(m.phase) {
            text.push_str(&format!("({:?})\n", m.phase));
            if interior::is_inner(m.phase) {
                text.push_str(if m.phase == Phase::IdFinish {
                    "(ID FINISH - DEFAULT STATION 7 - VERIFY TOOL CALL)\n"
                } else {
                    "(ID ROUGH - DEFAULT STATION 5 - VERIFY TOOL CALL)\n"
                });
            }
            previous = Some(m.phase);
        }
        let code = if m.motion == Motion::Rapid { "G00" } else { "G01" };
        text.mark_move(index)?;
        // Each rapid changes a single axis even after quantization. Feed moves state both.
        if m.motion == Motion::Rapid {
            if (m.from.diameter_mm - m.to.diameter_mm).abs() > EPS {
                text.push_str(&format!("{code} X{:.4}\n", m.to.diameter_mm));
            } else {
                text.push_str(&format!("{code} Z{:.4}\n", m.to.z_mm));
            }
        } else {
            text.push_str(&format!("{code} X{:.4} Z{:.4} F{:.4}\n", m.to.diameter_mm, m.to.z_mm, m.feed_mm_rev.unwrap_or(0.0)));
        }
    }
    text.push_str("M30\n%\n");
    Ok(text.finish())
}

#[cfg(test)]
mod tests;
