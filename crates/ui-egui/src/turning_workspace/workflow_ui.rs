//! Stage-specific cards using the CAD UI kit. All saved edits go through cam.turning.set.
use super::*;
use cam::{
    drawing::{Ending, Side},
    machining::ProfilePoint,
};

pub(super) fn panels(ui: &mut egui::Ui, plan: &mut Option<TurningPlan>, state: &mut State, revision: u64, bodies: &[String]) {
    let t = Tokens::get();
    let container = ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Panel);
    let frame = container.frame(ui.ctx(), t.panel, crate::frame::Side::Left).inner_margin(12);
    if plan.is_none() {
        container.edge("cam_start", ui_kit::Edge::Left).exact_size(300.).frame(frame).show(ui, |ui| {
            ui.heading("旋削CAM");
            ui.label("モデルから輪郭を取得し、材料と工程を設定します。");
            if button(ui, "cam_create_plan", "旋削CAMを開始") {
                *plan = Some(TurningPlan::nl2500());
                state.workflow = 1;
            }
        });
        return;
    }
    let Some(plan) = plan.as_mut() else { return };
    state.setup = state.setup.min(plan.setups.len().saturating_sub(1));
    let space = ui.available_width();
    let has_right = state.workflow == 2 || state.workflow == 4 || state.segment_editor.is_some();
    let desired_left: f32 = if state.workflow == 2 { 250. } else { 320. };
    let width = desired_left.min((space * if has_right { 0.27 } else { 0.38 }).max(220.));
    container.edge("cam_stage_left", ui_kit::Edge::Left).exact_size(width).resizable(false).frame(frame).show(ui, |ui| {
        let stock_blocked = state.workflow == 0
            && !plan.setups.get(state.setup).is_some_and(|setup| stock_ui::ready(setup, stock_ui::coverage(state, revision, &setup.id)));
        let footer = if matches!(state.workflow, 0 | 1) { 84.0 } else { 48.0 }
            + if stock_blocked { 32.0 } else { 0.0 }
            + if plan.setups.len() > 1 { t.control_height + t.space_8 } else { 0.0 };
        let output = ui
            .scope(|ui| {
                // Keep a visible track when the form continues below the viewport;
                // an idle floating scrollbar hid the material and ending controls.
                ui.style_mut().spacing.scroll.floating = false;
                egui::ScrollArea::vertical()
                    .id_salt(("cam_stage_left", state.workflow))
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .max_height((ui.available_height() - footer).max(100.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match state.workflow {
                            1 => {
                                if let Some(setup) = plan.setups.get(state.setup) {
                                    model_profile::section_settings(ui, &mut state.model_profile, bodies, setup);
                                }
                            }
                            0 => stock_ui::show(ui, plan, state, revision),
                            2 => selection_ui::tools(ui, plan, state),
                            _ => review(ui, plan, state, revision),
                        }
                        if let Some(error) = &state.error {
                            error_label(ui, error);
                        }
                    })
            })
            .inner;
        crate::scenario::publish_rect("cam_stage_left_scroll", output.inner_rect);
        crate::scenario::publish_count("cam_stage_left_overflow", f64::from((output.content_size.y - output.inner_rect.height()).max(0.)));
        ui.separator();
        if state.workflow == 1 {
            let ready = plan.setups.get(state.setup).is_some_and(|s| s.od_job.as_ref().is_some_and(|j| j.profile.len() >= 2));
            if enabled_button(ui, "cam_guide_next", tr("cam.profile_next"), ready, true) {
                guided::go(state, 0);
            }
        } else if state.workflow == 0 {
            stock_ui::next(ui, plan, state, revision);
        }
        context(ui, plan, state);
    });
    if has_right {
        let desired_right: f32 = if state.workflow == 4 { 390. } else { 350. };
        let width = desired_right.min((space * 0.32).max(260.));
        let right_frame = container.frame(ui.ctx(), t.panel, crate::frame::Side::Right).inner_margin(12);
        container.edge("cam_stage_right", ui_kit::Edge::Right).exact_size(width).resizable(false).frame(right_frame).show(ui, |ui| {
            if state.workflow == 4 {
                nc(ui, plan, state, revision);
                return;
            }
            // A new settings target starts at its heading. Candidate calculation
            // does not change this identity, so reviewing a range keeps its scroll.
            let target = settings_target(plan, state);
            let memory_id = ui.id().with("cam_settings_target");
            let changed = ui.ctx().data_mut(|data| {
                let changed = data.get_temp::<egui::Id>(memory_id) != Some(target);
                data.insert_temp(memory_id, target);
                changed
            });
            let mut scroll = egui::ScrollArea::vertical().id_salt(("cam_stage_right", state.workflow));
            if changed || state.operation_draft.as_ref().is_some_and(|draft| draft.fresh) {
                scroll = scroll.vertical_scroll_offset(0.);
            }
            let output = scroll.show(ui, |ui| {
                if state.workflow == 1 {
                    if let Some(setup) = plan.setups.get_mut(state.setup) {
                        segment(ui, setup, state);
                    }
                } else if state.tool_editor.is_some() {
                    tool(ui, plan, state);
                } else {
                    selection_ui::operations(ui, plan, state);
                    if state.operation_draft.is_some() {
                        operations_ui::composer(ui, plan, state);
                    } else if state.selection.editing {
                        selection_ui::settings(ui, plan, state, revision);
                    } else {
                        guided::settings(ui, plan, state, revision, bodies);
                    }
                }
            });
            crate::scenario::publish_rect("cam_stage_right_scroll", output.inner_rect);
            crate::scenario::publish_count("cam_stage_right_offset", f64::from(output.state.offset.y));
        });
    }
}

