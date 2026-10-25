use super::*;
use cam::{
    machining::Toolpath,
    simulation::{Holder, HolderKind, Simulation, ToolAssembly},
};

#[derive(Clone, Debug)]
pub(super) struct View {
    pub report: Option<(u64, String, Simulation)>,
    pub selected_station: u8,
    pub resolution: f64,
    pub action: Option<String>,
    pub error: Option<String>,
    pub stop_on_contact: bool,
    pub focus_contact: Option<usize>,
    pub bounds: Option<[f64; 4]>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            report: None,
            selected_station: 12,
            resolution: 0.1,
            action: None,
            error: None,
            stop_on_contact: true,
            focus_contact: None,
            bounds: None,
        }
    }
}

fn button(ui: &mut egui::Ui, id: &str, key: &str) -> bool {
    let r = ui_kit::button(ui, tr(key), ui_kit::Button::Normal);
    crate::scenario::publish_rect(id, r.rect);
    crate::scenario::publish_handle(id, r.rect.center());
    r.clicked()
}
fn field(ui: &mut egui::Ui, key: &str, value: &mut f64, signed: bool) {
    ui.horizontal(|ui| {
        ui_kit::number_field(ui, value, if signed { -1000.0..=1000.0 } else { 0.0..=1000.0 }, 0.1, " mm");
        ui.label(tr(key));
    });
}
pub(super) fn settings(ui: &mut egui::Ui, setup: &mut Setup, state: &mut State, revision: u64, library: &[cam::holder_cad::LibraryEntry]) {
    if button(ui, "cam_sim_back", "cam.guide_back") {
        guided::go(state, 3);
        return;
    }
    tool_settings(ui, setup, state, library);
    report_settings(ui, setup, state, revision);
}

