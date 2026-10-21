use super::*;
use cam::machining::{ModelSource, ProfilePoint};

#[derive(Clone, Debug, serde::Deserialize)]
struct Preview {
    source: ModelSource,
    profile: Vec<ProfilePoint>,
    revision: u64,
    #[serde(default)]
    inner: bool,
}

#[derive(Clone, Debug)]
enum Action {
    Read,
    Apply(String),
    Section(String),
}

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub inner: bool,
    body: String,
    reverse: bool,
    preview: Option<Preview>,
    action: Option<Action>,
    error: Option<String>,
    attempted: Option<(String, String)>,
    pub(super) suppress_auto: bool,
}

pub(super) fn section_settings(ui: &mut egui::Ui, view: &mut View, bodies: &[String], setup: &Setup) {
    ui.heading("モデル・加工方向");
    ui.label("中心軸を水平に揃え、最初に削る側を右に表示します。");
    if view.body.is_empty() {
        view.body = setup
            .od_job
            .as_ref()
            .and_then(|j| j.model_source.as_ref())
            .map(|s| s.body.clone())
            .or_else(|| (bodies.len() == 1).then(|| bodies.first().cloned()).flatten())
            .unwrap_or_default();
        view.reverse = setup.od_job.as_ref().and_then(|j| j.model_source.as_ref()).is_some_and(|s| s.reverse);
    }
    if let Some(source) = setup.od_job.as_ref().and_then(|j| j.model_source.as_ref())
        && source.body == view.body
    {
        // Undo/redo may have restored a different direction without changing the selected body.
        view.reverse = source.reverse;
    }
    let before = view.body.clone();
    controls::dropdown(ui, "cam_source_body", if view.body.is_empty() { "モデルを選択".into() } else { view.body.clone() }, |ui| {
        for body in bodies {
            controls::value(ui, &mut view.body, body.clone(), body);
        }
    });
    let key = (setup.id.clone(), view.body.clone());
    if bodies.contains(&view.body)
        && (before != view.body || (!view.suppress_auto && setup.od_job.is_none() && view.attempted.as_ref() != Some(&key)))
    {
        view.attempted = Some(key);
        view.action = Some(Action::Section(setup.id.clone()));
    }
    if button(ui, "cam_model_read", "外径・内径・端面を再抽出", bodies.contains(&view.body)) {
        view.action = Some(Action::Section(setup.id.clone()));
    }
    if button(ui, "cam_model_reverse", "加工方向を反転", bodies.contains(&view.body)) {
        view.reverse = !view.reverse;
        view.action = Some(Action::Section(setup.id.clone()));
    }
    ui.separator();
    ui.strong("線をダブルクリックして編集");
    ui.label("公差・ねじ指定・線の分割を設定できます。");
    if let Some(source) = setup.od_job.as_ref().and_then(|j| j.model_source.as_ref()) {
        ui.small(format!("中心軸 ({:.3}, {:.3}, {:.3})", source.axis[0], source.axis[1], source.axis[2]));
        if source.approximation_mm > 0. {
            ui.small(format!("円弧の折線近似 ≤ {:.3} mm", source.approximation_mm));
        }
    }
    if setup.drawing.as_ref().is_some_and(|d| d.inner.iter().any(|p| p.diameter_mm == 0.) && d.inner.iter().any(|p| p.diameter_mm > 0.)) {
        ui.colored_label(Tokens::get().warning, "止まり穴は輪郭表示のみ。内径経路は貫通穴に対応しています。");
    }
    if let Some(error) = &view.error {
        ui.colored_label(Tokens::get().warning, error);
    }
}

fn button(ui: &mut egui::Ui, id: &str, key: &str, enabled: bool) -> bool {
    let r = ui.add_enabled_ui(enabled, |ui| ui_kit::button(ui, tr(key), ui_kit::Button::Normal)).inner;
    crate::scenario::publish_rect(id, r.rect);
    if enabled {
        crate::scenario::publish_handle(id, r.rect.center());
    }
    r.clicked()
}

