//! Operations bound to explicit endpoints of the saved model contour.
//! Residual stock is a conservative nominal-tip axial grid, not certified cutter geometry.
use super::*;
use crate::{Operation, OperationKind as K, Setup, simulation::Slab};
use sequence::{Cutting, Limits, OperationSpan};

mod coverage;
pub(super) use coverage::require_covered;
pub use coverage::{ShortageKind, StockCoverage, StockShortage, stock_coverage};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contour {
    pub limits: Limits,
    pub inner: bool,
    pub first: usize,
    pub last: usize,
    pub profile_snapshot: Vec<ProfilePoint>,
    pub source_snapshot: Option<ModelSource>,
    pub radial_leave_mm: f64,
    pub axial_leave_mm: f64,
    pub depth_mm: f64,
    pub feed_mm_rev: f64,
}
impl Contour {
    pub fn finishing(&self) -> bool {
        self.radial_leave_mm == 0. && self.axial_leave_mm == 0.
    }
    pub fn kind(&self) -> K {
        match (self.inner, self.finishing()) {
            (false, false) => K::OdRough,
            (false, true) => K::OdFinish,
            (true, false) => K::IdRough,
            (true, true) => K::IdFinish,
        }
    }
    pub fn validate(&self, kind: K) -> Result<()> {
        if kind != self.kind() || self.first >= self.last || self.last >= 256 || self.profile_snapshot.len() > 256 {
            return invalid("輪郭の始点・終点、取り代と工程種別を確認してください");
        }
        OdJob { profile: self.profile_snapshot.clone(), model_source: self.source_snapshot.clone(), ..OdJob::default() }.validate_draft()?;
        for (name, n, min, max) in [
            ("radial leave", self.radial_leave_mm, 0., 100.),
            ("axial leave", self.axial_leave_mm, 0., 100.),
            ("depth", self.depth_mm, 0., 100.),
            ("feed", self.feed_mm_rev, 0., 10.),
            ("stock front", self.limits.stock_front_z_mm, 0., 1e6),
            ("chuck", self.limits.chuck_front_z_mm, -1e6, 0.),
            ("clearance", self.limits.clearance_mm, 0., 1000.),
        ] {
            bounded(name, n, min, max)?;
        }
        Ok(())
    }
    pub fn points(&self, setup: &Setup) -> Result<Vec<ProfilePoint>> {
        self.validate(self.kind())?;
        let job = if self.inner { &setup.id_job } else { &setup.od_job };
        let job = job.as_ref().ok_or_else(|| PlanError("先にモデルから輪郭を取り込んでください".into()))?;
        job.validate_draft()?;
        if self.profile_snapshot != job.profile || self.source_snapshot != job.model_source {
            return invalid("輪郭または加工方向が変わりました。この工程の範囲を選び直してください");
        }
        let points = job.profile.get(self.first..=self.last).ok_or_else(|| PlanError("輪郭が変わりました。加工範囲を選び直してください".into()))?;
        let points = if self.inner { points.to_vec() } else { sequence::turning_points(job, points)? };
        for pair in points.windows(2) {
            let [a, b] = pair else { continue };
            if b.z_mm > a.z_mm + EPS || if self.inner { b.diameter_mm > a.diameter_mm + EPS } else { b.diameter_mm < a.diameter_mm - EPS } {
                return invalid("この範囲は通常の外径・内径旋削で到達できません。溝や裏向きの面は別工程に分けてください");
            }
        }
        if points.first().zip(points.last()).is_none_or(|(a, b)| a.z_mm - b.z_mm <= EPS) {
            return invalid("Z方向に長さのある輪郭を選択してください。端面は端面工程で設定します");
        }
        Ok(points)
    }
    fn envelope(&self, setup: &Setup) -> Result<Vec<ProfilePoint>> {
        let points = self.points(setup)?;
        let Some(first) = points.first() else { return invalid("empty contour") };
        let front = if first.z_mm.abs() < EPS { self.limits.stock_front_z_mm } else { first.z_mm };
        let mut rough: Vec<_> = points
            .iter()
            .map(|p| ProfilePoint {
                z_mm: (p.z_mm + self.axial_leave_mm).min(front),
                diameter_mm: p.diameter_mm + if self.inner { -2. * self.radial_leave_mm } else { 2. * self.radial_leave_mm },
            })
            .collect();
        if let (Some(last), Some(r)) = (points.last(), rough.last().copied())
            && r.z_mm > last.z_mm + EPS
        {
            rough.push(ProfilePoint { z_mm: last.z_mm, ..r });
        }
        Ok(rough)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Residual {
    pub slabs: Vec<Slab>,
    pub remaining_mm3: f64,
    pub resolution_mm: f64,
    /// Material missing from the initial stock, independently of later cuts.
    #[serde(default)]
    pub coverage: StockCoverage,
}
fn initial(setup: &Setup, l: &Limits) -> Result<Vec<Slab>> {
    let stock = setup.stock.as_ref().ok_or_else(|| PlanError("材料を入力してください".into()))?;
    bounded("stock diameter", stock.outer_diameter_mm, 0.001, 1e6)?;
    bounded("stock bore", stock.inner_diameter_mm, 0., stock.outer_diameter_mm - 0.001)?;
    bounded("stock length", stock.length_mm, 0.001, 1e6)?;
    let back = l.stock_front_z_mm - stock.length_mm;
    let count = (stock.length_mm / 0.1).ceil().clamp(1., 4096.) as usize;
    let mut zs: Vec<_> = (0..=count).map(|n| back + stock.length_mm * n as f64 / count as f64).collect();
    zs.push(0.);
    for j in setup.od_job.iter().chain(&setup.id_job) {
        for p in &j.profile {
            zs.extend([p.z_mm, p.z_mm + j.axial_stock_to_leave_mm]);
        }
    }
    if let Some(drawing) = &setup.drawing {
        zs.extend(drawing.inner.iter().map(|p| p.z_mm));
    }
    for op in &setup.operations {
        match &op.cutting {
            Some(Cutting::Contour(c)) => zs.extend(c.envelope(setup)?.iter().map(|p| p.z_mm)),
            Some(Cutting::OdGroove(g)) => {
                zs.extend([g.front_z_mm, g.front_z_mm - g.width_mm, g.front_z_mm - g.axial_leave_mm, g.front_z_mm - g.width_mm + g.axial_leave_mm])
            }
            Some(Cutting::Drill(d)) => zs.extend([d.tip_end_z_mm, d.full_diameter_z()]),
            None => {}
        }
    }
    zs.retain(|z| z.is_finite() && *z >= back && *z <= l.stock_front_z_mm);
    zs.sort_by(f64::total_cmp);
    zs.dedup_by(|a, b| (*a - *b).abs() < EPS);
    if zs.len() > 16384 {
        return invalid("残材断面数が上限を超えました");
    }
    Ok(zs
        .windows(2)
        .filter_map(|w| {
            let [a, b] = w else { return None };
            Some(Slab { z0: *a, z1: *b, inner: stock.inner_diameter_mm * 0.5, outer: stock.outer_diameter_mm * 0.5 })
        })
        .collect())
}
fn replay(slabs: &mut [Slab], moves: &[Move], cutting: Option<&Cutting>) {
    for m in moves.iter().filter(|m| m.motion == Motion::Feed) {
        match cutting {
            Some(Cutting::Drill(d)) => {
                for s in slabs.iter_mut() {
                    let radius = ((s.z0 - m.from.z_mm.min(m.to.z_mm)) * (d.point_angle_rad * 0.5).tan()).clamp(0., d.diameter_mm * 0.5);
                    s.inner = s.inner.max(radius.min(s.outer));
                }
            }
            Some(Cutting::OdGroove(g)) => {
                for s in slabs.iter_mut() {
                    if s.z0 >= m.from.z_mm.min(m.to.z_mm) - g.tool_width_mm - EPS && s.z1 <= m.from.z_mm.max(m.to.z_mm) + EPS {
                        s.outer = s.outer.min((m.from.diameter_mm.min(m.to.diameter_mm) * 0.5).max(s.inner));
                    }
                }
            }
            _ => crate::simulation::remove(slabs, m.from, m.to, m.phase),
        }
    }
}
fn residual(setup: &Setup, slabs: Vec<Slab>) -> Result<Residual> {
    let remaining_mm3 = slabs
        .iter()
        .map(|s| {
            let z = (s.z0 + s.z1) * 0.5;
            let outer = setup.od_job.as_ref().and_then(|j| diameter_at(&j.profile, z)).map(|d| d * 0.5);
            let inner = setup
                .id_job
                .as_ref()
                .and_then(|j| diameter_at(&j.profile, z))
                .or_else(|| setup.drawing.as_ref().and_then(|d| diameter_at(&d.inner, z)))
                .map(|d| d * 0.5)
                .unwrap_or(s.inner);
            let finished = outer.map_or(if z > 0. { 0. } else { (s.outer * s.outer - s.inner * s.inner).max(0.) }, |r| {
                (r.min(s.outer).powi(2) - inner.max(s.inner).powi(2)).max(0.)
            });
            ((s.outer * s.outer - s.inner * s.inner) - finished).max(0.) * (s.z1 - s.z0) * std::f64::consts::PI
        })
        .sum();
    let resolution_mm = slabs.iter().map(|s| s.z1 - s.z0).fold(0., f64::max);
    Ok(Residual { slabs, remaining_mm3, resolution_mm, coverage: stock_coverage(setup)? })
}
/// Initial difference is available before any operations or cutting conditions exist.
pub fn difference(setup: &Setup) -> Result<Residual> {
    // Bound and validate source geometry before the axial grid visits any input points.
    stock_coverage(setup)?;
    if !setup.od_job.iter().chain(&setup.id_job).any(|j| j.profile.len() >= 2) {
        return invalid("先にモデルから完成輪郭を取得してください");
    }
    let l = setup
        .od_job
        .as_ref()
        .or(setup.id_job.as_ref())
        .map(|j| Limits { stock_front_z_mm: j.stock_front_z_mm, chuck_front_z_mm: j.chuck_front_z_mm, clearance_mm: j.clearance_mm })
        .unwrap_or_default();
    residual(setup, initial(setup, &l)?)
}
pub fn remaining(setup: &Setup) -> Result<Residual> {
    let (_, slabs) = plan(setup)?;
    residual(setup, slabs)
}
fn contour_moves(c: &Contour, setup: &Setup, slabs: &[Slab], l: &Limits) -> Result<Vec<Move>> {
    c.validate(c.kind())?;
    bounded("切込み", c.depth_mm, 0.001, 100.)?;
    bounded("送り", c.feed_mm_rev, 0.001, 10.)?;
    if [(c.limits.stock_front_z_mm, l.stock_front_z_mm), (c.limits.chuck_front_z_mm, l.chuck_front_z_mm), (c.limits.clearance_mm, l.clearance_mm)]
        .iter()
        .any(|(a, b)| (a - b).abs() > EPS)
    {
        return invalid("材料の配置・退避が変わりました。加工範囲を選び直して経路を確認してください");
    }
    let stock = setup.stock.as_ref().ok_or_else(|| PlanError("材料未設定".into()))?;
    if c.inner && c.points(setup)?.iter().any(|p| p.diameter_mm < stock.inner_diameter_mm - EPS) {
        return invalid("材料の下穴径が完成内径より大きいため加工できません");
    }
    let target = c.envelope(setup)?;
    let (Some(first), Some(last)) = (target.first(), target.last()) else { return invalid("empty selection") };
    if first.z_mm > l.stock_front_z_mm + EPS
        || last.z_mm < l.chuck_front_z_mm + l.clearance_mm - EPS
        || last.z_mm < l.stock_front_z_mm - stock.length_mm - EPS
    {
        return invalid("選択範囲が材料外またはチャックの退避領域に入っています");
    }
    if setup.cut_end_z_mm.is_some_and(|end| last.z_mm < end - EPS) {
        return invalid("選択範囲が共通の加工終端より奥です");
    }
    for p in &target {
        bounded("切削径", p.diameter_mm, 0.001, stock.outer_diameter_mm)?;
        if !c.inner && p.diameter_mm <= stock.inner_diameter_mm + EPS {
            return invalid("外径輪郭が材料の下穴に入っています");
        }
        let opposite = if c.inner { &setup.od_job } else { &setup.id_job };
        if let Some(d) = opposite.as_ref().and_then(|j| diameter_at(&j.profile, p.z_mm))
            && if c.inner { p.diameter_mm >= d - EPS } else { p.diameter_mm <= d + EPS }
        {
            return invalid("外径と内径の輪郭が交差しています");
        }
    }
    let cells: Vec<_> = slabs.iter().filter(|s| s.z1 > last.z_mm + EPS && s.z0 < first.z_mm - EPS && s.outer > s.inner + EPS).collect();
    let start = if c.inner {
        cells.iter().map(|s| s.inner * 2.).fold(stock.outer_diameter_mm, f64::min)
    } else {
        cells.iter().map(|s| s.outer * 2.).fold(0., f64::max)
    };
    let safe_x = if c.inner {
        let entry =
            slabs.iter().filter(|s| s.z1 > last.z_mm + EPS && s.outer > s.inner + EPS).map(|s| s.inner * 2.).fold(stock.outer_diameter_mm, f64::min);
        if entry <= 2. * l.clearance_mm {
            return invalid("内径工具の進入径がありません。先行ドリルの指定径到達深さ・下穴径を確認してください");
        }
        entry - 2. * l.clearance_mm
    } else {
        stock.outer_diameter_mm + 2. * l.clearance_mm
    };
    let extreme =
        target.iter().map(|p| p.diameter_mm).fold(if c.inner { 0. } else { stock.outer_diameter_mm }, if c.inner { f64::max } else { f64::min });
    let distance = if c.inner { extreme - start } else { start - extreme };
    let count = if c.finishing() { 1. } else { (distance / (2. * c.depth_mm)).ceil().max(1.) };
    bounded("passes", count, 1., 2000.)?;
    let safe = ProfilePoint { z_mm: l.stock_front_z_mm + l.clearance_mm, diameter_mm: safe_x };
    let mut b = Builder {
        moves: vec![],
        current: safe,
        safe,
        clearance: l.clearance_mm,
        phase: match (c.inner, c.finishing()) {
            (false, false) => Phase::OdRough,
            (false, true) => Phase::OdFinish,
            (true, false) => Phase::IdRough,
            (true, true) => Phase::IdFinish,
        },
        feed: c.feed_mm_rev,
    };
    for n in 1..=count as usize {
        let layer = if c.inner { start + 2. * c.depth_mm * n as f64 } else { start - 2. * c.depth_mm * n as f64 };
        let mut points = target.clone();
        if !c.finishing() {
            for p in &mut points {
                p.diameter_mm = if c.inner { p.diameter_mm.min(layer) } else { p.diameter_mm.max(layer) };
            }
        }
        b.go(ProfilePoint { z_mm: first.z_mm, ..safe }, Motion::Rapid)?;
        for p in points {
            b.go(p, Motion::Feed)?;
        }
        b.go(ProfilePoint { z_mm: last.z_mm, ..safe }, Motion::Rapid)?;
        b.go(safe, Motion::Rapid)?;
    }
    Ok(b.moves)
}

fn covered(c: &Contour, setup: &Setup, before: usize) -> bool {
    setup.operations.iter().take(before).any(|o| {
        if let Some(Cutting::Contour(r)) = &o.cutting {
            r.inner == c.inner
                && r.first <= c.first
                && r.last >= c.last
                && !r.finishing()
                && r.radial_leave_mm <= c.depth_mm
                && r.axial_leave_mm <= c.depth_mm
        } else {
            let job = if c.inner { &setup.id_job } else { &setup.od_job };
            o.kind == if c.inner { K::IdRough } else { K::OdRough }
                && job.as_ref().is_some_and(|j| j.radial_stock_to_leave_mm <= c.depth_mm && j.axial_stock_to_leave_mm <= c.depth_mm)
        }
    })
}
fn light_stock(c: &Contour, setup: &Setup, slabs: &[Slab]) -> Result<bool> {
    let points = c.points(setup)?;
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return Ok(false) };
    for slab in slabs.iter().filter(|s| s.z1 > last.z_mm + EPS && s.z0 < first.z_mm - EPS && s.outer > s.inner + EPS) {
        let z0 = slab.z0.max(last.z_mm);
        let z1 = slab.z1.min(first.z_mm);
        let values = points
            .iter()
            .filter(|p| p.z_mm >= z0 && p.z_mm <= z1)
            .map(|p| p.diameter_mm)
            .chain(diameter_at(&points, z0))
            .chain(diameter_at(&points, z1));
        let target = values.fold(if c.inner { 0. } else { f64::INFINITY }, if c.inner { f64::max } else { f64::min }) * 0.5;
        let remaining = if c.inner { target - slab.inner } else { slab.outer - target };
        if !remaining.is_finite() || remaining > c.depth_mm + EPS {
            return Ok(false);
        }
    }
    Ok(true)
}
pub fn calculate(setup: &Setup) -> Result<Toolpath> {
    if setup.operations.is_empty() {
        return invalid("先に加工工程を追加してください");
    }
    Ok(plan(setup)?.0)
}
fn plan(setup: &Setup) -> Result<(Toolpath, Vec<Slab>)> {
    if setup.previous_setup.is_some() {
        return invalid("つかみ直し後の残材転送は未対応です。独立した段取りの材料を設定してください");
    }
    let mut validation = crate::TurningPlan::nl2500();
    let mut isolated = setup.clone();
    isolated.previous_setup = None;
    validation.setups = vec![isolated];
    validation.validate()?;
    require_covered(setup)?;
    let l = sequence::limits(setup)?;
    let mut slabs = initial(setup, &l)?;
    let stock = setup.stock.as_ref().ok_or_else(|| PlanError("材料未設定".into()))?;
    let job = setup.od_job.as_ref().or(setup.id_job.as_ref()).cloned().unwrap_or_default();
    let mut path = Toolpath {
        source_setup: None,
        operation_spans: vec![],
        inner: setup.od_job.is_none(),
        id_path: setup.id_job.as_ref().filter(|_| setup.od_job.is_some()).map(|j| ProfileSummary {
            job: j.clone(),
            rough_profile: vec![],
            allowance_audit: vec![],
        }),
        basis: "selected_contour_nominal_tip".into(),
        machine_ready: false,
        stock: stock.clone(),
        job,
        rough_profile: vec![],
        moves: vec![],
        allowance_audit: vec![],
        rough_removed_volume_mm3: 0.,
        finish_removed_volume_mm3: 0.,
        warnings: vec!["選択輪郭の名目刃先経路。残材は保守的な軸方向断面近似。刃先R補正・実機干渉は未検証。".into()],
    };
    let mut finishing = false;
    for (index, op) in setup.operations.iter().enumerate() {
        let finish = matches!(op.kind, K::FaceFinish | K::OdFinish | K::IdFinish | K::OdGrooveFinish);
        if finishing && !finish {
            return invalid("全ての荒取り・穴あけを仕上げより前に配置してください");
        }
        finishing |= finish;
        let group = match &op.cutting {
            Some(Cutting::Contour(c)) => {
                if c.finishing() && !covered(c, setup, index) && !light_stock(c, setup, &slabs)? {
                    return invalid("この範囲の荒取りが未確認です。先に同じ範囲の荒取りを追加してください");
                }
                contour_moves(c, setup, &slabs, &l)?
            }
            Some(Cutting::Drill(d)) => {
                sequence::check_drill_target(setup, d)?;
                sequence::drill_moves(d, op.kind, stock, &l)?
            }
            Some(Cutting::OdGroove(g)) => {
                if setup.cut_end_z_mm.is_some_and(|end| g.front_z_mm - g.width_mm < end - EPS) {
                    return invalid("溝が共通の加工終端より奥にあります");
                }
                if setup.operations.iter().filter_map(|o|o.cutting.as_ref()).any(|c|matches!(c,Cutting::OdGroove(other) if !sequence::same_groove(g,other)&&g.front_z_mm.min(other.front_z_mm)>(g.front_z_mm-g.width_mm).max(other.front_z_mm-other.width_mm)+EPS)) {
                    return invalid("異なる寸法の溝が重なっています。重複を解消してください");
                }
                if finish
                    && !setup
                        .operations
                        .iter()
                        .take(index)
                        .any(|o| o.kind == K::OdGrooveRough && matches!(&o.cutting,Some(Cutting::OdGroove(r)) if sequence::same_groove(r,g)))
                {
                    return invalid("同じ寸法の溝荒取りが先に必要です");
                }
                let mut groove_stock = stock.clone();
                groove_stock.inner_diameter_mm = slabs
                    .iter()
                    .filter(|s| s.z1 > g.front_z_mm - g.width_mm && s.z0 < g.front_z_mm)
                    .map(|s| s.inner * 2.)
                    .fold(stock.inner_diameter_mm, f64::max);
                sequence::groove_moves(g, finish, setup, &groove_stock, &l)?
            }
            None => {
                if !matches!(op.kind, K::FaceRough | K::FaceFinish | K::OdRough | K::OdFinish | K::IdRough | K::IdFinish) {
                    return invalid("工具経路未対応の工程、または穴・溝の未設定寸法があります");
                }
                let inner = matches!(op.kind, K::IdRough | K::IdFinish);
                let j = if inner { &setup.id_job } else { &setup.od_job };
                let j = j.as_ref().ok_or_else(|| PlanError("輪郭未設定".into()))?;
                let j = if inner { j.clone() } else { sequence::turning_job(j)? };
                let mut j = if let Some(end) = setup.cut_end_z_mm { j.clipped(end)? } else { j };
                j.finish = finish;
                if finish
                    && !setup.operations.iter().take(index).any(|o| {
                        o.cutting.is_none()
                            && o.kind
                                == match op.kind {
                                    K::FaceFinish => K::FaceRough,
                                    K::IdFinish => K::IdRough,
                                    _ => K::OdRough,
                                }
                    })
                {
                    return invalid("輪郭全体の仕上げには、同じ種類の全体荒取りが先に必要です。部分仕上げは輪郭点から作成してください");
                }
                let mut bored = stock.clone();
                if inner {
                    bored.inner_diameter_mm = slabs
                        .iter()
                        .filter(|s| j.profile.last().is_some_and(|p| s.z1 > p.z_mm + EPS) && s.outer > s.inner + EPS)
                        .map(|s| s.inner * 2.)
                        .fold(stock.outer_diameter_mm, f64::min);
                }
                let all = if inner { interior::calculate(&j, &bored)? } else { super::calculate(&j, stock)? };
                all.moves.into_iter().filter(|m| crate::simulation::operation(m.phase) == Some(op.kind)).collect()
            }
        };
        if let (Some(last), Some(first)) = (path.moves.last(), group.first()) {
            if (last.to.z_mm - first.from.z_mm).abs() > EPS || last.to.z_mm < l.stock_front_z_mm + l.clearance_mm - EPS {
                return invalid("invalid transfer plane");
            }
            if (last.to.diameter_mm - first.from.diameter_mm).abs() > EPS {
                path.moves.push(Move { from: last.to, to: first.from, motion: Motion::Rapid, phase: Phase::Transfer, feed_mm_rev: None });
            }
        }
        let volume =
            |slabs: &[Slab]| slabs.iter().map(|s| (s.outer.powi(2) - s.inner.powi(2)).max(0.) * (s.z1 - s.z0) * std::f64::consts::PI).sum::<f64>();
        let before = volume(&slabs);
        replay(&mut slabs, &group, op.cutting.as_ref());
        let removed = (before - volume(&slabs)).max(0.);
        if finish {
            path.finish_removed_volume_mm3 += removed;
        } else {
            path.rough_removed_volume_mm3 += removed;
        }
        let start = path.moves.len();
        path.moves.extend(group);
        path.operation_spans.push(OperationSpan { operation_id: op.id.clone(), start, end: path.moves.len() });
        if path.moves.len() > MAX_MOVES {
            return invalid("combined move limit exceeded");
        }
    }
    let mut source = setup.clone();
    for t in &mut source.turret {
        t.holder_cad = None;
        t.insert_cad = None;
    }
    path.source_setup = Some(Box::new(source));
    if setup.od_job.as_ref().is_some_and(|job| sequence::turning_job(job).is_ok_and(|envelope| envelope.profile != job.profile)) {
        path.warnings.push("外径旋削は矩形溝の両側の外径をつなぐ経路です。溝の壁・底は残り、別の溝工程で加工します。完成輪郭と残材表示にはモデルの溝を保持しています。".into());
    }
    if !sequence::model_linked(setup) && setup.operations.iter().any(|o| sequence::local(o.kind)) {
        path.warnings.push("手入力の穴・溝寸法を完成形状の指定として使用しています。モデル形状との一致は検証されていません。".into());
    }
    Ok((path, slabs))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub operations: Vec<Operation>,
    pub insert_at: usize,
    pub rough_then_finish: bool,
    pub residual: Residual,
    pub path: Toolpath,
}
/// Preview and apply use this same deterministic proposal; a caller binds it to a revision.
pub fn propose(setup: &Setup, station: u8, c: &Contour) -> Result<Proposal> {
    c.validate(c.kind())?;
    let slot = setup.turret.iter().find(|t| t.station == station).ok_or_else(|| PlanError("工具を選択してください".into()))?;
    let compatible = if c.inner {
        matches!(slot.role, crate::ToolRole::IdRough | crate::ToolRole::IdFinish)
    } else {
        matches!(slot.role, crate::ToolRole::OdRough | crate::ToolRole::OdFinish)
    };
    if !compatible {
        return invalid("輪郭側と工具用途が一致しません");
    }
    let finish_at = setup
        .operations
        .iter()
        .position(|o| matches!(o.kind, K::FaceFinish | K::OdFinish | K::IdFinish | K::OdGrooveFinish | K::PartOff | K::PartingFinish))
        .unwrap_or(setup.operations.len());
    let mut prefix = setup.clone();
    prefix.operations.truncate(finish_at);
    let (_, prior_stock) = plan(&prefix)?;
    let rough_then_finish = c.finishing() && !covered(c, setup, finish_at) && !light_stock(c, setup, &prior_stock)?;
    let insert_at = if !c.finishing() || rough_then_finish {
        finish_at
    } else {
        setup.operations.iter().position(|o| matches!(o.kind, K::PartOff | K::PartingFinish)).unwrap_or(setup.operations.len())
    };
    let mut candidate = setup.clone();
    candidate.operations.truncate(insert_at);
    let mut operations = vec![];
    let mut add = |cut: Contour, station: u8| {
        let id = (1..=8225)
            .map(|n| format!("range-{n}"))
            .find(|id| !setup.operations.iter().chain(operations.iter()).any(|o: &Operation| &o.id == id))
            .unwrap_or_else(|| "range-overflow".into());
        operations.push(Operation {
            spindle: None,
            id,
            name: match cut.kind() {
                K::OdRough => "外径荒",
                K::OdFinish => "外径仕上げ",
                K::IdRough => "内径荒",
                _ => "内径仕上げ",
            }
            .into(),
            kind: cut.kind(),
            station: Some(station),
            cutting: Some(Cutting::Contour(cut)),
            manual_separation: None,
        });
    };
    if rough_then_finish {
        let job = if c.inner { setup.id_job.as_ref() } else { setup.od_job.as_ref() }
            .ok_or_else(|| PlanError("荒取り条件を設定する輪郭がありません。先にモデルから輪郭を取得してください".into()))?;
        let side = if c.inner { "内径" } else { "外径" };
        if !job.radial_depth_mm.is_finite() || !(0.001..=100.).contains(&job.radial_depth_mm) {
            return Err(PlanError(format!("自動追加する{side}荒取りの切込みを0.001～100 mmで設定してから、もう一度プレビューしてください。")));
        }
        if !job.rough_feed_mm_rev.is_finite() || !(0.001..=10.).contains(&job.rough_feed_mm_rev) {
            return Err(PlanError(format!("自動追加する{side}荒取りの送りを0.001～10 mm/revで設定してから、もう一度プレビューしてください。")));
        }
        let role = if c.inner { crate::ToolRole::IdRough } else { crate::ToolRole::OdRough };
        let rough_station =
            setup.turret.iter().find(|t| t.role == role).map(|t| t.station).ok_or_else(|| PlanError("荒取り工具を登録してください".into()))?;
        let mut rough = c.clone();
        rough.radial_leave_mm = 0.1;
        rough.axial_leave_mm = 0.1;
        rough.depth_mm = job.radial_depth_mm;
        rough.feed_mm_rev = job.rough_feed_mm_rev;
        add(rough, rough_station);
    }
    add(c.clone(), station);
    candidate.operations.extend(operations.clone());
    let (path, slabs) = plan(&candidate)?;
    Ok(Proposal { operations, insert_at, rough_then_finish, residual: residual(&candidate, slabs)?, path })
}

#[cfg(test)]
mod tests;
