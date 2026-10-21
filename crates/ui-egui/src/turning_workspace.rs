//! Manual turning process planning. The document is the sole source of saved data;
//! every accepted edit uses the same validated, undoable command as scripts.
use serde_json::json;
use solidcraft_engine::doc::cam::{self, ManualSeparation, Operation, OperationKind, PlannedOutcome, Setup, Stock, ToolRole, TurningPlan};

use crate::{SolveApp, i18n::tr, theme::Tokens, ui_kit};

mod catalog_ui;
mod conditions_ui;
mod controls;
mod guided;
pub mod machining_ui;
mod model_profile;
mod operations_ui;
mod selection_ui;
mod simulation_ui;
pub mod spatial;
pub use guided::toolbar;

#[derive(Clone, Debug)]
pub struct State {
    setup: usize,
    operation: usize,
    section: usize,
    workflow: usize,
    advanced: bool,
    model_profile: model_profile::View,
    kind: OperationKind,
    stock: Stock,
    bore: f64,
    error: Option<String>,
    pub machining: machining_ui::View,
    spatial: spatial::View,
    simulation: simulation_ui::View,
    condition_level: cam::presets::Level,
    catalog: catalog_ui::View,
    operation_draft: Option<operations_ui::Draft>,
    selection: selection_ui::View,
}

impl Default for State {
    fn default() -> Self {
        Self {
            setup: 0,
            operation: 0,
            section: 0,
            workflow: 1,
            advanced: false,
            model_profile: model_profile::View::default(),
            kind: OperationKind::OdRough,
            stock: Stock { outer_diameter_mm: 0.0, inner_diameter_mm: 0.0, length_mm: 0.0 },
            bore: 0.0,
            error: None,
            machining: machining_ui::View::default(),
            spatial: spatial::View::default(),
            simulation: simulation_ui::View::default(),
            condition_level: cam::presets::Level::default(),
            catalog: catalog_ui::View::default(),
            operation_draft: None,
            selection: selection_ui::View::default(),
        }
    }
}

pub fn active(app: &SolveApp) -> bool {
    app.ui.tab == "TURNING"
}

/// Shared by the workspace menu and the control channel.
pub fn enter(app: &mut SolveApp, workspace: &str) -> Result<(), String> {
    match workspace {
        "design" => crate::render_workspace::enter(app, false),
        "render" => crate::render_workspace::enter(app, true),
        "turning" => {
            if app.session.active_sketch.is_some() || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
                return Err(tr("cam.finish_modelling").into());
            }
            crate::render_workspace::enter(app, false)?;
            app.ui.tab = "TURNING".into();
            Ok(())
        }
        _ => Err("workspace must be design, render or turning".into()),
    }
}

const KINDS: &[(OperationKind, &str)] = &[
    (OperationKind::FaceRough, "cam.face_rough"),
    (OperationKind::FaceFinish, "cam.face_finish"),
    (OperationKind::CenterDrill, "cam.center_drill"),
    (OperationKind::Drill, "cam.drill"),
    (OperationKind::OdRough, "cam.od_rough"),
    (OperationKind::OdFinish, "cam.od_finish"),
    (OperationKind::IdRough, "cam.id_rough"),
    (OperationKind::IdFinish, "cam.id_finish"),
    (OperationKind::OdGroove, "cam.od_groove"),
    (OperationKind::OdGrooveRough, "cam.od_groove_rough"),
    (OperationKind::OdGrooveFinish, "cam.od_groove_finish"),
    (OperationKind::IdGroove, "cam.id_groove"),
    (OperationKind::FaceGroove, "cam.face_groove"),
    (OperationKind::OdThread, "cam.od_thread"),
    (OperationKind::IdThread, "cam.id_thread"),
    (OperationKind::PartOff, "cam.part_off"),
    (OperationKind::PartingFinish, "cam.parting_finish"),
];

