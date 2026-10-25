//! Material comes before cutting conditions; containment is cached per document revision.
use super::*;
use cam::machining::selection::{ShortageKind, StockCoverage, stock_coverage};
use cam::machining::sequence::Cutting;

const CLEARANCE_MIN: f64 = 0.01;
const CLEARANCE_MAX: f64 = 1000.;
const CHUCK_MIN: f64 = -1e6;
const CHUCK_MAX: f64 = -0.001;

pub(super) fn refresh(plan: Option<&TurningPlan>, state: &mut State, revision: u64) {
    let Some(setup) = plan.and_then(|p| p.setups.get(state.setup)) else {
        state.stock_check = None;
        return;
    };
    if state.stock_check.as_ref().is_none_or(|(r, id, _)| *r != revision || id != &setup.id) {
        state.stock_check = Some((revision, setup.id.clone(), stock_coverage(setup).map_err(|e| e.to_string())));
    }
}

pub(super) fn coverage<'a>(state: &'a State, revision: u64, setup: &str) -> Option<&'a StockCoverage> {
    state.stock_check.as_ref().filter(|(r, id, _)| *r == revision && id == setup).and_then(|(_, _, result)| result.as_ref().ok())
}

pub(super) fn ready(setup: &Setup, coverage: Option<&StockCoverage>) -> bool {
    let Some(job) = setup.od_job.as_ref().or(setup.id_job.as_ref()) else { return false };
    (CLEARANCE_MIN..=CLEARANCE_MAX).contains(&job.clearance_mm)
        && (CHUCK_MIN..=CHUCK_MAX).contains(&job.chuck_front_z_mm)
        && job.chuck_front_z_mm + job.clearance_mm < 0.
        && coverage.is_some_and(|c| c.shortages.is_empty() && c.outer_boundary_known)
}

pub(super) fn intended_end(setup: &Setup, state: &State) -> Option<(&'static str, f64)> {
    if state.selection.editing {
        let job = if state.model_profile.inner { &setup.id_job } else { &setup.od_job };
        if let (Some(job), Some(first), Some(last)) = (job, state.selection.first, state.selection.last)
            && let Some((a, b)) = job.profile.get(first).zip(job.profile.get(last))
        {
            return Some(("cam.stock_extent_selection", a.z_mm.min(b.z_mm)));
        }
    }
    let planned = setup.operations.iter().filter_map(|op| match &op.cutting {
        Some(Cutting::Contour(c)) => c.points(setup).ok().and_then(|p| p.last().map(|p| p.z_mm)),
        Some(Cutting::Drill(d)) => Some(d.tip_end_z_mm),
        Some(Cutting::OdGroove(g)) => Some(g.front_z_mm - g.width_mm),
        None => match op.kind {
            OperationKind::FaceRough | OperationKind::FaceFinish => Some(0.),
            OperationKind::OdRough | OperationKind::OdFinish | OperationKind::IdRough | OperationKind::IdFinish => {
                let job = if matches!(op.kind, OperationKind::IdRough | OperationKind::IdFinish) { &setup.id_job } else { &setup.od_job };
                job.as_ref().and_then(|j| j.profile.last()).map(|p| setup.cut_end_z_mm.unwrap_or(p.z_mm))
            }
            _ => None,
        },
    });
    if let Some(z) = planned.filter(|z| z.is_finite()).min_by(f64::total_cmp) {
        return Some(("cam.stock_extent_operations", z));
    }
    if let Some(z) = setup.cut_end_z_mm {
        return Some(("cam.stock_extent_limited", z));
    }
    setup.od_job.as_ref().or(setup.id_job.as_ref()).and_then(|j| j.profile.last()).map(|p| ("cam.stock_extent_model", p.z_mm))
}

fn placement_context(ui: &mut egui::Ui, setup: &Setup, state: &State) {
    let Some(job) = setup.od_job.as_ref().or(setup.id_job.as_ref()) else { return };
    if !(CLEARANCE_MIN..=CLEARANCE_MAX).contains(&job.clearance_mm) || !(CHUCK_MIN..=CHUCK_MAX).contains(&job.chuck_front_z_mm) {
        return;
    }
    let limit = job.chuck_front_z_mm + job.clearance_mm;
    ui.small(format!("{} · Z {limit:.3}", tr("cam.stock_reachable_limit")));
    if limit >= 0. {
        ui.colored_label(Tokens::get().warning, tr("cam.stock_no_access"));
    } else if let Some((key, end)) = intended_end(setup, state) {
        ui.small(format!("{} · Z {end:.3}", tr(key)));
        if end < limit - 1e-8 {
            ui.colored_label(Tokens::get().warning, tr("cam.stock_chuck_range_warning"));
        }
    }
}

