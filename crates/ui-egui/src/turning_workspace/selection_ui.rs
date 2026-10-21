//! Tool-first contour picking. Only explicit preview/add actions invoke planning.
use super::*;
use cam::machining::{
    selection::{Contour, Proposal, Residual},
    sequence::Limits,
};

#[derive(Clone, Debug)]
pub(super) struct View {
    pub station: Option<u8>,
    pub first: Option<usize>,
    pub last: Option<usize>,
    pub picking: bool,
    pub editing: bool,
    radial: f64,
    axial: f64,
    depth: f64,
    feed: f64,
    request: Option<serde_json::Value>,
    pub preview: Option<(u64, String, Proposal)>,
    pub difference: Option<(u64, String, Residual)>,
    pub show_difference: bool,
    action: Option<&'static str>,
    error: Option<String>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            station: None,
            first: None,
            last: None,
            picking: false,
            editing: false,
            radial: 0.1,
            axial: 0.1,
            depth: 0.,
            feed: 0.,
            request: None,
            preview: None,
            difference: None,
            show_difference: true,
            action: None,
            error: None,
        }
    }
}
fn button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let r = ui.add_enabled_ui(enabled, |ui| ui_kit::button(ui, label, ui_kit::Button::Normal)).inner;
    crate::scenario::publish_rect(id, r.rect);
    if enabled {
        crate::scenario::publish_handle(id, r.rect.center());
    }
    r.clicked()
}
fn number(ui: &mut egui::Ui, id: &str, label: &str, n: &mut f64, max: f64) {
    ui.horizontal(|ui| {
        let r = ui_kit::number_field(ui, n, 0.0..=max, 0.01, "");
        crate::scenario::publish_rect(id, r.rect);
        ui.label(label);
    });
}
pub(super) fn tools(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State) {
    ui.heading("工具から加工を作る");
    ui.small(format!("{} · {}", plan.machine, plan.material));
    if plan.setups.len() > 1 {
        controls::dropdown(ui, "cam_setup_choice", (plan.setups.get(state.setup).map_or("段取り", |s| s.name.as_str())).to_string(), |ui| {
            for (i, s) in plan.setups.iter().enumerate() {
                if controls::value(ui, &mut state.setup, i, &s.name).changed() {
                    state.selection = View::default();
                }
            }
        });
    }
    ui.separator();
    ui.small("工具 → 輪郭の始点・終点 → 経路確認 → 追加");
    let Some(setup) = plan.setups.get(state.setup) else { return };
    for slot in &setup.turret {
        let role = ROLES.iter().find(|(r, _)| *r == slot.role).map_or("", |(_, k)| tr(k));
        let label = format!("T{:02}  {}", slot.station, role);
        let r = ui
            .add_enabled_ui(slot.role != ToolRole::Vacant, |ui| {
                controls::pick(ui, state.selection.station == Some(slot.station) && state.selection.editing, label)
            })
            .inner;
        crate::scenario::publish_rect(&format!("cam_tool_{}", slot.station), r.rect);
        if slot.role != ToolRole::Vacant {
            crate::scenario::publish_handle(&format!("cam_tool_{}", slot.station), r.rect.center());
        }
        if r.clicked() {
            state.selection = View { station: Some(slot.station), editing: true, picking: true, ..View::default() };
            guided::go(state, 2);
            state.model_profile.inner = matches!(slot.role, ToolRole::IdRough | ToolRole::IdFinish);
            state.spatial.section = state.model_profile.inner;
            state.spatial.stage = 0;
            state.spatial.operation = None;
            state.machining.show = false;
            let job = if state.model_profile.inner { &setup.id_job } else { &setup.od_job };
            if let Some(j) = job {
                state.selection.depth = j.radial_depth_mm;
                state.selection.feed = j.rough_feed_mm_rev;
            }
            if matches!(slot.role, ToolRole::OdFinish | ToolRole::IdFinish) {
                state.selection.radial = 0.;
                state.selection.axial = 0.;
                if let Some(j) = job {
                    state.selection.feed = j.finish_feed_mm_rev;
                    state.selection.depth = j.radial_depth_mm.min(0.3);
                }
            }
            if !matches!(slot.role, ToolRole::OdRough | ToolRole::OdFinish | ToolRole::IdRough | ToolRole::IdFinish) {
                state.selection.picking = false;
            }
            state.selection.action = Some("difference");
        }
    }
    ui.separator();
    if button(ui, "cam_tool_library_open", "工具・ホルダー設定", true) {
        state.advanced = true;
        state.section = 1;
        state.selection.editing = false;
        state.selection.picking = false;
    }
    if button(ui, "cam_advanced", "段取りなどの詳細設定", true) {
        state.advanced = true;
        state.section = 0;
        state.selection.editing = false;
        state.selection.picking = false;
    }
    ui.small("外径・内径：輪郭点から範囲指定\n穴・溝・端面：右側の寸法設定");
}
pub(super) fn operations(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State) {
    ui.heading("加工工程");
    let Some(setup) = plan.setups.get(state.setup) else { return };
    egui::ScrollArea::vertical().id_salt("cam_right_operations").max_height(205.).show(ui, |ui| {
        for (i, op) in setup.operations.iter().enumerate() {
            let station = op.station.map_or("—".into(), |n| format!("T{n:02}"));
            let r = ui
                .allocate_ui_with_layout(egui::Vec2::from([ui.available_width(), 29.]), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    controls::pick(
                        ui,
                        state.operation == i && !state.selection.editing && state.workflow == 2,
                        format!("{:02}  {}  {}", i + 1, station, op.name),
                    )
                })
                .inner;
            crate::scenario::publish_rect(&format!("cam_operation_{i}"), r.rect);
            if r.clicked() {
                guided::go(state, 2);
                state.operation = i;
                state.section = 2;
                state.selection.editing = false;
                state.selection.picking = false;
                state.model_profile.inner = matches!(op.kind, OperationKind::IdRough | OperationKind::IdFinish);
                state.spatial.section = state.model_profile.inner;
                state.spatial.operation = Some(op.kind);
                state.machining.show = true;
            }
        }
        if setup.operations.is_empty() {
            ui.small("左の工具を選んで、最初の工程を作成します。");
        }
    });
    if button(ui, "cam_add_operation", "穴・溝・端面などを追加", true) {
        state.selection.editing = false;
        state.selection.picking = false;
        operations_ui::open(plan, state);
    }
    ui.separator();
}
pub(super) fn settings(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State, revision: u64) {
    let Some(setup) = plan.setups.get(state.setup) else { return };
    let station = state.selection.station.unwrap_or(0);
    let turning = setup
        .turret
        .iter()
        .find(|s| s.station == station)
        .is_some_and(|s| matches!(s.role, ToolRole::OdRough | ToolRole::OdFinish | ToolRole::IdRough | ToolRole::IdFinish));
    ui.strong(format!("T{station:02}  新しい加工範囲"));
    if !turning {
        ui.label("この工具は穴・溝・端面などの寸法を指定して追加します。");
        if button(ui, "cam_tool_local", "この工具で工程を作る", true) {
            operations_ui::open(plan, state);
            if let Some(draft) = state.operation_draft.as_mut() {
                operations_ui::select_tool(draft, setup, station);
            }
            state.selection.editing = false;
        }
        return;
    }
    let job = if state.model_profile.inner { &setup.id_job } else { &setup.od_job };
    let Some(job) = job.as_ref() else {
        ui.label("先に①でこの側の輪郭をモデルから取得してください。");
        if button(ui, "cam_selection_model", "① モデル・輪郭へ", true) {
            guided::go(state, 1);
        }
        return;
    };
    let view = &mut state.selection;
    ui.label(if view.first.is_none() {
        "中央の輪郭で始点をクリック"
    } else if view.last.is_none() {
        "中央の輪郭で終点をクリック"
    } else {
        "選択した範囲の経路を確認します"
    });
    for (label, index) in [("始点", view.first), ("終点", view.last)] {
        ui.monospace(
            index
                .and_then(|i| job.profile.get(i))
                .map_or(format!("{label}：未選択"), |p| format!("{label}：Z {:.3} / Φ {:.3}", p.z_mm, p.diameter_mm)),
        );
    }
    ui.horizontal(|ui| {
        if button(ui, "cam_select_all", "輪郭全体", job.profile.len() >= 2) {
            view.first = Some(0);
            view.last = job.profile.len().checked_sub(1);
            view.picking = false;
            view.preview = None;
        }
        if button(ui, "cam_select_reset", "選び直す", true) {
            view.first = None;
            view.last = None;
            view.picking = true;
            view.preview = None;
        }
    });
    ui.separator();
    ui.strong("残す取り代");
    number(ui, "cam_range_radial", "片側 mm", &mut view.radial, 100.);
    number(ui, "cam_range_axial", "Z方向 mm", &mut view.axial, 100.);
    ui.horizontal(|ui| {
        if button(ui, "cam_range_rough", "荒：0.1 / 0.1", true) {
            view.radial = 0.1;
            view.axial = 0.1;
            view.feed = job.rough_feed_mm_rev;
            view.depth = job.radial_depth_mm;
        }
        if button(ui, "cam_range_finish", "仕上げ：0 / 0", true) {
            view.radial = 0.;
            view.axial = 0.;
            view.feed = job.finish_feed_mm_rev;
            view.depth = job.radial_depth_mm.min(0.3);
        }
    });
    ui.small(if view.radial == 0. && view.axial == 0. {
        "仕上げを提案。未荒取りの範囲は荒＋仕上げを提案します。"
    } else {
        "取り代を残す荒取りを提案します。"
    });
    let finish = view.radial == 0. && view.axial == 0.;
    number(ui, "cam_range_depth", if finish { "仕上げ1回の上限（片側 mm）" } else { "切込み（片側 mm）" }, &mut view.depth, 100.);
    number(ui, "cam_range_feed", "送り mm/rev", &mut view.feed, 10.);
    let request = view.first.zip(view.last).map(|(a, b)| {
        let c = Contour {
            limits: Limits { stock_front_z_mm: job.stock_front_z_mm, chuck_front_z_mm: job.chuck_front_z_mm, clearance_mm: job.clearance_mm },
            inner: state.model_profile.inner,
            first: a.min(b),
            last: a.max(b),
            profile_snapshot: job.profile.clone(),
            source_snapshot: job.model_source.clone(),
            radial_leave_mm: view.radial,
            axial_leave_mm: view.axial,
            depth_mm: view.depth,
            feed_mm_rev: view.feed,
        };
        json!({"setup":setup.id,"station":station,"contour":c,"expected_revision":revision})
    });
    if view.request != request {
        view.preview = None;
        view.request = request;
        state.machining.show = false;
    }
    if button(ui, "cam_selection_preview", "経路と追加内容を確認", view.request.is_some()) {
        view.action = Some("preview");
    }
    if let Some((rev, _, proposal)) = &view.preview {
        if *rev == revision {
            ui.separator();
            ui.strong(if proposal.rough_then_finish { "荒取り → 仕上げを追加します" } else { "追加する工程" });
            for op in &proposal.operations {
                ui.label(format!("T{:02}  {}", op.station.unwrap_or(0), op.name));
            }
            ui.small(format!("工程 {} の位置へ追加 · 経路 {} 本", proposal.insert_at + 1, proposal.path.moves.len()));
            ui.small(format!("提案後の残材：約 {:.1} mm³", proposal.residual.remaining_mm3));
            if button(ui, "cam_selection_add", "この内容で工程に追加", true) {
                view.action = Some("add");
            }
        } else {
            ui.label("設定が変わりました。経路を再確認してください。");
        }
    }
    if let Some(error) = &view.error {
        ui.colored_label(Tokens::get().warning, error);
    }
}
pub(super) fn pick(state: &mut State, index: usize) {
    let v = &mut state.selection;
    if v.first.is_none() || v.last.is_some() {
        v.first = Some(index);
        v.last = None;
    } else if v.first != Some(index) {
        v.last = Some(index);
        v.picking = false;
    }
    v.preview = None;
    v.error = None;
    state.machining.show = false;
}
pub(super) fn difference_controls(ui: &mut egui::Ui, state: &mut State, revision: u64) {
    ui.horizontal(|ui| {
        if button(ui, "cam_difference", "材料との差分を表示", true) {
            state.selection.action = Some("difference");
        }
        ui_kit::checkbox(ui, "差分を重ねる", &mut state.selection.show_difference);
    });
    if let Some((r, _, d)) = &state.selection.difference
        && *r == revision
    {
        ui.small(format!("初期材料の削る部分：約 {:.1} mm³（断面近似）", d.remaining_mm3));
    }
    if let Some(error) = &state.selection.error {
        ui.colored_label(Tokens::get().warning, error);
    }
}
pub(super) fn process(app: &mut SolveApp, state: &mut State) {
    let Some(action) = state.selection.action.take() else { return };
    let result: Result<(), String> = (|| {
        if action == "difference" {
            let id = app.session.doc.turning.as_ref().and_then(|p| p.setups.get(state.setup)).map(|s| s.id.clone()).ok_or("段取り未設定")?;
            let v = app.run("cam.turning.difference", json!({"setup":id,"expected_revision":app.session.revision}))?;
            let d = serde_json::from_value(v["residual"].clone()).map_err(|e| e.to_string())?;
            state.selection.difference = Some((app.session.revision, id, d));
        } else {
            let request = state.selection.request.clone().ok_or("輪郭点を選択してください")?;
            if action == "preview" {
                let v = app.run("cam.turning.selection.preview", request.clone())?;
                let proposal: Proposal = serde_json::from_value(v["proposal"].clone()).map_err(|e| e.to_string())?;
                let id = request["setup"].as_str().unwrap_or("").to_owned();
                state.machining.step = proposal.path.moves.len();
                state.machining.preview = Some((app.session.revision, id.clone(), proposal.path.clone()));
                state.machining.generation = state.machining.generation.wrapping_add(1);
                state.machining.draft_preview = true;
                state.machining.show_nc = false;
                state.machining.show = true;
                state.spatial.operation = None;
                state.selection.preview = Some((app.session.revision, id, proposal));
            } else if action == "add" {
                let result = app.run("cam.turning.selection.add", request)?;
                state.operation = result["insert_at"].as_u64().unwrap_or(0) as usize;
                state.section = 2;
                state.selection.editing = false;
                state.selection.picking = false;
                state.selection.preview = None;
                state.machining.show = false;
            }
        }
        Ok(())
    })();
    state.selection.error = result.err();
}