const ROLES: &[(ToolRole, &str)] = &[
    (ToolRole::CenterDrill, "cam.center_drill"),
    (ToolRole::Drill, "cam.drill"),
    (ToolRole::Vacant, "cam.vacant"),
    (ToolRole::IdRough, "cam.id_rough"),
    (ToolRole::OdFinish, "cam.od_finish"),
    (ToolRole::IdFinish, "cam.id_finish"),
    (ToolRole::GrooveOrThread, "cam.groove_thread"),
    (ToolRole::OdGrooveRough, "cam.od_groove_rough"),
    (ToolRole::OdRough, "cam.od_rough"),
];

fn kind_label(kind: OperationKind) -> &'static str {
    if kind == OperationKind::Face {
        return "cam.face";
    }
    KINDS.iter().find(|(k, _)| *k == kind).map(|(_, label)| *label).unwrap_or("cam.operation")
}

fn choice<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, value: &mut T, choices: &[(T, &str)]) {
    let label = choices.iter().find(|(v, _)| v == value).map(|(_, label)| *label).unwrap_or("");
    controls::dropdown(ui, id, (tr(label)).to_string(), |ui| {
        for (v, label) in choices {
            controls::value(ui, value, *v, tr(label));
        }
    });
}

fn number(ui: &mut egui::Ui, key: &str, value: &mut f64) {
    ui.horizontal(|ui| {
        ui_kit::number_field(ui, value, 0.0..=1e6, 0.1, " mm");
        ui.label(tr(key));
    });
}

fn text(ui: &mut egui::Ui, key: &str, value: &mut String) {
    ui.label(tr(key));
    let response = ui_kit::bounded_cell_editor(ui, ui.next_auto_id(), value, "", false, 64);
    crate::scenario::publish_rect(key, response.rect);
}

fn stock_fields(ui: &mut egui::Ui, stock: &mut Stock) {
    number(ui, "cam.stock_od", &mut stock.outer_diameter_mm);
    number(ui, "cam.stock_id", &mut stock.inner_diameter_mm);
    number(ui, "cam.stock_length", &mut stock.length_mm);
}

fn next_id(plan: &TurningPlan, prefix: &str) -> String {
    // A validated plan has at most 32 setups and 8192 operations.
    for n in 1..=8225 {
        let id = format!("{prefix}-{n}");
        if plan.setups.iter().all(|s| s.id != id && s.operations.iter().all(|op| op.id != id)) {
            return id;
        }
    }
    // The engine rejects this duplicate if a malformed caller ever exceeds the bound.
    format!("{prefix}-overflow")
}

fn add_operation(plan: &mut TurningPlan, state: &mut State) {
    let id = next_id(plan, "operation");
    let Some(setup) = plan.setups.get_mut(state.setup) else { return };
    let station = state.kind.suggested_station().filter(|n| setup.turret.iter().any(|s| s.station == *n && s.role != ToolRole::Vacant));
    let operation = Operation {
        cutting: None,
        id,
        name: tr(kind_label(state.kind)).into(),
        kind: state.kind,
        station,
        manual_separation: (state.kind == OperationKind::PartingFinish)
            .then_some(ManualSeparation { finished_bore_diameter_mm: state.bore, stop_diameter_mm: state.bore }),
    };
    // Keep an existing separation last when inserting ordinary operations.
    let at = if setup.planned_outcome() != PlannedOutcome::RemainsClamped
        && !matches!(state.kind, OperationKind::PartOff | OperationKind::PartingFinish)
    {
        setup.operations.len().saturating_sub(1)
    } else {
        setup.operations.len()
    };
    setup.operations.insert(at, operation);
    state.operation = at;
    state.section = 2;
}

