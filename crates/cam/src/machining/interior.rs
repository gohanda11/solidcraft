//! Through-bore planning. Reflecting diameter makes accessible ID geometry dual to OD.
//! Verification is performed again after reflection and NC quantization.
use super::*;

pub fn is_inner(phase: Phase) -> bool {
    matches!(phase, Phase::IdRough | Phase::IdRoughContour | Phase::IdFinish)
}

pub fn is_finish(phase: Phase) -> bool {
    matches!(phase, Phase::FaceFinish | Phase::OdFinish | Phase::IdFinish | Phase::OdGrooveFinish)
}

pub(super) fn rounded(p: ProfilePoint, inner: bool) -> ProfilePoint {
    ProfilePoint { z_mm: outward(p.z_mm), diameter_mm: if inner { -outward(-p.diameter_mm) } else { outward(p.diameter_mm) } }
}

fn reflect(p: ProfilePoint, center: f64) -> ProfilePoint {
    ProfilePoint { diameter_mm: center - p.diameter_mm, ..p }
}

fn mirrored(job: &OdJob, stock: &Stock) -> Result<(OdJob, Stock, f64)> {
    job.validate_draft()?;
    bounded("stock OD", stock.outer_diameter_mm, 0.001, 1e6)?;
    bounded("through pre-bore diameter", stock.inner_diameter_mm, 0.001, stock.outer_diameter_mm - 0.001)?;
    if stock.inner_diameter_mm <= 2.0 * job.clearance_mm + EPS {
        return invalid("pre-bore is too small for the requested inward clearance");
    }
    if job.profile.iter().any(|p| p.diameter_mm >= stock.outer_diameter_mm - EPS) {
        return invalid("finished ID must remain inside the stock OD");
    }
    let center = stock.outer_diameter_mm + 2.0 * job.clearance_mm;
    let mut mirror = job.clone();
    mirror.model_source = None;
    mirror.profile = job.profile.iter().map(|p| reflect(*p, center)).collect();
    let stock = Stock { outer_diameter_mm: center - stock.inner_diameter_mm, inner_diameter_mm: 0.0, length_mm: stock.length_mm };
    mirror.validate(&stock).map_err(|e| {
        PlanError(format!("ID requires a bore narrowing towards negative Z, with pre-bore <= finished ID minus twice the radial allowance: {e}"))
    })?;
    Ok((mirror, stock, center))
}

fn cavity_volume(profile: &[ProfilePoint], bore: f64, front: f64) -> f64 {
    let Some(first) = profile.first() else { return 0.0 };
    let mut extended = vec![ProfilePoint { z_mm: front, ..*first }];
    extended.extend(profile.iter().copied());
    material_volume(&extended, bore)
}

pub fn calculate(job: &OdJob, stock: &Stock) -> Result<Toolpath> {
    let (mirror, mirror_stock, center) = mirrored(job, stock)?;
    let rough = mirror.rough_profile();
    let first = rough.first().ok_or_else(|| PlanError("missing ID profile".into()))?;
    let safe = ProfilePoint { diameter_mm: mirror_stock.outer_diameter_mm + 2.0 * job.clearance_mm, z_mm: job.stock_front_z_mm + job.clearance_mm };
    let mut builder = Builder { moves: vec![], current: safe, safe, clearance: job.clearance_mm, phase: Phase::IdRough, feed: job.rough_feed_mm_rev };
    let passes = ((mirror_stock.outer_diameter_mm - first.diameter_mm) / (2.0 * job.radial_depth_mm)).ceil() as usize;
    for n in 1..=passes {
        let diameter = (mirror_stock.outer_diameter_mm - n as f64 * 2.0 * job.radial_depth_mm).max(first.diameter_mm);
        builder.go(ProfilePoint { diameter_mm: diameter, ..safe }, Motion::Rapid)?;
        builder.go(ProfilePoint { diameter_mm: diameter, z_mm: stop_z(&rough, diameter)? }, Motion::Feed)?;
        builder.return_safe()?;
    }
    builder.phase = Phase::IdRoughContour;
    builder.contour(&rough)?;
    if job.finish {
        builder.phase = Phase::IdFinish;
        builder.feed = job.finish_feed_mm_rev;
        builder.contour(&mirror.profile)?;
    }
    verify_nominal_moves(&mirror, &mirror_stock, &builder.moves)?;
    // Reuse the independently probed shoulder/interval audit, never OD face motions.
    let audit = audit(&mirror, &rough)?
        .into_iter()
        .map(|r| AllowanceRow { finished_diameter_mm: center - r.finished_diameter_mm, rough_diameter_mm: center - r.rough_diameter_mm, ..r })
        .collect();
    let rough: Vec<_> = rough.into_iter().map(|p| reflect(p, center)).collect();
    for m in &mut builder.moves {
        m.from = reflect(m.from, center);
        m.to = reflect(m.to, center);
    }
    let removed = cavity_volume(&rough, stock.inner_diameter_mm, job.stock_front_z_mm);
    let finished = cavity_volume(&job.profile, stock.inner_diameter_mm, job.stock_front_z_mm);
    Ok(Toolpath {
        source_setup: None,
        operation_spans: vec![],
        inner: true,
        id_path: None,
        basis: "nominal_tip_no_nose_or_holder_compensation".into(),
        machine_ready: false,
        stock: stock.clone(),
        job: job.clone(),
        rough_profile: rough,
        moves: builder.moves,
        allowance_audit: audit,
        rough_removed_volume_mm3: removed,
        finish_removed_volume_mm3: if job.finish { finished - removed } else { 0.0 },
        warnings: vec![
            "Through pre-bore must already exist over the entire stock length. No drilling is generated.".into(),
            "Nominal tip only: boring-bar diameter, reach, insert nose and holder are not simulated.".into(),
        ],
    })
}