pub(super) fn show(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State, revision: u64) {
    ui.heading(tr("cam.stock_setup_title"));
    ui.label(tr("cam.stock_setup_help"));
    ui.separator();
    let Some(setup) = plan.setups.get_mut(state.setup) else { return };
    let placement = |s: &Setup| {
        let job = s.od_job.as_ref().or(s.id_job.as_ref());
        (s.stock.clone(), job.map(|j| [j.stock_front_z_mm, j.chuck_front_z_mm, j.clearance_mm]))
    };
    let before = placement(setup);
    ui.strong(tr("cam.stock_dimensions"));
    if let Some(stock) = &mut setup.stock {
        stock_fields(ui, stock);
    } else {
        stock_fields(ui, &mut state.stock);
        let valid = state.stock.outer_diameter_mm.is_finite()
            && state.stock.outer_diameter_mm > state.stock.inner_diameter_mm
            && state.stock.inner_diameter_mm >= 0.0
            && state.stock.length_mm.is_finite()
            && state.stock.length_mm > 0.0;
        if workflow_ui::enabled_button(ui, "cam_set_stock", tr("cam.set_stock"), valid, true) {
            setup.stock = Some(state.stock.clone());
        }
    }
    ui.small(tr("cam.stock_bore_zero"));
    ui.separator();
    ui.strong(tr("cam.stock_placement"));
    if let Some(job) = setup.od_job.as_mut().or(setup.id_job.as_mut()) {
        ui_kit::scope(ui, |ui| {
            for (key, value, minimum, maximum) in [
                ("cam.path_front_stock", &mut job.stock_front_z_mm, 0.0, 1e6),
                ("cam.path_chuck", &mut job.chuck_front_z_mm, CHUCK_MIN, CHUCK_MAX),
                ("cam.path_clearance", &mut job.clearance_mm, CLEARANCE_MIN, CLEARANCE_MAX),
            ] {
                ui_kit::row(ui, tr(key), |ui| {
                    let r = ui_kit::number_field_preserving_value(ui, value, minimum..=maximum, 0.1, " mm");
                    crate::scenario::publish_rect(key, r.rect);
                    crate::scenario::publish_handle(key, r.rect.center());
                });
            }
        });
        if !(CHUCK_MIN..=CHUCK_MAX).contains(&job.chuck_front_z_mm) {
            ui.colored_label(Tokens::get().warning, tr(if job.chuck_front_z_mm >= 0. { "cam.stock_need_chuck" } else { "cam.stock_chuck_range" }));
        }
        if !(CLEARANCE_MIN..=CLEARANCE_MAX).contains(&job.clearance_mm) {
            ui.colored_label(
                Tokens::get().warning,
                tr(if job.clearance_mm == 0. { "cam.stock_need_clearance" } else { "cam.stock_clearance_range" }),
            );
        }
    }
    if let (Some(od), Some(id)) = (&setup.od_job, &mut setup.id_job) {
        id.stock_front_z_mm = od.stock_front_z_mm;
        id.chuck_front_z_mm = od.chuck_front_z_mm;
        id.clearance_mm = od.clearance_mm;
    }
    if before != placement(setup) {
        // The cached result belongs to the saved revision, not this frame's unsaved edit.
        // Prevent advancing on that old result; refresh after the command saves the edit.
        state.stock_check = None;
    }
    placement_context(ui, setup, state);
    ui.separator();
    show_coverage(ui, state, revision, &setup.id);
    selection_ui::difference_controls(ui, state, revision);
    ui.separator();
    workflow_ui::ending(ui, setup);
    ui.separator();
    let response = controls::section_default(ui, tr("cam.material_and_conditions"), true, |ui| conditions_ui::show(ui, plan, state));
    crate::scenario::publish_rect("cam_stock_conditions", response.header_rect);
    controls::section(ui, tr("cam.machine"), |ui| {
        controls::dropdown(ui, "cam_machine_list", plan.machine.clone(), |ui| {
            for machine in ["NL2500", "NL2000", "SL25"] {
                controls::value(ui, &mut plan.machine, machine.into(), machine);
            }
        });
    });
}

pub(super) fn next(ui: &mut egui::Ui, plan: &TurningPlan, state: &mut State, revision: u64) {
    let can_continue = plan.setups.get(state.setup).is_some_and(|s| ready(s, coverage(state, revision, &s.id)));
    if workflow_ui::enabled_button(ui, "cam_guide_next", tr("cam.stock_next"), can_continue, true) {
        guided::go(state, 2);
    }
    if !can_continue {
        ui.small(tr("cam.stock_fix_before_next"));
    }
}

fn show_coverage(ui: &mut egui::Ui, state: &State, revision: u64, setup: &str) {
    let Some((_, _, result)) = state.stock_check.as_ref().filter(|(r, id, _)| *r == revision && id == setup) else { return };
    match result {
        Ok(c) if c.shortages.is_empty() => {
            ui.strong(tr(if c.outer_boundary_known && c.inner_boundary_known { "cam.stock_fits" } else { "cam.stock_partial_check" }));
            if !c.inner_boundary_known {
                ui.small(tr("cam.stock_inner_unknown"));
            }
        }
        Ok(c) => {
            ui.colored_label(Tokens::get().error, tr("cam.stock_shortage"));
            ui.label(format!("{} {:.1} mm³", tr("cam.stock_missing_volume"), c.missing_volume_mm3));
            for kind in [ShortageKind::OuterDiameter, ShortageKind::InnerDiameter, ShortageKind::Front, ShortageKind::Back] {
                let mut regions = c.shortages.iter().filter(|s| s.kind == kind);
                let Some(first) = regions.next() else { continue };
                let (mut z0, mut z1) = (first.z0_mm, first.z1_mm);
                for s in regions {
                    z0 = z0.min(s.z0_mm);
                    z1 = z1.max(s.z1_mm);
                }
                let key = match kind {
                    ShortageKind::OuterDiameter => "cam.stock_missing_od",
                    ShortageKind::InnerDiameter => "cam.stock_missing_id",
                    ShortageKind::Front => "cam.stock_missing_front",
                    ShortageKind::Back => "cam.stock_missing_back",
                };
                ui.label(format!("{} · Z {z0:.3} … {z1:.3}", tr(key)));
            }
            ui.small(tr("cam.stock_shortage_help"));
        }
        Err(error) => {
            ui.colored_label(Tokens::get().warning, error);
        }
    }
}