fn outline(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State) {
    ui.heading(tr("cam.workspace"));
    ui.label(&plan.machine);
    ui.label(&plan.material);
    ui.separator();
    ui.strong(tr("cam.setups"));
    for (i, setup) in plan.setups.iter().enumerate() {
        let response = controls::pick(ui, state.setup == i, &setup.name);
        crate::scenario::publish_rect(&format!("cam_setup_{i}"), response.rect);
        if response.clicked() {
            state.setup = i;
            state.operation = 0;
            guided::go(state, 0);
        }
    }
    let add = ui.add_enabled_ui(state.advanced && plan.setups.len() < 32, |ui| ui_kit::button(ui, tr("cam.add_setup"), ui_kit::Button::Normal)).inner;
    crate::scenario::publish_rect("cam_add_setup", add.rect);
    if add.clicked() {
        let id = next_id(plan, "setup");
        let previous = plan.setups.last();
        plan.setups.push(Setup {
            name: format!("{} {}", tr("cam.setup"), plan.setups.len() + 1),
            id,
            previous_setup: previous.map(|s| s.id.clone()),
            stock: None,
            turret: previous.map(|s| s.turret.clone()).unwrap_or_else(cam::nl2500_turret),
            operations: Vec::new(),
            od_job: None,
            id_job: None,
            cut_end_z_mm: None,
        });
        state.setup = plan.setups.len().saturating_sub(1);
        state.operation = 0;
        state.section = 0;
    }
    ui.separator();
    ui.strong(tr("cam.operations"));
    if operations_ui::button(ui, "cam_add_operation", "cam.add_operation", true) {
        operations_ui::open(plan, state);
    }
    ui.small(tr("cam.operation_select_help"));
    if let Some(setup) = plan.setups.get(state.setup) {
        for (i, op) in setup.operations.iter().enumerate() {
            let station = op.station.map(|s| format!("T{s:02}")).unwrap_or_else(|| "—".into());
            let response = ui
                .allocate_ui_with_layout(egui::Vec2::from([ui.available_width(), 32.0]), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    controls::pick(
                        ui,
                        state.operation == i && state.section == 2 && state.operation_draft.is_none(),
                        format!("{:02}  {station}  {}", i + 1, op.name),
                    )
                })
                .inner;
            crate::scenario::publish_rect(&format!("cam_operation_{i}"), response.rect);
            if response.clicked() {
                state.operation_draft = None;
                state.operation = i;
                state.section = 2;
                if !state.advanced {
                    guided::go(state, 2);
                    state.model_profile.inner = matches!(op.kind, OperationKind::IdRough | OperationKind::IdFinish);
                    state.spatial.section = state.model_profile.inner;
                    state.spatial.operation = Some(op.kind);
                    state.machining.show = true;
                }
            }
        }
        if setup.operations.is_empty() {
            ui.small(tr("cam.no_operations"));
        }
    }
    ui.separator();
    let response = controls::pick(ui, state.advanced, tr("cam.guide_advanced"));
    crate::scenario::publish_rect("cam_advanced", response.rect);
    if response.clicked() {
        state.operation_draft = None;
        state.advanced = !state.advanced;
    }
    if !state.advanced {
        ui.small(tr("cam.guide_order"));
    }
}