fn settings_target(plan: &TurningPlan, state: &State) -> egui::Id {
    let setup = plan.setups.get(state.setup);
    let target = if let Some(index) = state.segment_editor {
        format!("segment:{index}")
    } else if let Some(station) = state.tool_editor {
        format!("tool:{station}")
    } else if let Some(draft) = &state.operation_draft {
        format!("draft:{:?}:{}", draft.station, draft.compact)
    } else if state.selection.editing {
        format!("range:{:?}:{:?}", state.selection.station, state.selection.update.as_ref().map(|_| state.operation))
    } else {
        format!("operation:{}", setup.and_then(|setup| setup.operations.get(state.operation)).map_or("", |operation| operation.id.as_str()))
    };
    egui::Id::new((state.workflow, setup.map(|setup| setup.id.as_str()), target))
}

pub(super) fn button(ui: &mut egui::Ui, id: &str, label: &str) -> bool {
    enabled_button(ui, id, label, true, false)
}

pub(super) fn enabled_button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool, primary: bool) -> bool {
    let r = ui.add_enabled_ui(enabled, |ui| ui_kit::button(ui, label, if primary { ui_kit::Button::Primary } else { ui_kit::Button::Normal })).inner;
    crate::scenario::publish_rect(id, r.rect);
    if enabled {
        crate::scenario::publish_handle(id, r.rect.center());
    }
    r.clicked()
}

pub(super) fn context(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let response = ui_kit::button(ui, tr("cam.reset_short"), ui_kit::Button::Small).on_hover_text(tr("cam.reset_help"));
            crate::scenario::publish_rect("cam_reset", response.rect);
            if response.enabled() {
                crate::scenario::publish_handle("cam_reset", response.rect.center());
            }
            if response.clicked() {
                state.reset_requested = true;
            }
            let summary = format!("{} · {}", plan.material, plan.machine);
            ui.add(egui::Label::new(egui::RichText::new(&summary).small()).truncate()).on_hover_text(&summary);
        });
    });
    if plan.setups.len() > 1 {
        controls::dropdown(ui, "cam_setup_choice", plan.setups.get(state.setup).map_or("段取り", |s| s.name.as_str()).to_owned(), |ui| {
            for (i, s) in plan.setups.iter().enumerate() {
                if controls::value(ui, &mut state.setup, i, &s.name).changed() {
                    state.selection = selection_ui::View::default();
                    state.segment_editor = None;
                    state.tool_editor = None;
                    state.model_profile = model_profile::View::default();
                    state.spatial.playing = false;
                    if state.catalog.preview.take().is_some() {
                        state.catalog.fit = true;
                    }
                }
            }
        });
    }
}