pub(super) fn settings(ui: &mut egui::Ui, view: &mut View, bodies: &[String], setup: &Setup, revision: u64) {
    ui.set_max_width((ui.available_width() - 12.0).max(100.0));
    ui.horizontal(|ui| {
        for (inner, key, handle) in [(false, "cam.profile_od", "cam_model_od"), (true, "cam.profile_id", "cam_model_id")] {
            let response = controls::value(ui, &mut view.inner, inner, tr(key));
            crate::scenario::publish_rect(handle, response.rect);
        }
    });
    ui.strong(tr("cam.model_title"));
    ui.label(tr("cam.model_help"));
    if view.inner {
        ui.small(tr("cam.id_scope"));
    }
    if view.body.is_empty() && bodies.len() == 1 {
        view.body = bodies.first().cloned().unwrap_or_default();
    }
    controls::dropdown(ui, "cam_source_body", (if view.body.is_empty() { tr("cam.model_select") } else { &view.body }).to_string(), |ui| {
        for body in bodies {
            controls::value(ui, &mut view.body, body.clone(), body);
        }
    });
    let reverse = ui_kit::checkbox(ui, tr("cam.model_reverse"), &mut view.reverse);
    crate::scenario::publish_rect("cam_model_reverse", reverse.rect);
    if button(ui, "cam_model_read", "cam.model_read", bodies.contains(&view.body)) {
        view.action = Some(Action::Read);
    }
    if bodies.is_empty() {
        ui.label(tr("cam.model_none"));
    }
    if let Some(preview) = &view.preview {
        let valid =
            preview.inner == view.inner && preview.revision == revision && preview.source.body == view.body && preview.source.reverse == view.reverse;
        ui.separator();
        if valid {
            ui.label(format!("{}: {}", tr("cam.model_points"), preview.profile.len()));
            chart(ui, &preview.profile);
            if preview.source.approximation_mm > 0.0 {
                ui.label(tr("cam.model_approximation"));
            }
            let origin = preview.source.origin;
            let axis = preview.source.axis;
            ui.small(format!("{}: ({:.3}, {:.3}, {:.3})", tr("cam.model_origin"), origin[0], origin[1], origin[2]));
            ui.small(format!("{}: ({:.3}, {:.3}, {:.3})", tr("cam.model_axis"), axis[0], axis[1], axis[2]));
            if !view.inner {
                ui.label(tr("cam.model_exterior"));
            }
            if button(ui, "cam_model_apply", "cam.model_apply", true) {
                view.action = Some(Action::Apply(setup.id.clone()));
            }
        } else {
            ui.label(tr("cam.model_stale"));
        }
    }
    if let Some(error) = &view.error {
        ui.colored_label(Tokens::get().warning, tr("cam.model_failed"));
        controls::section(ui, tr("cam.model_error_detail"), |ui| {
            ui.label(error);
        });
    }
}

pub(super) fn chart(ui: &mut egui::Ui, profile: &[ProfilePoint]) {
    let Some(last) = profile.last() else { return };
    let length = -last.z_mm;
    let diameter = profile.iter().map(|p| p.diameter_mm).fold(0.0, f64::max);
    if length <= 0.0 || diameter <= 0.0 {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 145.0), egui::Sense::hover());
    let area = rect.shrink2(egui::vec2(8., 22.));
    let at = |p: &ProfilePoint| {
        egui::pos2(area.right() + (p.z_mm / length) as f32 * area.width(), area.bottom() - (p.diameter_mm / diameter) as f32 * area.height())
    };
    let paint = ui.painter_at(rect);
    let t = Tokens::get();
    paint.line_segment([area.left_bottom(), area.right_bottom()], egui::Stroke::new(1., t.text_dim));
    for pair in profile.windows(2) {
        if let (Some(a), Some(b)) = (pair.first(), pair.get(1)) {
            paint.line_segment([at(a), at(b)], egui::Stroke::new(2., t.accent));
        }
    }
    paint.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        format!("X / Ø{diameter:.3} mm"),
        egui::FontId::proportional(Tokens::get().font_small),
        t.text,
    );
    paint.text(
        rect.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        format!("Z -{length:.3}"),
        egui::FontId::proportional(Tokens::get().font_small),
        t.text,
    );
    paint.text(rect.right_bottom(), egui::Align2::RIGHT_BOTTOM, "Z 0", egui::FontId::proportional(Tokens::get().font_small), t.text);
}

pub(super) fn process(app: &mut SolveApp, view: &mut View) {
    match view.action.take() {
        Some(Action::Section(setup)) => {
            view.error = app.run("cam.turning.profile.apply", json!({"body":view.body,"reverse":view.reverse,"setup":setup,"expected_revision":app.session.revision,"all":true,"create_operations":false})).err();
            if view.error.is_some() {
                view.reverse = app
                    .session
                    .doc
                    .turning
                    .as_ref()
                    .and_then(|p| p.setups.iter().find(|s| s.id == setup))
                    .and_then(|s| s.od_job.as_ref())
                    .and_then(|j| j.model_source.as_ref())
                    .is_some_and(|s| s.reverse);
            }
            view.preview = None;
        }
        Some(Action::Read) => {
            view.preview = None;
            match app
                .run("cam.turning.profile", json!({"body":view.body,"reverse":view.reverse,"inner":view.inner}))
                .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            {
                Ok(preview) => {
                    view.preview = Some(preview);
                    view.error = None;
                }
                Err(e) => view.error = Some(e),
            }
        }
        Some(Action::Apply(setup)) => {
            if let Some(preview) = &view.preview {
                let legacy = app
                    .session
                    .doc
                    .turning
                    .as_ref()
                    .and_then(|p| p.setups.iter().find(|s| s.id == setup))
                    .is_some_and(|s| s.operations.iter().any(|o| o.kind == OperationKind::OdRough && o.cutting.is_none()));
                match app.run(
                    "cam.turning.profile.apply",
                    json!({"body":preview.source.body,"reverse":preview.source.reverse,"setup":setup,"expected_revision":preview.revision,"inner":preview.inner,"create_operations":legacy}),
                ) {
                    Ok(_) => {
                        view.preview = None;
                        view.error = None;
                    }
                    Err(e) => view.error = Some(e),
                }
            }
        }
        None => {}
    }
}

impl View {
    pub(super) fn pick_body(&mut self, body: &str) {
        self.body = body.into();
        self.action = Some(Action::Read);
    }
    pub(super) fn current_preview(&self, revision: u64) -> Option<(&ModelSource, &[ProfilePoint])> {
        self.preview
            .as_ref()
            .filter(|p| p.revision == revision && p.inner == self.inner && p.source.body == self.body && p.source.reverse == self.reverse)
            .map(|p| (&p.source, p.profile.as_slice()))
    }
}