fn inspector(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State, revision: u64) {
    ui.horizontal(|ui| {
        for (i, label) in ["cam.stock", "cam.tools", "cam.operation", "cam.path_tab"].into_iter().enumerate() {
            let response = controls::value(ui, &mut state.section, i, tr(label));
            crate::scenario::publish_rect(&format!("cam_section_{i}"), response.rect);
        }
    });
    ui.separator();
    if state.section == 0 {
        text(ui, "cam.machine", &mut plan.machine);
        text(ui, "cam.material", &mut plan.material);
    }
    let setup_count = plan.setups.len();
    let Some(setup) = plan.setups.get_mut(state.setup) else { return };
    ui.heading(&setup.name);
    match state.section {
        0 => {
            text(ui, "cam.setup_name", &mut setup.name);
            if let Some(previous) = &setup.previous_setup {
                ui.small(format!("{}: {previous}", tr("cam.previous_setup")));
                ui.small(tr("cam.regrip_help"));
            }
            ui.separator();
            if let Some(stock) = &mut setup.stock {
                stock_fields(ui, stock);
                if ui_kit::button(ui, tr("cam.clear_stock"), ui_kit::Button::Normal).clicked() {
                    setup.stock = None;
                }
            } else {
                ui.label(tr("cam.stock_unset"));
                stock_fields(ui, &mut state.stock);
                let response = ui_kit::button(ui, tr("cam.set_stock"), ui_kit::Button::Normal);
                crate::scenario::publish_rect("cam_set_stock", response.rect);
                if response.clicked() {
                    setup.stock = Some(state.stock.clone());
                }
            }
            ui.separator();
            ui.small(tr("cam.edit_help"));
            if ui
                .add_enabled_ui(setup_count > 1 && state.setup + 1 == setup_count, |ui| {
                    ui_kit::button(ui, tr("cam.remove_setup"), ui_kit::Button::Normal)
                })
                .inner
                .clicked()
            {
                plan.setups.pop();
                state.setup = plan.setups.len().saturating_sub(1);
                state.operation = 0;
            }
        }
        1 => {
            ui.small(tr("cam.tool_help"));
            for slot in &mut setup.turret {
                ui.push_id(slot.station, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(format!("T{:02}", slot.station));
                        choice(ui, "role", &mut slot.role, ROLES);
                    });
                    let mut name = slot.tool_name.clone().unwrap_or_default();
                    let response = ui_kit::bounded_cell_editor(ui, ui.next_auto_id(), &mut name, tr("cam.tool_name"), false, 64);
                    crate::scenario::publish_rect(&format!("cam_tool_name_{}", slot.station), response.rect);
                    if response.changed() {
                        slot.tool_name = (!name.is_empty()).then_some(name);
                    }
                    let mut offset = slot.offset_number.unwrap_or(0);
                    ui.horizontal(|ui| {
                        if ui_kit::number_field(ui, &mut offset, 0..=u16::MAX, 1.0, "").changed() {
                            slot.offset_number = (offset > 0).then_some(offset);
                        }
                        ui.label(tr("cam.offset"));
                    });
                    ui.separator();
                });
            }
        }
        3 => machining_ui::settings(ui, setup, &mut state.machining, revision),
        _ => {
            let count = setup.operations.len();
            if let Some(op) = setup.operations.get_mut(state.operation) {
                ui.strong(tr(kind_label(op.kind)));
                text(ui, "cam.operation_name", &mut op.name);
                ui.label(tr("cam.station"));
                controls::dropdown(
                    ui,
                    "operation_station",
                    (op.station.map(|n| format!("T{n:02}")).unwrap_or_else(|| tr("cam.unassigned").into())).to_string(),
                    |ui| {
                        controls::value(ui, &mut op.station, None, tr("cam.unassigned"));
                        for slot in &setup.turret {
                            if slot.role != ToolRole::Vacant {
                                controls::value(ui, &mut op.station, Some(slot.station), format!("T{:02}", slot.station));
                            }
                        }
                    },
                );
                if let Some(manual) = &mut op.manual_separation {
                    ui.separator();
                    number(ui, "cam.finished_bore", &mut manual.finished_bore_diameter_mm);
                    number(ui, "cam.stop_diameter", &mut manual.stop_diameter_mm);
                    ui.small(tr("cam.manual_help"));
                }
                ui.separator();
                let mut movement = None;
                ui.horizontal(|ui| {
                    if ui.add_enabled_ui(state.operation > 0, |ui| ui_kit::button(ui, tr("cam.move_up"), ui_kit::Button::Normal)).inner.clicked() {
                        movement = Some(state.operation - 1);
                    }
                    if ui
                        .add_enabled_ui(state.operation + 1 < count, |ui| ui_kit::button(ui, tr("cam.move_down"), ui_kit::Button::Normal))
                        .inner
                        .clicked()
                    {
                        movement = Some(state.operation + 1);
                    }
                });
                if let Some(to) = movement {
                    setup.operations.swap(state.operation, to);
                    state.operation = to;
                }
                let remove = ui_kit::button(ui, tr("cam.remove_operation"), ui_kit::Button::Normal);
                crate::scenario::publish_rect("cam_remove_operation", remove.rect);
                if remove.clicked() {
                    setup.operations.remove(state.operation);
                    state.operation = state.operation.saturating_sub(1);
                }
            } else {
                ui.label(tr("cam.no_operations"));
            }
            ui.separator();
            ui.label(tr(match setup.planned_outcome() {
                PlannedOutcome::RemainsClamped => "cam.remains_clamped",
                PlannedOutcome::CutOff => "cam.cut_off",
                PlannedOutcome::ManualSeparationRequired => "cam.manual_separation",
            }));
        }
    }
}