pub(super) fn tool_settings(ui: &mut egui::Ui, setup: &mut Setup, state: &mut State, library: &[cam::holder_cad::LibraryEntry]) {
    super::catalog_ui::show(ui, setup, state, library);
    ui.label(tr("cam.sim_scope"));
    if button(ui, "cam_insert_all", "cam.insert_all") {
        for slot in &mut setup.turret {
            if !matches!(slot.role, ToolRole::OdRough | ToolRole::OdFinish | ToolRole::IdRough | ToolRole::IdFinish) {
                continue;
            }
            let tool = slot.simulation.get_or_insert_with(Default::default);
            if tool.insert.is_none() {
                tool.triangular_insert = true;
                tool.nose_radius_mm.get_or_insert(0.4);
                tool.insert = Some(cam::insert::Insert { inner: matches!(slot.role, ToolRole::IdRough | ToolRole::IdFinish), ..Default::default() });
            }
        }
    }
    ui.small(tr("cam.insert_assumed"));
    let panel = controls::section_default(ui, tr("cam.sim_tool_settings"), state.tool_editor.is_some(), |ui| {
        controls::dropdown(ui, "cam_sim_station", (format!("T{:02}", state.simulation.selected_station)).to_string(), |ui| {
            for slot in setup.turret.iter().filter(|s| s.role != ToolRole::Vacant) {
                controls::value(
                    ui,
                    &mut state.simulation.selected_station,
                    slot.station,
                    format!("T{:02} {}", slot.station, slot.tool_name.as_deref().unwrap_or("")),
                );
            }
        });
        let Some(slot) = setup.turret.iter_mut().find(|s| s.station == state.simulation.selected_station) else { return };
        if slot.simulation.is_none() {
            ui.colored_label(Tokens::get().warning, tr("cam.sim_unset"));
            if button(ui, "cam_sim_add_tool", "cam.sim_add_tool") {
                let known = matches!(slot.station, 5 | 12);
                slot.simulation = Some(ToolAssembly {
                    triangular_insert: known,
                    nose_radius_mm: known.then_some(0.4),
                    holder: Some(Holder { kind: if slot.station == 5 { HolderKind::Round } else { HolderKind::Rectangular }, ..Default::default() }),
                    holder_verified: false,
                    insert: None,
                });
            }
            return;
        }
        let Some(tool) = &mut slot.simulation else { return };
        if ui_kit::checkbox(ui, tr("cam.sim_triangle"), &mut tool.triangular_insert).changed() && !tool.triangular_insert {
            tool.insert = None;
        }
        let mut radius = tool.nose_radius_mm.unwrap_or(0.);
        ui.label(tr("cam.sim_nose"));
        let before = radius;
        controls::dropdown(ui, "cam_insert_radius", (format!("R{radius:.2}")).to_string(), |ui| {
            for r in [0.1, 0.2, 0.4, 0.8, 1.2, 1.6] {
                controls::value(ui, &mut radius, r, format!("R{r}"));
            }
        });
        ui.horizontal(|ui| {
            ui_kit::number_field(ui, &mut radius, 0.05..=2., 0.05, " mm");
            ui.label(tr("cam.sim_nose"));
        });
        if radius != before {
            tool.nose_radius_mm = Some(radius);
        }
        let mut enabled = tool.insert.is_some();
        if ui.add_enabled_ui(tool.triangular_insert, |ui| ui_kit::checkbox(ui, tr("cam.insert_enable"), &mut enabled)).inner.changed() {
            tool.insert =
                enabled.then(|| cam::insert::Insert { inner: matches!(slot.role, ToolRole::IdRough | ToolRole::IdFinish), ..Default::default() });
            if enabled {
                tool.nose_radius_mm.get_or_insert(0.4);
            }
        }
        if let Some(insert) = &mut tool.insert {
            ui.horizontal(|ui| {
                ui.label(tr("cam.insert_size"));
                controls::dropdown(ui, "cam_insert_size", (format!("{} mm", insert.inscribed_diameter_mm)).to_string(), |ui| {
                    for d in [3.97, 4.76, 6.35, 9.525, 12.7] {
                        controls::value(ui, &mut insert.inscribed_diameter_mm, d, format!("{d} mm"));
                    }
                });
            });
            field(ui, "cam.insert_size", &mut insert.inscribed_diameter_mm, false);
            field(ui, "cam.insert_thickness", &mut insert.thickness_mm, false);
            ui.horizontal(|ui| {
                ui.label(tr("cam.insert_side"));
                controls::value(ui, &mut insert.inner, false, tr("cam.profile_od"));
                controls::value(ui, &mut insert.inner, true, tr("cam.profile_id"));
            });
            let mut angle = insert.orientation_rad.to_degrees();
            ui.horizontal(|ui| {
                ui.label(tr("cam.insert_orientation"));
                if ui_kit::number_field(ui, &mut angle, -180.0..=180., 1., "°").changed() {
                    insert.orientation_rad = angle.to_radians();
                }
            });
            ui.small(tr("cam.insert_reference"));
        }
        ui.small(tr("cam.sim_insert_unchecked"));
        // Opening the editor must not mutate the plan or invalidate an existing replay.
        let mut holder = tool
            .holder
            .clone()
            .unwrap_or_else(|| Holder { kind: if slot.station == 5 { HolderKind::Round } else { HolderKind::Rectangular }, ..Default::default() });
        let h = &mut holder;
        let before = h.clone();
        ui.horizontal(|ui| {
            controls::value(ui, &mut h.kind, HolderKind::Rectangular, tr("cam.sim_rectangular"));
            controls::value(ui, &mut h.kind, HolderKind::Round, tr("cam.sim_round"));
        });
        field(ui, "cam.sim_length", &mut h.length_mm, false);
        field(ui, if h.kind == HolderKind::Round { "cam.sim_diameter" } else { "cam.sim_width" }, &mut h.width_mm, false);
        if h.kind == HolderKind::Rectangular {
            field(ui, "cam.sim_height", &mut h.height_mm, false);
        }
        ui.small(tr("cam.sim_coordinates"));
        field(ui, "cam.sim_z_start", &mut h.z_start_mm, true);
        field(ui, "cam.sim_r_center", &mut h.r_center_mm, true);
        field(ui, "cam.sim_t_center", &mut h.t_center_mm, true);
        if *h != before {
            tool.holder = Some(h.clone());
            tool.holder_verified = false;
        }
        ui.add_enabled_ui(h.ready(), |ui| {
            ui_kit::checkbox(ui, tr("cam.sim_confirm"), &mut tool.holder_verified);
        });
        if !tool.holder_verified {
            ui.colored_label(Tokens::get().warning, tr("cam.sim_unverified"));
        }
    });
    crate::scenario::publish_rect("cam_sim_tool_settings", panel.header_rect);
    crate::scenario::publish_handle("cam_sim_tool_settings", panel.header_rect.center());
}

