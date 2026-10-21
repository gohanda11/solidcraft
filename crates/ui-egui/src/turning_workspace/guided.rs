//! Presentation-only navigation; saved edits still pass through cam.turning.set.
use super::*;
use cam::machining::{OdJob, ProfilePoint};

const STEPS: [&str; 5] = ["cam.guide_stock", "cam.guide_profile", "cam.guide_conditions", "cam.guide_review", "cam.guide_sim"];

pub(super) fn go(state: &mut State, step: usize) {
    let step = if step == 3 { 4 } else { step };
    state.operation_draft = None;
    if state.workflow != step.min(4) && state.catalog.preview.take().is_some() {
        state.catalog.fit = true;
    }
    state.workflow = step.min(4);
    state.advanced = false;
    state.spatial.playing = false;
    state.tool_editor = None;
    state.segment_editor = None;
    state.spatial.two_d = true;
    if step == 4 {
        state.machining.show_nc = false;
    }
    state.machining.show = step >= 3 && state.machining.preview.is_some();
}

fn button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let primary = matches!(id, "cam_guide_next" | "cam_path_calculate" | "cam_set_stock");
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let ink = if primary { Tokens::get().accent } else { Tokens::get().text };
            ui_kit::button(ui, egui::RichText::new(tr(label)).color(ink), ui_kit::Button::Normal)
        })
        .inner;
    crate::scenario::publish_rect(id, response.rect);
    if enabled {
        crate::scenario::publish_handle(id, response.rect.center());
    }
    response.clicked()
}

pub fn toolbar(app: &mut SolveApp, ui: &mut egui::Ui, origin: egui::Pos2) {
    let available = (ui.max_rect().right() - origin.x - 16.0).max(320.0);
    let width = (available / 4.0).min(220.0);
    for (position, (step, key)) in
        [(1, "① 輪郭・加工方向"), (0, "② 材料・削る部分"), (2, "③ 加工工程"), (4, "④ シミュレーション・NC")].iter().copied().enumerate()
    {
        let rect = egui::Rect::from_min_size(origin + egui::vec2(position as f32 * width, 0.0), egui::vec2(width - 6.0, 38.0));
        let selected = !app.turning_workspace.advanced && app.turning_workspace.workflow == step;
        let response = ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| controls::pick(ui, selected, tr(key))).inner;
        let id = format!("cam_step_{step}");
        crate::scenario::publish_rect(&id, response.rect);
        crate::scenario::publish_handle(&id, response.rect.center());
        if response.clicked() {
            go(&mut app.turning_workspace, step);
        }
    }
}

/// Only seed an empty process; never replace an existing user's operations.
pub(super) fn prepare(plan: &mut TurningPlan, state: &mut State) {
    let Some(setup) = plan.setups.get_mut(state.setup) else { return };
    if !setup.operations.is_empty() {
        return;
    }
    setup.od_job.get_or_insert_with(|| OdJob { finish: true, ..OdJob::default() });
    let finish = setup.od_job.as_ref().is_some_and(|job| job.finish);
    for kind in [OperationKind::FaceRough, OperationKind::OdRough, OperationKind::FaceFinish, OperationKind::OdFinish].into_iter().take(if finish {
        4
    } else {
        2
    }) {
        state.kind = kind;
        super::add_operation(plan, state);
    }
    state.section = 0;
}

fn missing(setup: &Setup, step: usize) -> Option<&'static str> {
    match step {
        0 if setup.stock.is_none() => Some("cam.guide_need_stock"),
        1 if (setup.od_job.is_none() && setup.id_job.is_none() && !setup.operations.iter().any(|o| cam::machining::sequence::local(o.kind)))
            || setup.od_job.iter().chain(&setup.id_job).any(|job| job.profile.len() < 2 || job.profile.iter().any(|p| p.diameter_mm <= 0.0)) =>
        {
            Some("cam.guide_need_profile")
        }
        2 if !setup.operations.iter().any(|o| matches!(o.cutting, Some(cam::machining::sequence::Cutting::Contour(_))))
            && setup.od_job.iter().chain(&setup.id_job).any(|j| {
                j.radial_depth_mm <= 0.0
                    || j.face_depth_mm <= 0.0
                    || j.rough_feed_mm_rev <= 0.0
                    || (j.finish && j.finish_feed_mm_rev <= 0.0)
                    || j.clearance_mm <= 0.0
                    || j.chuck_front_z_mm >= 0.0
            }) =>
        {
            Some("cam.guide_need_conditions")
        }
        _ => None,
    }
}

