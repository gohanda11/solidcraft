//! Draft-first operation creation. Browsing and cancelling never change the document.
use super::*;

#[derive(Clone, Debug)]
pub(super) struct Draft {
    setup: String,
    group: usize,
    kind: Option<OperationKind>,
    name: String,
    station: Option<u8>,
    before: Option<String>,
    pub(super) bore: f64,
    error: Option<String>,
}

const GROUPS: &[(&str, &[OperationKind])] = &[
    ("cam.op_group_face", &[OperationKind::FaceRough, OperationKind::FaceFinish]),
    ("cam.profile_od", &[OperationKind::OdRough, OperationKind::OdFinish]),
    ("cam.profile_id", &[OperationKind::IdRough, OperationKind::IdFinish]),
    ("cam.op_group_hole", &[OperationKind::CenterDrill, OperationKind::Drill]),
    (
        "cam.op_group_groove",
        &[
            OperationKind::OdGrooveRough,
            OperationKind::OdGrooveFinish,
            OperationKind::IdGroove,
            OperationKind::FaceGroove,
            OperationKind::OdThread,
            OperationKind::IdThread,
        ],
    ),
    ("cam.op_group_part", &[OperationKind::PartOff, OperationKind::PartingFinish]),
];

pub(super) fn button(ui: &mut egui::Ui, id: &str, key: &str, enabled: bool) -> bool {
    let primary = matches!(id, "cam_add_operation" | "cam_operation_confirm");
    let r = ui
        .add_enabled_ui(enabled, |ui| {
            let ink = if primary { Tokens::get().accent } else { Tokens::get().text };
            ui_kit::button(ui, egui::RichText::new(tr(key)).color(ink), ui_kit::Button::Normal)
        })
        .inner;
    crate::scenario::publish_rect(id, r.rect);
    if enabled {
        crate::scenario::publish_handle(id, r.rect.center());
    }
    r.clicked()
}

pub(super) fn open(plan: &TurningPlan, state: &mut State) {
    let Some(setup) = plan.setups.get(state.setup) else { return };
    let draft = Draft { setup: setup.id.clone(), group: 0, kind: None, name: String::new(), station: None, before: None, bore: 0., error: None };
    guided::go(state, 2);
    state.operation_draft = Some(draft);
    state.error = None;
}

pub(super) fn select_tool(draft: &mut Draft, setup: &Setup, station: u8) {
    let role = setup.turret.iter().find(|s| s.station == station).map(|s| s.role);
    let kind = match role {
        Some(ToolRole::CenterDrill) => OperationKind::CenterDrill,
        Some(ToolRole::Drill) => OperationKind::Drill,
        Some(ToolRole::OdGrooveRough) => OperationKind::OdGrooveRough,
        _ => OperationKind::OdGrooveFinish,
    };
    draft.kind = Some(kind);
    draft.station = Some(station);
    draft.name = tr(kind_label(kind)).into();
    draft.group = if matches!(kind, OperationKind::CenterDrill | OperationKind::Drill) { 3 } else { 4 };
}

fn rank(kind: OperationKind) -> u8 {
    use OperationKind::*;
    match kind {
        CenterDrill => 2,
        Drill => 3,
        Face | FaceRough => 0,
        OdRough => 1,
        IdRough => 4,
        OdGrooveRough | OdGroove | IdGroove | FaceGroove | OdThread | IdThread => 5,
        FaceFinish => 6,
        OdFinish => 7,
        IdFinish => 8,
        OdGrooveFinish => 9,
        PartOff | PartingFinish => 10,
    }
}

pub(super) fn supported(kind: OperationKind) -> bool {
    cam::machining::sequence::local(kind)
        || matches!(
            kind,
            OperationKind::FaceRough
                | OperationKind::FaceFinish
                | OperationKind::OdRough
                | OperationKind::OdFinish
                | OperationKind::IdRough
                | OperationKind::IdFinish
        )
}

fn insertion(setup: &Setup, draft: &Draft, kind: OperationKind) -> usize {
    if matches!(kind, OperationKind::PartOff | OperationKind::PartingFinish) {
        return setup.operations.len();
    }
    let last = setup
        .operations
        .iter()
        .position(|op| matches!(op.kind, OperationKind::PartOff | OperationKind::PartingFinish))
        .unwrap_or(setup.operations.len());
    match &draft.before {
        Some(id) => setup.operations.iter().position(|o| &o.id == id).unwrap_or(last).min(last),
        None => setup.operations.iter().position(|o| rank(o.kind) > rank(kind)).unwrap_or(last).min(last),
    }
}