pub(super) fn report_settings(ui: &mut egui::Ui, setup: &Setup, state: &mut State, revision: u64) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(tr("cam.sim_resolution"));
        controls::dropdown(ui, "cam_sim_resolution", (format!("{} mm", state.simulation.resolution)).to_string(), |ui| {
            for n in [0.1, 0.25, 0.5, 1., 2.] {
                controls::value(ui, &mut state.simulation.resolution, n, format!("{n} mm"));
            }
        });
    });
    if state.machining.draft_preview {
        ui.colored_label(Tokens::get().warning, tr("cam.sim_unsaved_candidate"));
    }
    if button(ui, "cam_sim_calculate", if state.machining.draft_preview { "cam.sim_saved_operations" } else { "cam.sim_calculate" }) {
        state.simulation.action = Some(setup.id.clone());
    }
    ui_kit::checkbox(ui, tr("cam.sim_stop_contact"), &mut state.simulation.stop_on_contact);
    let valid = !state.machining.draft_preview
        && state
            .simulation
            .report
            .as_ref()
            .is_some_and(|(r, id, sim)| *r == revision && id == &setup.id && sim.resolution_mm == state.simulation.resolution);
    if let Some((_, _, report)) = &state.simulation.report {
        if !valid {
            ui.colored_label(Tokens::get().warning, tr("cam.path_stale"));
        } else {
            ui.label(format!("{}: {}", tr("cam.sim_contacts"), report.contacts.len()));
            let stations = |ns: &[u8]| ns.iter().map(|n| format!("T{n:02}")).collect::<Vec<_>>().join(", ");
            if !report.insert_stations.is_empty() {
                ui.label(format!("{}: {}", tr("cam.insert_active"), stations(&report.insert_stations)));
            }
            if !report.nominal_stations.is_empty() {
                ui.small(format!("{}: {}", tr("cam.insert_nominal"), stations(&report.nominal_stations)));
            }
            if !report.missing_stations.is_empty() {
                ui.colored_label(
                    Tokens::get().warning,
                    format!(
                        "{}: {}",
                        tr("cam.sim_missing"),
                        report.missing_stations.iter().map(|n| format!("T{n:02}")).collect::<Vec<_>>().join(", ")
                    ),
                );
            }
            ui.small(format!("{}: {}", tr("cam.sim_unchecked_moves"), report.unchecked_moves));
            if report.contacts.is_empty() && !report.checked_stations.is_empty() {
                ui.label(tr("cam.sim_no_candidate"));
            }
            if report.checked_stations.is_empty() {
                ui.colored_label(Tokens::get().warning, tr("cam.sim_no_check"));
            }
            egui::ScrollArea::vertical().id_salt("cam_sim_contacts").max_height(170.).show(ui, |ui| {
                for (index, c) in report.contacts.iter().enumerate() {
                    let label = format!("#{} T{:02} {}", c.move_index + 1, c.station, contact_reason(&c.target));
                    let response = controls::pick(ui, state.simulation.focus_contact == Some(index), label);
                    let handle = format!("cam_sim_contact_{index}");
                    crate::scenario::publish_rect(&handle, response.rect);
                    crate::scenario::publish_handle(&handle, response.rect.center());
                    if response.clicked() {
                        state.machining.step = c.move_index;
                        state.machining.before_move = None;
                        state.spatial.playing = false;
                        state.simulation.focus_contact = Some(index);
                        state.flat.refit();
                    }
                }
            });
            if report.contact_list_truncated {
                ui.small(tr("cam.sim_contact_limit"));
            }
        }
    }
    if let Some(error) = &state.simulation.error {
        error_label(ui, error);
    }
    ui.small(tr("cam.sim_limitations"));
}