pub fn panels(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut state = std::mem::take(&mut app.turning_workspace);
    let original = app.session.doc.turning.clone();
    let mut plan = original.as_deref().cloned();
    let bodies: Vec<String> = app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    let frame = egui::Frame::NONE.fill(Tokens::get().panel).inner_margin(12);
    egui::Panel::left("sc_turning_outline").exact_size(260.0).resizable(false).frame(frame).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(plan) = &mut plan {
                state.setup = state.setup.min(plan.setups.len().saturating_sub(1));
                if state.advanced {
                    outline(ui, plan, &mut state);
                } else {
                    selection_ui::tools(ui, plan, &mut state);
                }
            } else {
                ui.heading(tr("cam.workspace"));
                ui.label(tr("cam.start_help"));
                let response = ui_kit::button(ui, tr("cam.create_plan"), ui_kit::Button::Normal);
                crate::scenario::publish_rect("cam_create_plan", response.rect);
                if response.clicked() {
                    match app
                        .run("cam.turning.defaults", json!({}))
                        .and_then(|v| serde_json::from_value(v["plan"].clone()).map_err(|e| e.to_string()))
                    {
                        Ok(template) => {
                            state.workflow = 1;
                            plan = Some(template);
                        }
                        Err(e) => state.error = Some(e),
                    }
                }
            }
        });
    });
    if let Some(plan) = &mut plan {
        egui::Panel::right("sc_turning_inspector").exact_size(350.0).resizable(false).frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt((state.advanced, state.workflow, state.section, state.operation_draft.is_some())).show(ui, |ui| {
                if !state.advanced {
                    selection_ui::operations(ui, plan, &mut state);
                }
                if state.operation_draft.is_some() {
                    operations_ui::composer(ui, plan, &mut state);
                } else if state.advanced {
                    if ui_kit::button(ui, tr("cam.guide_return"), ui_kit::Button::Normal).clicked() {
                        state.advanced = false;
                    }
                    inspector(ui, plan, &mut state, app.session.revision);
                } else if state.workflow == 2 && state.selection.editing {
                    selection_ui::settings(ui, plan, &mut state, app.session.revision);
                } else {
                    guided::settings(ui, plan, &mut state, app.session.revision, &bodies);
                }
                if let Some(error) = &state.error {
                    ui.separator();
                    ui.label(error);
                }
            });
        });
    }
    if plan.as_ref() != original.as_deref() {
        match app.run("cam.turning.set", json!({"plan": plan})) {
            Ok(_) => state.error = None,
            Err(error) => {
                state.error = Some(format!("{}\n{error}", tr("cam.rejected")));
                state.machining.action = None;
            }
        }
    }
    machining_ui::process(app, &mut state.machining);
    model_profile::process(app, &mut state.model_profile);
    simulation_ui::process(app, &mut state);
    catalog_ui::process(app, &mut state);
    selection_ui::process(app, &mut state);
    let fit = std::mem::take(&mut state.catalog.fit);
    app.turning_workspace = state;
    if fit {
        app.fit_view();
    }
}

#[cfg(test)]
mod tests;

pub fn catalogue_bounds(app: &SolveApp) -> Option<solidcraft_engine::geom::Aabb3> {
    catalog_ui::bounds(app)
}