pub(super) fn settings(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State, revision: u64, bodies: &[String]) {
    let step = state.workflow.min(4);
    ui.heading(match step {
        0 => "材料・配置",
        1 => "モデル・加工方向",
        2 => "工程の設定",
        3 => "Gコード出力",
        _ => "シミュレーション",
    });
    ui.add_space(10.0);
    if step != 4 {
        let issue = plan.setups.get(state.setup).and_then(|setup| missing(setup, step));
        if navigation(ui, state, step, issue) {
            return;
        }
        ui.separator();
    }
    if step == 0 {
        super::conditions_ui::show(ui, plan, state);
    }
    if step == 2 {
        let Some(setup) = plan.setups.get_mut(state.setup) else { return };
        if !super::operations_ui::selected(ui, setup, state) {
            return;
        }
        let r = controls::section(ui, tr("cam.operation_material_conditions"), |ui| {
            super::conditions_ui::show(ui, plan, state);
        });
        crate::scenario::publish_rect("cam_operation_material_conditions", r.header_rect);
    }
    if step == 0 {
        controls::section(ui, tr("cam.machine"), |ui| {
            controls::dropdown(ui, "cam_machine_list", plan.machine.clone(), |ui| {
                for machine in ["NL2500", "NL2000", "SL25"] {
                    controls::value(ui, &mut plan.machine, machine.into(), machine);
                }
            });
            text(ui, "cam.machine", &mut plan.machine);
        });
    }
    let Some(setup) = plan.setups.get_mut(state.setup) else { return };
    if step == 4 {
        super::simulation_ui::settings(ui, setup, state, revision, &plan.tool_cad_library);
        return;
    }
    match step {
        0 => {
            ui.label(tr("cam.guide_stock_help"));
            if setup.id_job.is_some() {
                ui.label(tr("cam.id_stock_help"));
            }
            if let Some(stock) = &mut setup.stock {
                stock_fields(ui, stock);
            } else {
                stock_fields(ui, &mut state.stock);
                let ready = state.stock.outer_diameter_mm > state.stock.inner_diameter_mm && state.stock.length_mm > 0.0;
                if button(ui, "cam_set_stock", "cam.set_stock", ready) {
                    setup.stock = Some(state.stock.clone());
                }
            }
            ui.separator();
            ui.strong("材料の配置と退避");
            if let Some(job) = setup.od_job.as_mut().or(setup.id_job.as_mut()) {
                number(ui, "cam.path_front_stock", &mut job.stock_front_z_mm);
                ui.horizontal(|ui| {
                    ui_kit::number_field(ui, &mut job.chuck_front_z_mm, -1e6..=0., 0.1, " mm");
                    ui.label(tr("cam.path_chuck"));
                });
                number(ui, "cam.path_clearance", &mut job.clearance_mm);
            }
            if let (Some(od), Some(id)) = (&setup.od_job, &mut setup.id_job) {
                id.stock_front_z_mm = od.stock_front_z_mm;
                id.chuck_front_z_mm = od.chuck_front_z_mm;
                id.clearance_mm = od.clearance_mm;
            }
            // Render the difference controls after releasing the mutable setup borrow below.
        }
        1 => {
            super::model_profile::settings(ui, &mut state.model_profile, bodies, setup, revision);
            ui.separator();
            if let Some(job) = if state.model_profile.inner { &mut setup.id_job } else { &mut setup.od_job } {
                if let Some(source) = &job.model_source {
                    ui.strong(format!("{}: {}", tr("cam.model_linked"), source.body));
                    super::model_profile::chart(ui, &job.profile);
                    if source.approximation_mm > 0.0 {
                        ui.label(tr("cam.model_approximation"));
                    }
                    ui.label(tr("cam.model_link_help"));
                    controls::section(ui, tr("cam.model_values"), |ui| {
                        for p in &job.profile {
                            ui.monospace(format!("Z {:8.3}   X {:8.3}", p.z_mm, p.diameter_mm));
                        }
                    });
                    if ui_kit::button(ui, tr("cam.model_manual"), ui_kit::Button::Normal).clicked() {
                        job.model_source = None;
                    }
                } else {
                    ui.label(tr("cam.guide_profile_help"));
                    profile(ui, job);
                }
            } else if !state.model_profile.inner && button(ui, "cam_guide_enable", "cam.path_enable", true) {
                if state.model_profile.inner {
                    setup.id_job = Some(OdJob::default());
                } else {
                    setup.od_job = Some(OdJob::default());
                }
            }
        }
        2 => {
            let previous_inner = state.model_profile.inner;
            ui.horizontal(|ui| {
                controls::value(ui, &mut state.model_profile.inner, false, tr("cam.profile_od"));
                ui.add_enabled_ui(setup.id_job.is_some(), |ui| {
                    controls::value(ui, &mut state.model_profile.inner, true, tr("cam.profile_id"));
                });
            });
            if previous_inner != state.model_profile.inner {
                let finish = setup
                    .operations
                    .get(state.operation)
                    .is_some_and(|o| matches!(o.kind, OperationKind::OdFinish | OperationKind::IdFinish | OperationKind::FaceFinish));
                let kind = match (state.model_profile.inner, finish) {
                    (true, true) => OperationKind::IdFinish,
                    (true, false) => OperationKind::IdRough,
                    (false, true) => OperationKind::OdFinish,
                    (false, false) => OperationKind::OdRough,
                };
                if let Some(index) = setup.operations.iter().position(|o| o.kind == kind) {
                    state.operation = index;
                }
                state.spatial.section = state.model_profile.inner;
                state.spatial.operation = Some(kind);
            }
            if state.model_profile.inner {
                ui.label(tr("cam.id_scope"));
            }
            if let Some(job) = if state.model_profile.inner { &mut setup.id_job } else { &mut setup.od_job } {
                ui.strong(tr("cam.path_conditions"));
                number(ui, "cam.path_depth", &mut job.radial_depth_mm);
                number(ui, "cam.path_face_depth", &mut job.face_depth_mm);
                for (key, value) in [("cam.path_rough_feed", &mut job.rough_feed_mm_rev), ("cam.path_finish_feed", &mut job.finish_feed_mm_rev)] {
                    ui.horizontal(|ui| {
                        ui_kit::number_field(ui, value, 0.0..=10.0, 0.01, " mm/rev");
                        ui.label(tr(key));
                    });
                }
                ui.separator();
                ui.strong(tr("cam.path_limits"));
                number(ui, "cam.path_front_stock", &mut job.stock_front_z_mm);
                ui.horizontal(|ui| {
                    ui_kit::number_field(ui, &mut job.chuck_front_z_mm, -1e6..=0.0, 0.1, " mm");
                    ui.label(tr("cam.path_chuck"));
                });
                number(ui, "cam.path_clearance", &mut job.clearance_mm);
                ui.separator();
                ui.strong(tr("cam.path_allowances"));
                number(ui, "cam.path_radial_leave", &mut job.radial_stock_to_leave_mm);
                number(ui, "cam.path_axial_leave", &mut job.axial_stock_to_leave_mm);
                ui.small(format!("{} {:.3} mm", tr("cam.path_diameter_leave"), job.radial_stock_to_leave_mm * 2.0));
            } else {
                ui.label(tr("cam.guide_need_profile"));
                if button(ui, "cam_operation_profile", "cam.guide_profile", true) {
                    go(state, 1);
                    return;
                }
            }
            // Setup coordinates are shared; cutting feeds and radial depth remain per-tool.
            if let (Some(od), Some(id)) = (&mut setup.od_job, &mut setup.id_job) {
                if state.model_profile.inner {
                    od.stock_front_z_mm = id.stock_front_z_mm;
                    od.chuck_front_z_mm = id.chuck_front_z_mm;
                    od.clearance_mm = id.clearance_mm;
                    od.axial_stock_to_leave_mm = id.axial_stock_to_leave_mm;
                } else {
                    id.stock_front_z_mm = od.stock_front_z_mm;
                    id.chuck_front_z_mm = od.chuck_front_z_mm;
                    id.clearance_mm = od.clearance_mm;
                    id.axial_stock_to_leave_mm = od.axial_stock_to_leave_mm;
                }
            }
            ui.label(tr("cam.guide_order"));
        }
        _ => {
            super::post_ui::show(ui, setup, &mut state.operation);
            ui.label(tr("cam.guide_review_help"));
            if setup.id_job.is_some()
                && setup.stock.as_ref().is_none_or(|s| s.inner_diameter_mm <= 0.0)
                && !setup.operations.iter().any(|o| o.kind == OperationKind::Drill)
            {
                ui.colored_label(Tokens::get().warning, tr("cam.id_need_bore"));
                if button(ui, "cam_id_fix_stock", "cam.guide_stock", true) {
                    go(state, 0);
                    return;
                }
            }
            let mut ready = !(setup.id_job.is_some()
                && setup.stock.as_ref().is_none_or(|s| s.inner_diameter_mm <= 0.0)
                && !setup.operations.iter().any(|o| o.kind == OperationKind::Drill));
            for index in 0..3 {
                if let Some(key) = missing(setup, index) {
                    ready = false;
                    ui.colored_label(Tokens::get().warning, tr(key));
                    if button(ui, &format!("cam_fix_{index}"), STEPS.get(index).copied().unwrap_or("cam.guide_stock"), true) {
                        go(state, index);
                        return;
                    }
                }
            }
            ui.add_space(8.0);
            if button(ui, "cam_path_calculate", "cam.path_calculate", ready) {
                state.machining.action = Some(machining_ui::Action::Calculate(setup.id.clone()));
            }
            let valid =
                state.machining.preview.as_ref().is_some_and(|(r, id, _)| *r == revision && id == &setup.id) && !state.machining.draft_preview;
            ui.separator();
            if button(ui, "cam_path_nc_view", "cam.path_nc_view", valid) {
                state.machining.action = Some(machining_ui::Action::InspectNc(setup.id.clone(), revision));
            }
            if button(ui, "cam_path_export", "cam.path_export", valid) {
                state.machining.action = Some(machining_ui::Action::Export(setup.id.clone(), revision));
            }
            ui.label(tr("cam.path_review_only"));
            if valid && let Some((_, _, path)) = &state.machining.preview {
                for warning in path.warnings.iter().filter(|_| path.source_setup.is_some()) {
                    ui.small(warning);
                }
            }
            if !valid && state.machining.preview.is_some() {
                ui.colored_label(Tokens::get().warning, tr("cam.path_stale"));
            }
            if let Some(error) = &state.machining.error {
                ui.colored_label(Tokens::get().warning, error);
            }
        }
    }
    let empty = setup.operations.is_empty();
    if step == 0 {
        super::selection_ui::difference_controls(ui, state, revision);
    }

    if empty && state.advanced && button(ui, "cam_guide_prepare", "cam.guide_prepare", true) {
        prepare(plan, state);
    }
}

