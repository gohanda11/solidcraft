//! Separate the saved finished target from a temporary, reachable turning envelope.
//! Only exact rectangular OD recesses between equal-diameter lands are bridged.
use super::*;

pub(crate) fn model_linked(setup: &Setup) -> bool {
    setup.od_job.iter().chain(&setup.id_job).any(|j| j.model_source.is_some())
}

#[derive(Clone, Copy)]
struct Recess {
    front: f64,
    back: f64,
    bottom: f64,
    land: f64,
}

fn recesses(job: &OdJob) -> Vec<Recess> {
    if job.model_source.is_none() {
        return vec![];
    }
    crate::drawing::simplified(&job.profile)
        .windows(4)
        .filter_map(|w| {
            let [a, b, c, d] = w else { return None };
            ((a.z_mm - b.z_mm).abs() < EPS
                && (c.z_mm - d.z_mm).abs() < EPS
                && (b.diameter_mm - c.diameter_mm).abs() < EPS
                && (a.diameter_mm - d.diameter_mm).abs() < EPS
                && a.z_mm > c.z_mm + EPS
                && a.diameter_mm > b.diameter_mm + EPS)
                .then_some(Recess { front: a.z_mm, back: c.z_mm, bottom: b.diameter_mm, land: a.diameter_mm })
        })
        .collect()
}

pub(super) fn groove_land(setup: &Setup, g: &Groove) -> Result<Option<f64>> {
    if !model_linked(setup) {
        return Ok(None);
    }
    let Some(job) = &setup.od_job else { return invalid("モデルの外径輪郭を取り込んでから溝を設定してください") };
    let matched = recesses(job).into_iter().find(|r| {
        (r.front - g.front_z_mm).abs() < EPS && (r.back - (g.front_z_mm - g.width_mm)).abs() < EPS && (r.bottom - g.bottom_diameter_mm).abs() < EPS
    });
    matched
        .map(|r| Some(r.land))
        .ok_or_else(|| PlanError("溝寸法が完成モデルの矩形溝と一致しません。前端Z・幅・底径を確認してください（斜面・R付きの溝は未対応）。".into()))
}

/// Input points still use the exact saved contour's endpoint selection. A range that
/// ends on a recess wall/floor cannot silently become a wider turning range.
pub(in crate::machining) fn turning_points(job: &OdJob, points: &[ProfilePoint]) -> Result<Vec<ProfilePoint>> {
    let mut result = points.to_vec();
    for r in recesses(job) {
        let in_recess = |p: &ProfilePoint| p.z_mm <= r.front + EPS && p.z_mm >= r.back - EPS && p.diameter_mm < r.land - EPS;
        if !result.iter().any(in_recess) {
            continue;
        }
        let complete = result.first().is_some_and(|p| p.z_mm >= r.front - EPS && !in_recess(p))
            && result.last().is_some_and(|p| p.z_mm <= r.back + EPS && !in_recess(p));
        if !complete {
            return invalid("矩形溝の壁・底は外径旋削の範囲にできません。溝工程を使うか、両側の外径まで範囲を選択してください");
        }
        result.retain(|p| !in_recess(p));
    }
    Ok(result)
}

pub(in crate::machining) fn turning_job(job: &OdJob) -> Result<OdJob> {
    job.validate_draft()?;
    let mut envelope = job.clone();
    envelope.profile = turning_points(job, &job.profile)?;
    Ok(envelope)
}

pub(in crate::machining) fn check_drill_target(setup: &Setup, drill: &Drill) -> Result<()> {
    let mut inner = setup.id_job.as_ref().map(|j| j.profile.as_slice());
    if inner.is_none() && model_linked(setup) {
        inner = setup.drawing.as_ref().map(|d| d.inner.as_slice()).filter(|p| p.len() >= 2);
        // A known solid model does not acquire a bore just because a drilling operation exists.
        if inner.is_none() && setup.od_job.as_ref().is_some_and(|j| j.profile.iter().any(|p| drill.diameter_at(p.z_mm) > EPS)) {
            return invalid("ドリルが穴のない完成モデルを削り込みます。モデルの穴形状を確認してください");
        }
    }
    let outer = setup.od_job.as_ref().map(|j| j.profile.as_slice());
    for (profile, is_outer) in outer.into_iter().map(|p| (p, true)).chain(inner.into_iter().map(|p| (p, false))) {
        let transition = diameter_at(profile, drill.full_diameter_z()).map(|diameter_mm| ProfilePoint { z_mm: drill.full_diameter_z(), diameter_mm });
        for p in profile.iter().chain(transition.iter()) {
            let cut = drill.diameter_at(p.z_mm);
            if cut > EPS && if is_outer { cut >= p.diameter_mm - EPS } else { cut > p.diameter_mm + EPS } {
                return invalid("ドリルの円すい・指定径が完成輪郭を削り込みます");
            }
        }
    }
    Ok(())
}