fn station_label(setup: &Setup, n: Option<u8>) -> String {
    n.and_then(|n| setup.turret.iter().find(|s| s.station == n))
        .map(|s| {
            let role = ROLES.iter().find(|(r, _)| *r == s.role).map(|(_, key)| tr(key)).unwrap_or("");
            format!("T{:02} · {}", s.station, s.tool_name.as_deref().unwrap_or(role))
        })
        .unwrap_or_else(|| tr("cam.unassigned").into())
}

fn station_picker(ui: &mut egui::Ui, setup: &Setup, station: &mut Option<u8>, id: &str) {
    let r = controls::dropdown(ui, id, (station_label(setup, *station)).to_string(), |ui| {
        controls::value(ui, station, None, tr("cam.unassigned"));
        for slot in setup.turret.iter().filter(|s| s.role != ToolRole::Vacant) {
            let r = controls::value(ui, station, Some(slot.station), station_label(setup, Some(slot.station)));
            crate::scenario::publish_rect(&format!("{id}_{}", slot.station), r.rect);
        }
    });
    crate::scenario::publish_rect(id, r.response.rect);
}

pub(super) fn composer(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State) {
    let Some(mut draft) = state.operation_draft.take() else { return };
    let Some(setup) = plan.setups.get(state.setup).filter(|s| s.id == draft.setup) else { return };
    ui.heading(tr("cam.operation_create_title"));
    ui.label(&setup.name);
    ui.add_space(8.);
    ui.strong(tr("cam.operation_choose_kind"));
    egui::Grid::new("cam_operation_groups").num_columns(3).spacing([6., 6.]).show(ui, |ui| {
        for (i, (key, _)) in GROUPS.iter().enumerate() {
            let r = ui
                .allocate_ui_with_layout(egui::Vec2::from([96., 32.]), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    controls::pick(ui, draft.group == i, tr(key))
                })
                .inner;
            crate::scenario::publish_rect(&format!("cam_operation_group_{i}"), r.rect);
            if r.clicked() && draft.group != i {
                draft.group = i;
                draft.kind = None;
                draft.name.clear();
                draft.station = None;
                draft.before = None;
                draft.error = None;
            }
            if i % 3 == 2 {
                ui.end_row();
            }
        }
    });
    ui.add_space(6.);
    if let Some((_, kinds)) = GROUPS.get(draft.group) {
        egui::Grid::new("cam_operation_kinds").num_columns(2).spacing([6., 6.]).show(ui, |ui| {
            for (i, kind) in kinds.iter().enumerate() {
                let r = ui
                    .allocate_ui_with_layout(egui::Vec2::from([150., 32.]), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        controls::pick(ui, draft.kind == Some(*kind), tr(kind_label(*kind)))
                    })
                    .inner;
                crate::scenario::publish_rect(&format!("cam_operation_kind_{kind:?}"), r.rect);
                if r.clicked() {
                    draft.kind = Some(*kind);
                    draft.name = tr(kind_label(*kind)).into();
                    draft.station = kind.suggested_station().filter(|n| setup.turret.iter().any(|s| s.station == *n && s.role != ToolRole::Vacant));
                    draft.error = None;
                    state.model_profile.inner =
                        matches!(kind, OperationKind::IdRough | OperationKind::IdFinish | OperationKind::IdGroove | OperationKind::IdThread);
                    state.spatial.section = state.model_profile.inner;
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    }
    ui.add_space(8.);
    if let Some(kind) = draft.kind {
        ui.strong(format!(
            "{} · {}",
            tr(kind_label(kind)),
            tr(if supported(kind) { "cam.operation_path_supported" } else { "cam.operation_plan_only" })
        ));
        if !supported(kind) {
            ui.add(egui::Label::new(tr("cam.operation_plan_only_help")).wrap());
        }
        if !cam::machining::sequence::local(kind) && setup.operations.iter().any(|o| o.kind == kind) {
            ui.colored_label(Tokens::get().warning, tr("cam.operation_duplicate_help"));
        }
        ui.separator();
        ui.strong(tr("cam.operation_choose_tool"));
        station_picker(ui, setup, &mut draft.station, "cam_operation_draft_station");
        ui.small(tr("cam.operation_tool_help"));
        if kind == OperationKind::PartingFinish {
            number(ui, "cam.finished_bore", &mut draft.bore);
            ui.add(egui::Label::new(tr("cam.bore_help")).wrap());
        }
        ui.separator();
        ui.strong(tr("cam.operation_confirm_details"));
        text(ui, "cam.operation_name", &mut draft.name);
        let at = insertion(setup, &draft, kind);
        ui.label(format!("{}: {:02}", tr("cam.operation_position"), at + 1));
        if !matches!(kind, OperationKind::PartOff | OperationKind::PartingFinish) {
            let selected = match &draft.before {
                None => tr("cam.operation_auto_order").to_string(),
                Some(id) => setup
                    .operations
                    .iter()
                    .find(|o| &o.id == id)
                    .map(|o| format!("{}: {}", tr("cam.operation_before"), o.name))
                    .unwrap_or_else(|| tr("cam.operation_end").into()),
            };
            controls::dropdown(ui, "cam_operation_position", (selected).to_string(), |ui| {
                controls::value(ui, &mut draft.before, None, tr("cam.operation_auto_order"));
                for op in &setup.operations {
                    controls::value(ui, &mut draft.before, Some(op.id.clone()), format!("{}: {}", tr("cam.operation_before"), op.name));
                }
                controls::value(ui, &mut draft.before, Some(String::new()), tr("cam.operation_end"));
            });
        } else {
            ui.small(tr("cam.operation_separation_last"));
        }
        if let Some(next) = setup.operations.get(at) {
            ui.small(format!("{}: {}", tr("cam.operation_before"), next.name));
        }
        ui.small(tr("cam.operation_after_add"));
    } else {
        ui.label(tr("cam.operation_choose_help"));
    }
    let issue = if draft.name.trim().is_empty() {
        Some("cam.operation_need_name")
    } else if draft.kind == Some(OperationKind::PartingFinish)
        && (!draft.bore.is_finite() || draft.bore <= 0. || setup.stock.as_ref().is_some_and(|s| draft.bore >= s.outer_diameter_mm))
    {
        Some("cam.operation_need_bore")
    } else if draft.kind.is_some_and(|k| matches!(k, OperationKind::PartOff | OperationKind::PartingFinish))
        && setup.planned_outcome() != PlannedOutcome::RemainsClamped
    {
        Some("cam.operation_has_separation")
    } else if setup.operations.len() >= 256 {
        Some("cam.operation_limit")
    } else {
        None
    };
    if draft.kind.is_some()
        && let Some(key) = issue
    {
        ui.colored_label(Tokens::get().warning, tr(key));
    }
    if let Some(error) = &draft.error {
        ui.colored_label(Tokens::get().error, error);
    }
    ui.add_space(12.);
    let mut cancel = false;
    let mut confirm = false;
    ui.horizontal(|ui| {
        confirm = button(ui, "cam_operation_confirm", "cam.operation_add_continue", draft.kind.is_some() && issue.is_none());
        cancel = button(ui, "cam_operation_cancel", "ui.cancel", true);
    });
    if cancel {
        return;
    }
    if confirm && let Some(kind) = draft.kind {
        let mut candidate = plan.clone();
        let id = next_id(&candidate, "operation");
        if let Some(setup) = candidate.setups.get_mut(state.setup) {
            let at = insertion(setup, &draft, kind);
            setup.operations.insert(
                at,
                Operation {
                    cutting: default_cutting(kind),
                    id,
                    name: draft.name.trim().into(),
                    kind,
                    station: draft.station,
                    manual_separation: (kind == OperationKind::PartingFinish)
                        .then_some(ManualSeparation { finished_bore_diameter_mm: draft.bore, stop_diameter_mm: draft.bore }),
                },
            );
            match candidate.validate() {
                Ok(()) => {
                    *plan = candidate;
                    state.operation = at;
                    state.section = 2;
                    state.model_profile.inner = matches!(kind, OperationKind::IdRough | OperationKind::IdFinish);
                    state.spatial.section = state.model_profile.inner;
                    state.spatial.operation = Some(kind);
                    return;
                }
                Err(error) => draft.error = Some(error.to_string()),
            }
        }
    }
    state.operation_draft = Some(draft);
}

/// Common visible controls for the selected operation in the guided flow.
pub(super) fn selected(ui: &mut egui::Ui, setup: &mut Setup, state: &mut State) -> bool {
    let Some(op) = setup.operations.get(state.operation) else {
        ui.label(tr("cam.no_operations"));
        return false;
    };
    let kind = op.kind;
    let separates = |kind| matches!(kind, OperationKind::PartOff | OperationKind::PartingFinish);
    let can_up = state.operation > 0 && !separates(kind);
    let can_down = setup.operations.get(state.operation + 1).is_some_and(|o| !separates(o.kind)) && !separates(kind);
    ui.strong(format!("{:02}  {}", state.operation + 1, op.name));
    ui.small(tr(if cam::machining::sequence::local(kind) {
        "cam.local_conditions_help"
    } else if supported(kind) {
        "cam.operation_shared_conditions"
    } else {
        "cam.operation_plan_only_help"
    }));
    let mut station = op.station;
    station_picker(ui, setup, &mut station, "cam_guided_station");
    if let Some(op) = setup.operations.get_mut(state.operation) {
        op.station = station;
        text(ui, "cam.operation_name", &mut op.name);
        if let Some(manual) = &mut op.manual_separation {
            number(ui, "cam.finished_bore", &mut manual.finished_bore_diameter_mm);
            number(ui, "cam.stop_diameter", &mut manual.stop_diameter_mm);
            ui.add(egui::Label::new(tr("cam.manual_help")).wrap());
        }
    }
    let mut removed = false;
    ui.horizontal(|ui| {
        if button(ui, "cam_operation_up", "cam.move_up", can_up) {
            setup.operations.swap(state.operation, state.operation - 1);
            state.operation -= 1;
        }
        if button(ui, "cam_operation_down", "cam.move_down", can_down) {
            setup.operations.swap(state.operation, state.operation + 1);
            state.operation += 1;
        }
        if button(ui, "cam_remove_operation", "cam.remove_operation", true) {
            setup.operations.remove(state.operation);
            state.operation = state.operation.saturating_sub(1);
            if let Some(next) = setup.operations.get(state.operation) {
                state.model_profile.inner = matches!(next.kind, OperationKind::IdRough | OperationKind::IdFinish);
                state.spatial.section = state.model_profile.inner;
                state.spatial.operation = Some(next.kind);
            }
            removed = true;
        }
    });
    ui.separator();
    if !removed
        && (cam::machining::sequence::local(kind)
            || setup.operations.get(state.operation).is_some_and(|o| matches!(o.cutting, Some(cam::machining::sequence::Cutting::Contour(_)))))
    {
        local_settings(ui, setup, state.operation);
        return false;
    }
    !removed && supported(kind)
}

fn default_cutting(kind: OperationKind) -> Option<cam::machining::sequence::Cutting> {
    use cam::machining::sequence::{Cutting, Drill, Groove};
    match kind {
        OperationKind::CenterDrill | OperationKind::Drill => Some(Cutting::Drill(Drill::default())),
        OperationKind::OdGrooveRough | OperationKind::OdGrooveFinish => Some(Cutting::OdGroove(Groove::default())),
        _ => None,
    }
}

fn local_number(ui: &mut egui::Ui, key: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, unit: &str) {
    ui.label(tr(key));
    let response = ui_kit::number_field(ui, value, range, 0.01, unit);
    crate::scenario::publish_rect(key, response.rect);
}

fn local_settings(ui: &mut egui::Ui, setup: &mut Setup, index: usize) {
    use cam::machining::sequence::Cutting;
    let Some(op) = setup.operations.get(index) else { return };
    let kind = op.kind;
    if kind == OperationKind::OdGrooveFinish {
        let candidates: Vec<_> = setup
            .operations
            .iter()
            .take(index)
            .filter(|o| o.kind == OperationKind::OdGrooveRough)
            .filter_map(|o| o.cutting.clone().map(|c| (o.name.clone(), c)))
            .collect();
        ui.label(tr("cam.groove_copy_help"));
        for (i, (name, mut cutting)) in candidates.into_iter().enumerate() {
            let r = ui_kit::button(ui, format!("{}: {}", tr("cam.groove_copy"), name), ui_kit::Button::Normal);
            crate::scenario::publish_rect(&format!("cam_groove_copy_{i}"), r.rect);
            if r.clicked()
                && let Some(op) = setup.operations.get_mut(index)
            {
                if let (Some(Cutting::OdGroove(current)), Cutting::OdGroove(copied)) = (&op.cutting, &mut cutting) {
                    copied.feed_mm_rev = current.feed_mm_rev;
                    if current.tool_width_mm > 0. {
                        copied.tool_width_mm = current.tool_width_mm;
                    }
                }
                op.cutting = Some(cutting);
            }
        }
    }
    let Some(op) = setup.operations.get_mut(index) else { return };
    if op.cutting.is_none() && button(ui, "cam_local_configure", "cam.local_configure", true) {
        op.cutting = default_cutting(kind);
    }
    let Some(cutting) = op.cutting.as_mut() else { return };
    ui.strong(tr("cam.local_geometry"));
    match cutting {
        Cutting::Contour(c) => {
            ui.label(format!("輪郭点 {} → {}", c.first + 1, c.last + 1));
            ui.small(format!("残す取り代：片側 {:.3} / Z {:.3} mm", c.radial_leave_mm, c.axial_leave_mm));
            ui.small("範囲や荒／仕上げを変える場合は、左の工具から新しい工程を作成します。");
            local_number(ui, "cam.path_depth", &mut c.depth_mm, 0.0..=100., " mm");
            local_number(ui, "cam.local_feed", &mut c.feed_mm_rev, 0.0..=10., " mm/rev");
        }
        Cutting::Drill(d) => {
            local_number(ui, "cam.drill_diameter", &mut d.diameter_mm, 0.0..=1000., " mm");
            local_number(ui, "cam.drill_tip_z", &mut d.tip_end_z_mm, -1e6..=0., " mm");
            let mut degrees = d.point_angle_rad.to_degrees();
            local_number(ui, "cam.drill_point_angle", &mut degrees, 30.0..=170., " °");
            d.point_angle_rad = degrees.to_radians();
            ui.small(format!("{}: Z{:.3}", tr("cam.drill_full_z"), d.full_diameter_z()));
            ui.small(tr("cam.drill_point_help"));
            local_number(ui, "cam.drill_peck", &mut d.peck_mm, 0.0..=1000., " mm");
            local_number(ui, "cam.local_feed", &mut d.feed_mm_rev, 0.0..=10., " mm/rev");
        }
        Cutting::OdGroove(g) => {
            local_number(ui, "cam.groove_front_z", &mut g.front_z_mm, -1e6..=0., " mm");
            local_number(ui, "cam.groove_width", &mut g.width_mm, 0.0..=1e6, " mm");
            local_number(ui, "cam.groove_bottom", &mut g.bottom_diameter_mm, 0.0..=1e6, " mm");
            local_number(ui, "cam.groove_tool_width", &mut g.tool_width_mm, 0.0..=1e6, " mm");
            ui.small(tr("cam.groove_reference_help"));
            controls::section(ui, tr("cam.path_allowances"), |ui| {
                local_number(ui, "cam.path_radial_leave", &mut g.radial_leave_mm, 0.0..=100., " mm");
                local_number(ui, "cam.groove_wall_leave", &mut g.axial_leave_mm, 0.0..=100., " mm");
                ui.small(format!(
                    "{}: Φ{:.3} / Z{:.3} … Z{:.3}",
                    tr("cam.groove_rough_limits"),
                    g.bottom_diameter_mm + 2. * g.radial_leave_mm,
                    g.front_z_mm - g.axial_leave_mm,
                    g.front_z_mm - g.width_mm + g.axial_leave_mm
                ));
            });
            local_number(ui, "cam.path_depth", &mut g.radial_depth_mm, 0.0..=100., " mm");
            local_number(ui, "cam.local_feed", &mut g.feed_mm_rev, 0.0..=10., " mm/rev");
        }
    }
    if setup.od_job.is_some() || setup.id_job.is_some() {
        ui.small(tr("cam.local_shared_limits"));
    } else {
        let l = match cutting {
            Cutting::Contour(c) => &mut c.limits,
            Cutting::Drill(d) => &mut d.limits,
            Cutting::OdGroove(g) => &mut g.limits,
        };
        controls::section(ui, tr("cam.path_limits"), |ui| {
            local_number(ui, "cam.path_front_stock", &mut l.stock_front_z_mm, 0.0..=1e6, " mm");
            local_number(ui, "cam.path_chuck", &mut l.chuck_front_z_mm, -1e6..=0., " mm");
            local_number(ui, "cam.path_clearance", &mut l.clearance_mm, 0.0..=1000., " mm");
        });
    }
    ui.small(tr("cam.local_nominal_help"));
}
