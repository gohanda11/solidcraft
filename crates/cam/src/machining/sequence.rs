//! Ordered, bounded review paths for axial drilling and rectangular outside grooves.
//! Coordinates are nominal: drill tip on X0; groove's front cutting edge in Z.
use super::*;
use crate::{OperationKind as K, Setup};

mod target;
pub(crate) use target::model_linked;
pub(super) use target::{check_drill_target, turning_job, turning_points};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub stock_front_z_mm: f64,
    pub chuck_front_z_mm: f64,
    pub clearance_mm: f64,
}
impl Default for Limits {
    fn default() -> Self {
        Self { stock_front_z_mm: 0., chuck_front_z_mm: 0., clearance_mm: 2. }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drill {
    pub limits: Limits,
    pub diameter_mm: f64,
    /// Absolute Z of the point, NOT the full-diameter bottom of the hole.
    pub tip_end_z_mm: f64,
    pub point_angle_rad: f64,
    /// Zero selects a single feed; otherwise fully retract to the front between pecks.
    pub peck_mm: f64,
    pub feed_mm_rev: f64,
}
impl Default for Drill {
    fn default() -> Self {
        Self { limits: Limits::default(), diameter_mm: 0., tip_end_z_mm: 0., point_angle_rad: 118f64.to_radians(), peck_mm: 0., feed_mm_rev: 0. }
    }
}
impl Drill {
    pub(crate) fn diameter_at(&self, z: f64) -> f64 {
        ((z - self.tip_end_z_mm) * (self.point_angle_rad * 0.5).tan() * 2.).clamp(0., self.diameter_mm)
    }
    pub fn point_length(&self) -> f64 {
        self.diameter_mm * 0.5 / (self.point_angle_rad * 0.5).tan()
    }
    pub fn full_diameter_z(&self) -> f64 {
        self.tip_end_z_mm + self.point_length()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Groove {
    pub limits: Limits,
    pub front_z_mm: f64,
    pub width_mm: f64,
    pub bottom_diameter_mm: f64,
    /// Tool extends from the programmed front edge towards negative Z.
    pub tool_width_mm: f64,
    pub radial_depth_mm: f64,
    pub radial_leave_mm: f64,
    pub axial_leave_mm: f64,
    pub feed_mm_rev: f64,
}
impl Default for Groove {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            front_z_mm: 0.,
            width_mm: 0.,
            bottom_diameter_mm: 0.,
            tool_width_mm: 0.,
            radial_depth_mm: 0.,
            radial_leave_mm: 0.1,
            axial_leave_mm: 0.1,
            feed_mm_rev: 0.,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Cutting {
    Contour(super::selection::Contour),
    Drill(Drill),
    OdGroove(Groove),
}
impl Cutting {
    pub fn limits(&self) -> &Limits {
        match self {
            Self::Contour(c) => &c.limits,
            Self::Drill(d) => &d.limits,
            Self::OdGroove(g) => &g.limits,
        }
    }
    pub fn validate_draft(&self, kind: K) -> Result<()> {
        if let Self::Contour(c) = self {
            return c.validate(kind);
        }
        let l = self.limits();
        bounded("stock front Z", l.stock_front_z_mm, 0., 1e6)?;
        bounded("chuck front Z", l.chuck_front_z_mm, -1e6, 0.)?;
        bounded("clearance", l.clearance_mm, 0., 1000.)?;
        match self {
            Self::Contour(c) => c.validate(kind),
            Self::Drill(d) => {
                if !matches!(kind, K::CenterDrill | K::Drill) {
                    return invalid("drilling parameters require a drilling operation");
                }
                bounded("drill diameter", d.diameter_mm, 0., 1000.)?;
                bounded("drill point Z", d.tip_end_z_mm, -1e6, 0.)?;
                bounded("point angle", d.point_angle_rad, 30f64.to_radians(), 170f64.to_radians())?;
                bounded("peck depth", d.peck_mm, 0., 1000.)?;
                bounded("drill feed", d.feed_mm_rev, 0., 10.)
            }
            Self::OdGroove(g) => {
                if !matches!(kind, K::OdGrooveRough | K::OdGrooveFinish) {
                    return invalid("groove parameters require an OD groove rough/finish operation");
                }
                bounded("groove front Z", g.front_z_mm, -1e6, 0.)?;
                for (name, value) in [
                    ("width", g.width_mm),
                    ("bottom diameter", g.bottom_diameter_mm),
                    ("tool width", g.tool_width_mm),
                    ("radial depth", g.radial_depth_mm),
                    ("radial leave", g.radial_leave_mm),
                    ("axial leave", g.axial_leave_mm),
                ] {
                    bounded(name, value, 0., 1e6)?;
                }
                bounded("groove feed", g.feed_mm_rev, 0., 10.)
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationSpan {
    pub operation_id: String,
    pub start: usize,
    pub end: usize,
}

pub fn operation_at<'a>(path: &Toolpath, setup: &'a Setup, index: usize) -> Option<&'a crate::Operation> {
    let span = path.operation_spans.iter().find(|s| s.start <= index && index < s.end)?;
    setup.operations.iter().find(|o| o.id == span.operation_id)
}

pub fn local(kind: K) -> bool {
    matches!(kind, K::CenterDrill | K::Drill | K::OdGrooveRough | K::OdGrooveFinish)
}

/// Validate an unfinished local operation before leaving its settings page.
/// Ordering, prior stock and finish-after-rough requirements are checked by the full planner.
pub fn validate_local(setup: &Setup, operation: &crate::Operation) -> Result<()> {
    if !local(operation.kind) {
        return Ok(());
    }
    for job in setup.od_job.iter().chain(&setup.id_job) {
        job.validate_draft()?;
    }
    if let Some(drawing) = &setup.drawing {
        drawing.validate()?;
    }
    let cutting = operation.cutting.as_ref().ok_or_else(|| PlanError("加工寸法・送りを設定してください".into()))?;
    cutting.validate_draft(operation.kind)?;
    let stock = setup.stock.as_ref().ok_or_else(|| PlanError("材料寸法を設定してください".into()))?;
    let l = limits(setup)?;
    match cutting {
        Cutting::Drill(d) => {
            check_drill_target(setup, d)?;
            drill_parameters(d, stock, &l)?;
        }
        Cutting::OdGroove(g) => {
            groove_parameters(g, operation.kind == K::OdGrooveFinish, setup, stock, &l)?;
            if operation.kind == K::OdGrooveFinish
                && !setup
                    .operations
                    .iter()
                    .take_while(|o| o.id != operation.id)
                    .any(|o| o.kind == K::OdGrooveRough && matches!(&o.cutting, Some(Cutting::OdGroove(r)) if same_groove(r, g)))
            {
                return invalid("同じ寸法・仕上げ代の溝荒取りを先に配置してください");
            }
        }
        Cutting::Contour(_) => return invalid("穴・溝の加工寸法を設定してください"),
    }
    Ok(())
}

/// Axial midpoint approximation, using the actual nominal drill cone or groove width.
/// No triangular turning insert is substituted for these different cutter types.
pub fn remove_local(slabs: &mut [crate::simulation::Slab], a: ProfilePoint, b: ProfilePoint, cutting: &Cutting) {
    for s in slabs {
        let z = (s.z0 + s.z1) * 0.5;
        match cutting {
            Cutting::Contour(_) => {}
            Cutting::Drill(d) => {
                let tip = a.z_mm.min(b.z_mm);
                if z >= tip {
                    let radius = ((z - tip) * (d.point_angle_rad * 0.5).tan()).min(d.diameter_mm * 0.5);
                    s.inner = s.inner.max(radius.min(s.outer));
                }
            }
            Cutting::OdGroove(g) => {
                if z >= a.z_mm.min(b.z_mm) - g.tool_width_mm && z <= a.z_mm.max(b.z_mm) {
                    s.outer = s.outer.min((a.diameter_mm.min(b.diameter_mm) * 0.5).max(s.inner));
                }
            }
        }
    }
}

/// Resolve and validate only shared placement, independently of unfinished cutting dimensions
/// or feeds. OD/ID jobs take precedence over local operations' saved placement snapshots.
pub fn shared_limits(setup: &Setup) -> Result<Limits> {
    limits(setup)
}

pub(super) fn limits(setup: &Setup) -> Result<Limits> {
    if let Some((od, id)) = setup.od_job.as_ref().zip(setup.id_job.as_ref())
        && [(od.stock_front_z_mm, id.stock_front_z_mm), (od.chuck_front_z_mm, id.chuck_front_z_mm), (od.clearance_mm, id.clearance_mm)]
            .iter()
            .any(|(a, b)| !a.is_finite() || !b.is_finite() || (a - b).abs() > EPS)
    {
        return invalid("外径・内径の材料前端・チャック前端・退避を同じ共通配置に設定してください");
    }
    let l = if let Some(j) = setup.od_job.as_ref().or(setup.id_job.as_ref()) {
        Limits { stock_front_z_mm: j.stock_front_z_mm, chuck_front_z_mm: j.chuck_front_z_mm, clearance_mm: j.clearance_mm }
    } else {
        setup
            .operations
            .iter()
            .find_map(|o| o.cutting.as_ref())
            .ok_or_else(|| PlanError("configure profile or drilling/groove dimensions".into()))?
            .limits()
            .clone()
    };
    bounded("stock front Z", l.stock_front_z_mm, 0., 1e6)?;
    bounded("clearance", l.clearance_mm, 0.01, 1000.)?;
    bounded("chuck front", l.chuck_front_z_mm, -1e6, -0.001)?;
    for c in setup.operations.iter().filter_map(|o| o.cutting.as_ref()).filter(|_| setup.od_job.is_none() && setup.id_job.is_none()) {
        let b = c.limits();
        if [(l.stock_front_z_mm, b.stock_front_z_mm), (l.chuck_front_z_mm, b.chuck_front_z_mm), (l.clearance_mm, b.clearance_mm)]
            .iter()
            .any(|(a, b)| !a.is_finite() || !b.is_finite() || (a - b).abs() > EPS)
        {
            return invalid("all operations must share stock front Z, chuck front Z and clearance");
        }
    }
    Ok(l)
}

fn drill_parameters(d: &Drill, stock: &Stock, l: &Limits) -> Result<usize> {
    bounded("drill diameter", d.diameter_mm, 0.01, stock.outer_diameter_mm - 0.001)?;
    bounded("drill feed", d.feed_mm_rev, 0.001, 10.)?;
    if d.tip_end_z_mm >= l.stock_front_z_mm - EPS || d.tip_end_z_mm < l.chuck_front_z_mm + l.clearance_mm {
        return invalid("drill point must enter stock and stay in front of chuck clearance");
    }
    let distance = l.stock_front_z_mm - d.tip_end_z_mm;
    let count = if d.peck_mm > 0. { (distance / d.peck_mm).ceil() } else { 1. };
    bounded("drill pecks", count, 1., 2000.)?;
    Ok(count as usize)
}

pub(super) fn drill_moves(d: &Drill, kind: K, stock: &Stock, l: &Limits) -> Result<Vec<Move>> {
    let count = drill_parameters(d, stock, l)?;
    let safe = ProfilePoint { diameter_mm: 0., z_mm: l.stock_front_z_mm + l.clearance_mm };
    let mut b = Builder {
        moves: vec![],
        current: safe,
        safe,
        clearance: l.clearance_mm,
        phase: if kind == K::CenterDrill { Phase::CenterDrill } else { Phase::Drill },
        feed: d.feed_mm_rev,
    };
    for n in 1..=count {
        let z = if d.peck_mm > 0. { (l.stock_front_z_mm - n as f64 * d.peck_mm).max(d.tip_end_z_mm) } else { d.tip_end_z_mm };
        b.go(ProfilePoint { z_mm: z, ..safe }, Motion::Feed)?;
        b.go(safe, Motion::Rapid)?;
    }
    Ok(b.moves)
}

pub(super) fn same_groove(a: &Groove, b: &Groove) -> bool {
    [
        (a.front_z_mm, b.front_z_mm),
        (a.width_mm, b.width_mm),
        (a.bottom_diameter_mm, b.bottom_diameter_mm),
        (a.radial_leave_mm, b.radial_leave_mm),
        (a.axial_leave_mm, b.axial_leave_mm),
    ]
    .iter()
    .all(|(a, b)| (a - b).abs() < EPS)
}

fn groove_parameters(g: &Groove, finish: bool, setup: &Setup, stock: &Stock, l: &Limits) -> Result<()> {
    bounded("groove width", g.width_mm, 0.001, stock.length_mm)?;
    // Roughing keeps both wall allowances; finishing may use a tool spanning the
    // final width because its nominal front edge runs exactly between the walls.
    let cutting_width = g.width_mm - if finish { 0. } else { 2. * g.axial_leave_mm };
    bounded("groove tool width", g.tool_width_mm, 0.001, cutting_width)?;
    bounded("groove step", g.radial_depth_mm, 0.001, 100.)?;
    bounded("groove feed", g.feed_mm_rev, 0.001, 10.)?;
    let back = g.front_z_mm - g.width_mm;
    if g.front_z_mm >= -EPS || back < (l.chuck_front_z_mm + l.clearance_mm).max(l.stock_front_z_mm - stock.length_mm) {
        return invalid("groove must lie within stock and in front of chuck clearance");
    }
    let mut outer = stock.outer_diameter_mm;
    let mut inner = stock.inner_diameter_mm;
    // A later drill cannot restore the wall removed by an earlier groove. Check final
    // local intent as well as the bore present at this point in the operation replay.
    for cutting in setup.operations.iter().filter_map(|o| o.cutting.as_ref()) {
        if let Cutting::Drill(d) = cutting {
            inner = inner.max(d.diameter_at(g.front_z_mm));
        }
    }
    let model_groove = target::groove_land(setup, g)?;
    for (job, is_inner) in [(setup.od_job.as_ref(), false), (setup.id_job.as_ref(), true)] {
        if let Some(job) = job {
            let zs = [g.front_z_mm, back].into_iter().chain(job.profile.iter().filter(|p| p.z_mm >= back && p.z_mm <= g.front_z_mm).map(|p| p.z_mm));
            for z in zs {
                let d = diameter_at(&job.profile, z).ok_or_else(|| PlanError("groove must be inside the configured profile extent".into()))?;
                if is_inner {
                    inner = inner.max(d);
                } else if model_groove.is_none() {
                    outer = outer.min(d);
                }
            }
        }
    }
    if let Some(land) = model_groove {
        outer = outer.min(land);
    }
    bounded("groove bottom diameter", g.bottom_diameter_mm, inner + 0.001, outer - 2. * g.radial_leave_mm - 0.001)?;
    if g.radial_leave_mm > g.radial_depth_mm || g.axial_leave_mm > g.tool_width_mm * 0.5 {
        return invalid("groove finishing allowance exceeds the allowed pass engagement");
    }
    if setup.cut_end_z_mm.is_some_and(|end| back < end - EPS) {
        return invalid("溝が共通の加工終端より奥にあります");
    }
    let columns = ((g.width_mm - 2. * g.axial_leave_mm - g.tool_width_mm) / (g.tool_width_mm * 0.8)).ceil().max(1.);
    let layers = ((stock.outer_diameter_mm - g.bottom_diameter_mm - 2. * g.radial_leave_mm) / (2. * g.radial_depth_mm)).ceil().max(1.);
    bounded("groove passes", (columns + 1.) * layers, 1., 4000.)?;
    Ok(())
}

pub(super) fn groove_moves(g: &Groove, finish: bool, setup: &Setup, stock: &Stock, l: &Limits) -> Result<Vec<Move>> {
    groove_parameters(g, finish, setup, stock, l)?;
    let back = g.front_z_mm - g.width_mm;
    let safe = ProfilePoint { z_mm: l.stock_front_z_mm + l.clearance_mm, diameter_mm: stock.outer_diameter_mm + 2. * l.clearance_mm };
    let mut b = Builder {
        moves: vec![],
        current: safe,
        safe,
        clearance: l.clearance_mm,
        phase: if finish { Phase::OdGrooveFinish } else { Phase::OdGrooveRough },
        feed: g.feed_mm_rev,
    };
    let front = g.front_z_mm - if finish { 0. } else { g.axial_leave_mm };
    let end = back + g.tool_width_mm + if finish { 0. } else { g.axial_leave_mm };
    let target = g.bottom_diameter_mm + if finish { 0. } else { 2. * g.radial_leave_mm };
    if finish {
        b.go(ProfilePoint { z_mm: front, ..safe }, Motion::Rapid)?;
        b.go(ProfilePoint { z_mm: front, diameter_mm: target }, Motion::Feed)?;
        b.go(ProfilePoint { z_mm: end, diameter_mm: target }, Motion::Feed)?;
        b.go(ProfilePoint { z_mm: end, ..safe }, Motion::Feed)?;
    } else {
        let columns = ((front - end) / (g.tool_width_mm * 0.8)).ceil().max(1.);
        let layers = ((stock.outer_diameter_mm - target) / (2. * g.radial_depth_mm)).ceil().max(1.);
        bounded("groove passes", (columns + 1.) * layers, 1., 4000.)?;
        for layer in 1..=layers as usize {
            let diameter = (stock.outer_diameter_mm - 2. * layer as f64 * g.radial_depth_mm).max(target);
            for col in 0..=columns as usize {
                let z = front + (end - front) * col as f64 / columns;
                b.go(ProfilePoint { z_mm: z, ..safe }, Motion::Rapid)?;
                b.go(ProfilePoint { z_mm: z, diameter_mm: diameter }, Motion::Feed)?;
                b.go(ProfilePoint { z_mm: z, ..safe }, Motion::Rapid)?;
            }
        }
    }
    b.go(safe, Motion::Rapid)?;
    Ok(b.moves)
}

/// Rebuild from validated intent; every listed operation is either emitted or rejected.
pub fn calculate(setup: &Setup) -> Result<Toolpath> {
    if setup.operations.iter().any(|o| matches!(o.cutting, Some(Cutting::Contour(_)))) {
        return super::selection::calculate(setup);
    }
    if let Some(job) = &setup.od_job
        && turning_job(job)?.profile != job.profile
    {
        // Selection planning retains the exact target and replays the separate local cuts.
        return super::selection::calculate(setup);
    }
    let mut plan = crate::TurningPlan::nl2500();
    plan.setups = vec![setup.clone()];
    plan.validate()?;
    super::selection::require_covered(setup)?;
    if setup.previous_setup.is_some() {
        return invalid("regrip stock transfer is not supported");
    }
    let stock = setup.stock.as_ref().ok_or_else(|| PlanError("stock dimensions are missing".into()))?;
    let l = limits(setup)?;
    let clip = |j: &OdJob| setup.cut_end_z_mm.map_or_else(|| Ok(j.clone()), |end| j.clipped(end));
    let od = setup.od_job.as_ref().map(clip).transpose()?;
    let id = setup.id_job.as_ref().map(clip).transpose()?;
    let mut expected = vec![];
    if od.is_some() {
        expected.extend([K::FaceRough, K::OdRough]);
    }
    if id.is_some() {
        expected.push(K::IdRough);
    }
    if od.as_ref().is_some_and(|j| j.finish) {
        expected.extend([K::FaceFinish, K::OdFinish]);
    }
    if id.as_ref().is_some_and(|j| j.finish) {
        expected.push(K::IdFinish);
    }
    if setup.operations.iter().filter(|o| !local(o.kind)).map(|o| o.kind).collect::<Vec<_>>() != expected {
        return super::selection::calculate(setup);
    }
    if !setup.operations.iter().any(|o| local(o.kind)) {
        return interior::calculate_setup(od.as_ref(), id.as_ref(), stock);
    }
    let mut finishing = false;
    let mut bored = stock.clone();
    let id_start = setup.operations.iter().position(|o| o.kind == K::IdRough).unwrap_or(0);
    for (index, op) in setup.operations.iter().enumerate() {
        let finish = matches!(op.kind, K::FaceFinish | K::OdFinish | K::IdFinish | K::OdGrooveFinish);
        if finishing && !finish {
            return invalid("全ての荒取り・穴あけを仕上げ工程より前に配置してください。");
        }
        finishing |= finish;
        if local(op.kind) && op.cutting.is_none() {
            return Err(PlanError(format!("{}: 加工寸法・送りを設定してください", op.name)));
        }
        if let Some(Cutting::Drill(d)) = &op.cutting {
            check_drill_target(setup, d)?;
            drill_moves(d, op.kind, stock, &l)?;
            if index < id_start
                && op.kind == K::Drill
                && id.as_ref().and_then(|j| j.profile.last()).is_some_and(|p| d.full_diameter_z() <= p.z_mm + EPS)
            {
                bored.inner_diameter_mm = bored.inner_diameter_mm.max(d.diameter_mm);
            }
        }
    }
    if id.is_some() && bored.inner_diameter_mm <= EPS {
        return invalid(
            "内径荒取りに必要な下穴がありません。ドリルの『指定径になる最深位置』を内径輪郭の奥まで届かせ、内径荒取りより前に配置してください。",
        );
    }
    let mut path = if let Some(j) = &od {
        let outer = super::calculate(j, stock)?;
        if let Some(i) = &id {
            let mut combined = interior::calculate_setup(Some(j), Some(i), &bored)?;
            combined.stock = stock.clone();
            combined.moves.retain(|m| interior::is_inner(m.phase));
            combined.moves.extend(outer.moves);
            combined
        } else {
            outer
        }
    } else if let Some(j) = &id {
        let mut p = interior::calculate(j, &bored)?;
        p.stock = stock.clone();
        p
    } else {
        Toolpath {
            source_setup: None,
            operation_spans: vec![],
            inner: false,
            id_path: None,
            basis: "nominal_drill_tip_and_groove_front_edge".into(),
            machine_ready: false,
            stock: stock.clone(),
            job: OdJob {
                stock_front_z_mm: l.stock_front_z_mm,
                chuck_front_z_mm: l.chuck_front_z_mm,
                clearance_mm: l.clearance_mm,
                ..OdJob::default()
            },
            rough_profile: vec![],
            moves: vec![],
            allowance_audit: vec![],
            rough_removed_volume_mm3: 0.,
            finish_removed_volume_mm3: 0.,
            warnings: vec![],
        }
    };
    let original = std::mem::take(&mut path.moves);
    for (index, op) in setup.operations.iter().enumerate() {
        let group = match &op.cutting {
            Some(Cutting::Contour(_)) => return invalid("contour operations require selection planning"),
            Some(Cutting::Drill(d)) => drill_moves(d, op.kind, stock, &l)?,
            Some(Cutting::OdGroove(g)) => {
                if op.kind == K::OdGrooveFinish
                    && !setup
                        .operations
                        .iter()
                        .take(index)
                        .any(|o| o.kind == K::OdGrooveRough && matches!(&o.cutting,Some(Cutting::OdGroove(r)) if same_groove(r,g)))
                {
                    return invalid("外径溝仕上げには、同じ溝寸法・仕上げ代の溝荒取りを先に配置してください。");
                }
                let mut groove_stock = stock.clone();
                for other in &setup.operations {
                    if let Some(Cutting::Drill(d)) = &other.cutting {
                        groove_stock.inner_diameter_mm = groove_stock.inner_diameter_mm.max(d.diameter_at(g.front_z_mm));
                    }
                    if let Some(Cutting::OdGroove(other)) = &other.cutting
                        && !same_groove(g, other)
                        && g.front_z_mm.min(other.front_z_mm) > (g.front_z_mm - g.width_mm).max(other.front_z_mm - other.width_mm) + EPS
                    {
                        return invalid("異なる寸法の溝が重なっています。重複を解消してください。");
                    }
                }
                let mut clipped = setup.clone();
                clipped.od_job = od.clone();
                clipped.id_job = id.clone();
                groove_moves(g, op.kind == K::OdGrooveFinish, &clipped, &groove_stock, &l)?
            }
            None => original.iter().filter(|m| crate::simulation::operation(m.phase) == Some(op.kind)).cloned().collect(),
        };
        if let (Some(last), Some(first)) = (path.moves.last(), group.first()) {
            if (last.to.z_mm - first.from.z_mm).abs() > EPS || last.to.z_mm < l.stock_front_z_mm + l.clearance_mm - EPS {
                return invalid("invalid operation transfer plane");
            }
            path.moves.push(Move { from: last.to, to: first.from, motion: Motion::Rapid, phase: Phase::Transfer, feed_mm_rev: None });
        }
        let start = path.moves.len();
        path.moves.extend(group);
        path.operation_spans.push(OperationSpan { operation_id: op.id.clone(), start, end: path.moves.len() });
        if path.moves.len() > MAX_MOVES {
            return invalid("combined toolpath move limit exceeded");
        }
    }
    if path.moves.is_empty() {
        return invalid("no cutting operations configured");
    }
    let mut intent = setup.clone();
    for slot in &mut intent.turret {
        slot.holder_cad = None;
        slot.insert_cad = None;
    }
    path.source_setup = Some(Box::new(intent));
    if setup.operations.iter().any(|o| local(o.kind)) {
        path.warnings = vec![
            "穴あけは先端Z基準、外径溝は工具の手前側刃先Z基準。ドリル形状は円すい＋円筒、溝工具は角形の近似。刃先R補正・実工具干渉は未検証。".into(),
            "除去体積の集計は従来の輪郭加工分のみ。穴あけ・溝を含む残材はシミュレーションで確認してください。".into(),
        ];
        if !model_linked(setup) {
            path.warnings.push("手入力の穴・溝寸法を完成形状の指定として使用しています。モデル形状との一致は検証されていません。".into());
        }
    }
    Ok(path)
}

pub fn review_nc(setup: &Setup) -> Result<String> {
    review_program(setup).map(|program| program.text)
}

pub fn review_program(setup: &Setup) -> Result<crate::post::ReviewProgram> {
    let path = calculate(setup)?;
    let mut text = crate::post::ReviewWriter::new(
        "%\nO9001 (SOLIDCRAFT REVIEW ONLY)\nM30\n(DISARMED - NOT MACHINE READY)\n(X DIAMETER MM / ABSOLUTE Z / F MM PER REV)\n(NO TOOL CALLS SPINDLE MODES OFFSETS OR NOSE COMPENSATION)\n",
        path.moves.len(),
    );
    if let Some(first) = path.moves.first() {
        text.push_str(&format!("(REQUIRED START X{:.4} Z{:.4})\n", first.from.diameter_mm, first.from.z_mm));
    }
    for (index, m) in path.moves.iter().enumerate() {
        if let Some(span) = path.operation_spans.iter().find(|s| s.start == index)
            && let Some(op) = setup.operations.iter().find(|o| o.id == span.operation_id)
        {
            let station = op.station.map_or_else(|| "UNASSIGNED".into(), |n| format!("T{n:02}"));
            text.push_str(&format!("({:?} - STATION {} - VERIFY TOOL CALL)\n", op.kind, station));
            if let Some(Cutting::Drill(d)) = &op.cutting {
                text.push_str(&format!("(DRILL DIA {:.4} POINT Z {:.4} FULL DIA Z {:.4})\n", d.diameter_mm, d.tip_end_z_mm, d.full_diameter_z()));
            }
            if let Some(Cutting::OdGroove(g)) = &op.cutting {
                text.push_str(&format!(
                    "(GROOVE FRONT EDGE Z / WIDTH {:.4} / TOOL WIDTH {:.4} / RADIAL LEAVE {:.4} / EACH WALL LEAVE {:.4})\n",
                    g.width_mm, g.tool_width_mm, g.radial_leave_mm, g.axial_leave_mm
                ));
            }
        }
        // Nine decimals keep review quantization below one nanometre. This fragment is
        // deliberately disarmed; controller-specific rounding/compensation is not certified.
        text.mark_move(index)?;
        if m.motion == Motion::Rapid {
            if (m.from.diameter_mm - m.to.diameter_mm).abs() > EPS {
                text.push_str(&format!("G00 X{:.9}\n", m.to.diameter_mm));
            } else {
                text.push_str(&format!("G00 Z{:.9}\n", m.to.z_mm));
            }
        } else {
            text.push_str(&format!("G01 X{:.9} Z{:.9} F{:.9}\n", m.to.diameter_mm, m.to.z_mm, m.feed_mm_rev.unwrap_or(0.)));
        }
    }
    text.push_str("M30\n%\n");
    Ok(text.finish())
}

#[cfg(test)]
mod tests;