fn verify_inner(job: &OdJob, stock: &Stock, moves: &[Move]) -> Result<()> {
    let (mirror, mirror_stock, center) = mirrored(job, stock)?;
    let reflected: Vec<_> = moves.iter().map(|m| Move { from: reflect(m.from, center), to: reflect(m.to, center), ..m.clone() }).collect();
    verify_nominal_moves(&mirror, &mirror_stock, &reflected)
}

pub(super) fn verify_combined(path: &Toolpath) -> Result<()> {
    let od: Vec<_> = path.moves.iter().filter(|m| !is_inner(m.phase) && m.phase != Phase::Transfer).cloned().collect();
    let id: Vec<_> = path.moves.iter().filter(|m| is_inner(m.phase)).cloned().collect();
    if !path.inner {
        verify_nominal_moves(&path.job, &path.stock, &od)?;
    }
    let id_job = if path.inner { Some(&path.job) } else { path.id_path.as_ref().map(|p| &p.job) };
    if let Some(job) = id_job {
        verify_inner(job, &path.stock, &id)?;
    }
    for m in path.moves.iter().filter(|m| m.phase == Phase::Transfer) {
        if m.motion != Motion::Rapid
            || m.from.z_mm.min(m.to.z_mm) < path.job.stock_front_z_mm + path.job.clearance_mm - EPS
            || (m.from.z_mm - m.to.z_mm).abs() > EPS
            || !m.to.diameter_mm.is_finite()
            || m.to.diameter_mm < 0.0
        {
            return invalid("invalid front-of-stock transfer");
        }
    }
    Ok(())
}

/// Combines complete passes, keeping all roughing before any finishing.
pub fn calculate_setup(od: Option<&OdJob>, id: Option<&OdJob>, stock: &Stock) -> Result<Toolpath> {
    let Some(od) = od else { return calculate(id.ok_or_else(|| PlanError("configure an OD or ID profile".into()))?, stock) };
    let mut outer = super::calculate(od, stock)?;
    let Some(id) = id else { return Ok(outer) };
    let inner = calculate(id, stock)?;
    // One setup has one coordinate frame and one common clearance plane.
    for (a, b) in [
        (od.stock_front_z_mm, id.stock_front_z_mm),
        (od.chuck_front_z_mm, id.chuck_front_z_mm),
        (od.clearance_mm, id.clearance_mm),
        (od.axial_stock_to_leave_mm, id.axial_stock_to_leave_mm),
    ] {
        if (a - b).abs() > EPS {
            return invalid("OD and ID must share front stock Z, chuck Z, clearance and axial allowance");
        }
    }
    if od.profile.last().map(|p| p.z_mm) != id.profile.last().map(|p| p.z_mm) {
        let delta = od.profile.last().zip(id.profile.last()).map(|(a, b)| (a.z_mm - b.z_mm).abs());
        if delta.is_none_or(|d| d > 1e-6) {
            return invalid("OD and through ID must cover the same axial extent");
        }
    }
    for p in od.profile.iter().chain(&id.profile) {
        let o = diameter_at(&od.profile, p.z_mm).ok_or_else(|| PlanError("missing OD extent".into()))?;
        let i = diameter_at(&id.profile, p.z_mm).ok_or_else(|| PlanError("missing ID extent".into()))?;
        // Check every side of a shoulder too; no zero-thickness wall is allowed.
        let min_o = od.profile.iter().filter(|q| (q.z_mm - p.z_mm).abs() < EPS).map(|q| q.diameter_mm).fold(o, f64::min);
        if i >= min_o - EPS {
            return invalid("ID intersects the finished OD");
        }
    }
    let groups = [
        outer.moves.iter().filter(|m| !is_finish(m.phase)).cloned().collect::<Vec<_>>(),
        inner.moves.iter().filter(|m| !is_finish(m.phase)).cloned().collect(),
        outer.moves.iter().filter(|m| is_finish(m.phase)).cloned().collect(),
        inner.moves.iter().filter(|m| is_finish(m.phase)).cloned().collect(),
    ];
    let mut moves: Vec<Move> = vec![];
    for group in groups {
        if let (Some(last), Some(first)) = (moves.last(), group.first()) {
            moves.push(Move { from: last.to, to: first.from, motion: Motion::Rapid, phase: Phase::Transfer, feed_mm_rev: None });
        }
        moves.extend(group);
    }
    if moves.len() > MAX_MOVES {
        return invalid("combined toolpath move limit exceeded");
    }
    outer.moves = moves;
    // OD facing has already removed the front cap before ID roughing.
    let inner_rough = cavity_volume(&inner.rough_profile, stock.inner_diameter_mm, od.axial_stock_to_leave_mm);
    outer.rough_removed_volume_mm3 += inner_rough;
    let cap = cavity_volume(&inner.rough_profile, stock.inner_diameter_mm, od.axial_stock_to_leave_mm)
        - cavity_volume(&inner.rough_profile, stock.inner_diameter_mm, 0.0);
    if od.finish {
        outer.finish_removed_volume_mm3 -= cap;
    }
    if id.finish {
        let front = if od.finish { 0.0 } else { od.axial_stock_to_leave_mm };
        outer.finish_removed_volume_mm3 +=
            cavity_volume(&id.profile, stock.inner_diameter_mm, front) - cavity_volume(&inner.rough_profile, stock.inner_diameter_mm, front);
    }
    outer.warnings.extend(inner.warnings);
    outer.id_path = Some(ProfileSummary { job: inner.job, rough_profile: inner.rough_profile, allowance_audit: inner.allowance_audit });
    verify_combined(&outer)?;
    Ok(outer)
}

#[cfg(test)]
mod tests;