pub(super) fn reset(plan: &mut TurningPlan, state: &mut State) {
    if let Some(first) = plan.setups.first_mut() {
        first.operations.clear();
        first.stock = None;
        first.od_job = None;
        first.id_job = None;
        first.drawing = None;
        first.cut_end_z_mm = None;
        first.previous_setup = None;
        first.ending = Ending::Clamped;
    }
    plan.setups.truncate(1);
    *state = State::default();
    state.model_profile.suppress_auto = true;
}

pub(super) fn ending(ui: &mut egui::Ui, setup: &mut Setup) {
    ui.strong(tr("cam.ending_title"));
    let options = [
        (Ending::Clamped, "cam.ending_clamped"),
        (Ending::Reverse, "cam.ending_reverse"),
        (Ending::PartingFinish, "cam.ending_manual"),
        (Ending::PartOff, "cam.ending_partoff"),
    ];
    let selected = options.iter().find(|(value, _)| *value == setup.ending).map_or("cam.ending_clamped", |(_, key)| *key);
    let response = controls::dropdown(ui, "cam_ending_choice", tr(selected).into(), |ui| {
        for (value, label) in options {
            let r = controls::value(ui, &mut setup.ending, value, tr(label));
            crate::scenario::publish_rect(&format!("cam_ending_{value:?}"), r.rect);
        }
    });
    crate::scenario::publish_rect("cam_ending_choice", response.response.rect);
    if setup.ending != Ending::Clamped {
        ui.colored_label(Tokens::get().warning, tr("cam.ending_nc_blocked"));
    }
}

pub(super) fn segment(ui: &mut egui::Ui, setup: &mut Setup, state: &mut State) {
    let Some(index) = state.segment_editor else { return };
    if setup.drawing.is_none() {
        setup.drawing = Some(super::flat::drawing(setup));
    }
    let Some(drawing) = &mut setup.drawing else { return };
    let Some(s) = drawing.segments.get_mut(index) else {
        state.segment_editor = None;
        return;
    };
    ui.heading("線の情報");
    ui.label(match s.side {
        Side::Outer => "外径",
        Side::Inner => "内径 / 中心軸",
        Side::Front => "正面端面",
        Side::Back => "背面端面",
    });
    let selectable = s.side == Side::Outer || (s.side == Side::Inner && setup.id_job.is_some());
    ui.monospace(format!("Z {:.3} / Φ {:.3}\nZ {:.3} / Φ {:.3}", s.a.z_mm, s.a.diameter_mm, s.b.z_mm, s.b.diameter_mm));
    ui.separator();
    ui.strong("寸法公差（mm）");
    ui.horizontal(|ui| {
        ui.label("下限差");
        ui_kit::number_field(ui, &mut s.lower_mm, -100.0..=s.upper_mm, 0.001, "");
    });
    ui.horizontal(|ui| {
        ui.label("上限差");
        ui_kit::number_field(ui, &mut s.upper_mm, s.lower_mm..=100., 0.001, "");
    });
    ui.small("寸法の指示値として保存。経路はモデルの呼び寸法で作ります。");
    let mut thread = s.thread_pitch_mm.is_some();
    if ui_kit::checkbox(ui, "ねじのある線", &mut thread).changed() {
        s.thread_pitch_mm = thread.then_some(1.);
    }
    if let Some(pitch) = &mut s.thread_pitch_mm {
        ui.horizontal(|ui| {
            ui.label("ピッチ");
            ui_kit::number_field(ui, pitch, 0.01..=100., 0.05, " mm");
        });
        ui.colored_label(Tokens::get().warning, "ねじ経路は未対応です。この指定がある間はNC出力を停止します。");
    }
    let can_split = drawing.segments.len() < 1024;
    if can_split && button(ui, "cam_segment_split", "線を中点で分割") {
        split(setup, index);
    }
    ui.small(if selectable {
        "分割後は③でその点を加工範囲の端として選べます。既存工程は範囲を再確認してください。"
    } else {
        "分割を輪郭表示へ保存します。端面・中心軸の点から加工範囲を指定する機能は未対応です。"
    });
    if button(ui, "cam_segment_close", "閉じる") {
        state.segment_editor = None;
    }
}