pub(super) fn process(app: &mut SolveApp, state: &mut State) {
    let Some(setup) = state.simulation.action.take() else { return };
    if state.catalog.preview.take().is_some() {
        state.catalog.fit = true;
    }
    state.simulation.report = None;
    state.simulation.bounds = None;
    state.simulation.focus_contact = None;
    state.spatial.playing = false;
    #[derive(serde::Deserialize)]
    struct Response {
        revision: u64,
        setup: String,
        toolpath: Toolpath,
        simulation: Simulation,
    }
    match app
        .run("cam.turning.simulate", json!({"setup":setup,"expected_revision":app.session.revision,"resolution_mm":state.simulation.resolution}))
        .and_then(|value| serde_json::from_value::<Response>(value).map_err(|e| e.to_string()))
    {
        Ok(r) => {
            state.simulation.bounds = app
                .session
                .doc
                .turning
                .as_ref()
                .and_then(|plan| plan.setups.iter().find(|setup| setup.id == r.setup))
                .map(|setup| spatial::toolpath_bounds(setup, &r.toolpath));
            state.machining.preview = Some((r.revision, r.setup.clone(), r.toolpath));
            state.machining.generation = state.machining.generation.wrapping_add(1);
            state.machining.draft_preview = false;
            state.selection.preview = None;
            state.selection.picking = true;
            state.machining.show = true;
            state.machining.step = 0;
            state.machining.before_move = Some(0);
            state.machining.show_nc = false;
            state.machining.action = Some(machining_ui::Action::InspectNc(r.setup.clone(), r.revision));
            state.simulation.report = Some((r.revision, r.setup, r.simulation));
            state.simulation.error = None;
            state.spatial.two_d = true;
        }
        Err(e) => state.simulation.error = Some(e),
    }
}

pub(super) fn current(app: &SolveApp) -> Option<&Simulation> {
    let state = &app.turning_workspace;
    if state.workflow != 4 || state.machining.draft_preview {
        return None;
    }
    let (r, id, sim) = state.simulation.report.as_ref()?;
    let selected = app.session.doc.turning.as_ref()?.setups.get(state.setup)?;
    (*r == app.session.revision && id == &selected.id && sim.resolution_mm == state.simulation.resolution).then_some(sim)
}

pub(super) fn current_move(state: &State) -> Option<usize> {
    if state.machining.draft_preview {
        return None;
    }
    state
        .simulation
        .focus_contact
        .and_then(|index| {
            state
                .simulation
                .report
                .as_ref()
                .filter(|(revision, setup, report)| {
                    report.resolution_mm == state.simulation.resolution
                        && state
                            .machining
                            .preview
                            .as_ref()
                            .is_some_and(|(path_revision, path_setup, _)| revision == path_revision && setup == path_setup)
                })
                .and_then(|(_, _, report)| report.contacts.get(index))
        })
        .map(|contact| contact.move_index)
        .or(state.machining.before_move)
        .or_else(|| state.machining.step.checked_sub(1))
}

fn contact_reason(target: &str) -> &'static str {
    tr(match target {
        "remaining_stock" => "cam.sim_stock_hit",
        "insert_rapid_stock" => "cam.insert_rapid_hit",
        "insert_gouge" => "cam.insert_gouge",
        "cutter_rapid_stock" => "cam.cutter_rapid_hit",
        "cutter_gouge" => "cam.cutter_gouge",
        _ => "cam.sim_chuck_hit",
    })
}

pub(super) fn contact_status(ui: &mut egui::Ui, state: &State) {
    let Some(contact) =
        state.simulation.focus_contact.and_then(|index| state.simulation.report.as_ref().and_then(|(_, _, report)| report.contacts.get(index)))
    else {
        return;
    };
    let response = ui.colored_label(
        Tokens::get().error,
        format!("{} #{} · T{:02} · {}", tr("cam.sim_contact_at_move"), contact.move_index + 1, contact.station, contact_reason(&contact.target)),
    );
    crate::scenario::publish_rect("cam_sim_contact_status", response.rect);
    ui.small(format!(
        "{} {} / X{:.3} Z{:.3} mm",
        tr("cam.sim_completed_before_contact"),
        state.machining.step,
        contact.tip.diameter_mm,
        contact.tip.z_mm
    ));
}