fn navigation(ui: &mut egui::Ui, state: &mut State, step: usize, issue: Option<&str>) -> bool {
    if let Some(key) = issue {
        ui.label(tr(key));
    }
    let previous = state.workflow;
    ui.horizontal(|ui| {
        if step != 1 && button(ui, "cam_guide_back", "cam.guide_back", true) {
            go(
                state,
                match step {
                    0 => 1,
                    2 => 0,
                    4 => 2,
                    _ => 4,
                },
            );
        }
        if step != 3 && button(ui, "cam_guide_next", "cam.guide_next", issue.is_none()) {
            go(
                state,
                match step {
                    1 => 0,
                    0 => 2,
                    2 => 4,
                    _ => 4,
                },
            );
        }
    });
    state.workflow != previous
}

fn profile(ui: &mut egui::Ui, job: &mut OdJob) {
    let mut remove = None;
    egui::ScrollArea::vertical().id_salt("guide_profile_rows").max_height(310.0).show(ui, |ui| {
        egui::Grid::new("guide_profile").striped(true).show(ui, |ui| {
            ui.label("Z (mm)");
            ui.label(tr("cam.guide_diameter"));
            ui.end_row();
            for (i, p) in job.profile.iter_mut().enumerate() {
                ui.allocate_ui_with_layout(egui::Vec2::from([96.0, 26.0]), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui_kit::number_field(ui, &mut p.z_mm, -1e6..=0.0, 0.1, "")
                });
                ui.allocate_ui_with_layout(egui::Vec2::from([96.0, 26.0]), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui_kit::number_field(ui, &mut p.diameter_mm, 0.0..=1e6, 0.1, "")
                });
                if ui_kit::button(ui, tr("cam.guide_delete_point"), ui_kit::Button::Small).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = remove {
        job.profile.remove(i);
    }
    if button(ui, "cam_profile_add", "cam.guide_add_point", job.profile.len() < 256) {
        job.profile.push(job.profile.last().copied().unwrap_or(ProfilePoint { z_mm: 0.0, diameter_mm: 0.0 }));
    }
}