pub(super) fn split(setup: &mut Setup, index: usize) {
    let Some(d) = &mut setup.drawing else { return };
    if d.segments.len() >= 1024 {
        return;
    }
    let Some(segment) = d.segments.get(index).cloned() else { return };
    let mid = ProfilePoint { z_mm: (segment.a.z_mm + segment.b.z_mm) * 0.5, diameter_mm: (segment.a.diameter_mm + segment.b.diameter_mm) * 0.5 };
    let profile = match segment.side {
        Side::Outer => setup.od_job.as_mut(),
        Side::Inner => setup.id_job.as_mut(),
        _ => None,
    };
    if let Some(job) = profile {
        if job.profile.len() >= 256 {
            return;
        }
        let at = job.profile.windows(2).position(|p| matches!(p,[a,b] if (*a==segment.a&&*b==segment.b)||(*a==segment.b&&*b==segment.a)));
        if let Some(at) = at {
            job.profile.insert(at + 1, mid);
        } else {
            return;
        }
    }
    if let Some(s) = d.segments.get_mut(index) {
        s.b = mid;
    }
    d.segments.insert(index + 1, cam::drawing::Segment { a: mid, ..segment });
}

pub(super) fn tool(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State) {
    let Some(station) = state.tool_editor else { return };
    ui.heading(format!("T{station:02} 工具を編集"));
    if button(ui, "cam_tool_edit_close", "工程の設定に戻る") {
        state.tool_editor = None;
        return;
    }
    let Some(setup) = plan.setups.get_mut(state.setup) else { return };
    if let Some(slot) = setup.turret.iter_mut().find(|s| s.station == station) {
        choice(ui, "cam_tool_edit_role", &mut slot.role, ROLES);
        let mut name = slot.tool_name.clone().unwrap_or_default();
        if ui_kit::bounded_cell_editor(ui, ui.next_auto_id(), &mut name, tr("cam.tool_name"), false, 64).changed() {
            slot.tool_name = (!name.is_empty()).then_some(name);
        }
        let mut offset = slot.offset_number.unwrap_or(0);
        ui.horizontal(|ui| {
            ui.label("補正番号");
            if ui_kit::number_field(ui, &mut offset, 0..=u16::MAX, 1., "").changed() {
                slot.offset_number = (offset > 0).then_some(offset);
            }
        });
    }
    simulation_ui::tool_settings(ui, setup, state, &plan.tool_cad_library);
    // Catalogue and dimension selectors must keep the editor heading/name/offset on the same tool.
    state.tool_editor = Some(state.simulation.selected_station);
}

pub(super) fn review(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State, revision: u64) {
    selection_ui::operations(ui, plan, state);
    let Some(setup) = plan.setups.get(state.setup) else { return };
    simulation_ui::report_settings(ui, setup, state, revision);
    if button(ui, "cam_review_edit", "③ 工程を編集") {
        guided::go(state, 2);
    }
}

pub(super) fn nc(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State, revision: u64) {
    ui.heading("Gコード");
    let Some(setup) = plan.setups.get_mut(state.setup) else { return };
    if button(ui, "cam_path_nc_view", "現在の工程からNCを表示") {
        state.machining.action = Some(machining_ui::Action::InspectNc(setup.id.clone(), revision));
    }
    if button(ui, "cam_path_export", "NCファイルを保存") {
        state.machining.action = Some(machining_ui::Action::Export(setup.id.clone(), revision));
    }
    let settings = controls::section(ui, "出力設定", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("cam_nc_settings_scroll")
            .max_height((ui.available_height() * 0.4).clamp(80., 300.))
            .show(ui, |ui| post_ui::show(ui, setup, &mut state.operation));
    });
    crate::scenario::publish_rect("cam_nc_settings", settings.header_rect);
    if let Some(e) = &state.machining.error {
        error_label(ui, e);
    }
    let current_move = simulation_ui::current_move(state);
    if let Some(index) = machining_ui::nc_panel(ui, &mut state.machining, revision, &setup.id, current_move) {
        state.machining.step = index;
        state.machining.before_move = Some(index);
        state.simulation.focus_contact = None;
        state.spatial.playing = false;
    }
}
