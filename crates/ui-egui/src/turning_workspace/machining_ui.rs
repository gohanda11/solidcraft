use egui::{Align2, FontId, Stroke, pos2, vec2};
use serde_json::json;
use solidcraft_engine::doc::cam::{
    Setup,
    machining::{Motion, OdJob, ProfilePoint, Toolpath},
};

use super::controls;
use crate::{SolveApp, i18n::tr, theme::Tokens, ui_kit};

#[derive(Clone, Debug)]
pub enum Action {
    Calculate(String),
    InspectNc(String, u64),
    Export(String, u64),
}

#[derive(Clone, Debug)]
struct NcPreview {
    revision: u64,
    setup: String,
    text: String,
    lines: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct View {
    post_operation: usize,
    text: String,
    loaded_setup: String,
    loaded_profile: Vec<ProfilePoint>,
    pub action: Option<Action>,
    pub preview: Option<(u64, String, Toolpath)>,
    pub(super) generation: u64,
    pub(super) draft_preview: bool,
    pub show: bool,
    pub(super) show_rapid: bool,
    pub(super) phase: usize,
    audit_inner: bool,
    pub(super) step: usize,
    nc: Option<NcPreview>,
    pub(super) show_nc: bool,
    pub error: Option<String>,
}

fn parse_profile(text: &str) -> Result<Vec<ProfilePoint>, String> {
    if text.len() > 16384 {
        return Err(tr("cam.path_profile_limit").into());
    }
    let mut points = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let words: Vec<_> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
        let [z, d] = words.as_slice() else {
            return Err(tr("cam.path_profile_format").into());
        };
        let z: f64 = z.parse().map_err(|_| tr("cam.path_profile_format"))?;
        let d: f64 = d.parse().map_err(|_| tr("cam.path_profile_format"))?;
        if !z.is_finite() || !d.is_finite() || points.len() >= 256 {
            return Err(tr("cam.path_profile_limit").into());
        }
        points.push(ProfilePoint { z_mm: z, diameter_mm: d });
    }
    Ok(points)
}

pub fn settings(ui: &mut egui::Ui, setup: &mut Setup, view: &mut View, revision: u64) {
    super::post_ui::show(ui, setup, &mut view.post_operation);
    ui.small(tr("cam.path_scope"));
    if setup.od_job.is_none() {
        if ui_kit::button(ui, tr("cam.path_enable"), ui_kit::Button::Normal).clicked() {
            setup.od_job = Some(OdJob::default());
        }
        return;
    }
    let Some(job) = &mut setup.od_job else { return };
    if view.loaded_setup != setup.id || view.loaded_profile != job.profile {
        view.loaded_setup = setup.id.clone();
        view.loaded_profile = job.profile.clone();
        view.text = job.profile.iter().map(|p| format!("{} {}", p.z_mm, p.diameter_mm)).collect::<Vec<_>>().join("\n");
    }
    controls::section(ui, tr("cam.path_profile"), |ui| {
        ui.small(tr("cam.path_profile_help"));
        ui_kit::text_area(ui, "cam_profile", ui.next_auto_id(), &mut view.text, 6, 16384);
        if ui_kit::button(ui, tr("cam.path_profile_apply"), ui_kit::Button::Normal).clicked() {
            match parse_profile(&view.text) {
                Ok(points) => {
                    job.profile = points;
                    job.model_source = None;
                    view.error = None;
                }
                Err(e) => view.error = Some(e),
            }
        }
    });
    controls::section(ui, tr("cam.path_limits"), |ui| {
        super::number(ui, "cam.path_front_stock", &mut job.stock_front_z_mm);
        ui.horizontal(|ui| {
            ui_kit::number_field(ui, &mut job.chuck_front_z_mm, -1e6..=0.0, 0.1, " mm");
            ui.label(tr("cam.path_chuck"));
        });
        super::number(ui, "cam.path_clearance", &mut job.clearance_mm);
    });
    ui.strong(tr("cam.path_allowances"));
    super::number(ui, "cam.path_radial_leave", &mut job.radial_stock_to_leave_mm);
    ui.small(format!("{} {:.4} mm", tr("cam.path_diameter_leave"), 2.0 * job.radial_stock_to_leave_mm));
    super::number(ui, "cam.path_axial_leave", &mut job.axial_stock_to_leave_mm);
    controls::section(ui, tr("cam.path_conditions"), |ui| {
        super::number(ui, "cam.path_depth", &mut job.radial_depth_mm);
        super::number(ui, "cam.path_face_depth", &mut job.face_depth_mm);
        for (key, value) in [("cam.path_rough_feed", &mut job.rough_feed_mm_rev), ("cam.path_finish_feed", &mut job.finish_feed_mm_rev)] {
            ui.horizontal(|ui| {
                ui_kit::number_field(ui, value, 0.0..=10.0, 0.01, " mm/rev");
                ui.label(tr(key));
            });
        }
    });
    ui_kit::checkbox(ui, tr("cam.path_finish"), &mut job.finish);
    let pending = parse_profile(&view.text).ok().as_ref() != Some(&job.profile);
    if pending {
        ui.small(tr("cam.path_apply_first"));
    }
    let response = ui.add_enabled_ui(!pending, |ui| ui_kit::button(ui, tr("cam.path_calculate"), ui_kit::Button::Normal)).inner;
    crate::scenario::publish_rect("cam_path_calculate", response.rect);
    if response.clicked() {
        view.action = Some(Action::Calculate(setup.id.clone()));
    }
    let valid = view.preview.as_ref().is_some_and(|(r, id, _)| *r == revision && id == &setup.id) && !pending && !view.draft_preview;
    let response = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("cam.path_nc_view"), ui_kit::Button::Normal)).inner;
    crate::scenario::publish_rect("cam_path_nc_view", response.rect);
    if response.clicked() {
        view.action = Some(Action::InspectNc(setup.id.clone(), revision));
    }
    let response = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("cam.path_export"), ui_kit::Button::Normal)).inner;
    crate::scenario::publish_rect("cam_path_export", response.rect);
    if response.clicked() {
        view.action = Some(Action::Export(setup.id.clone(), revision));
    }
    ui.small(tr("cam.path_review_only"));
    if view.preview.is_some() && !valid {
        ui.colored_label(Tokens::get().warning, tr("cam.path_stale"));
    }
    if let Some(e) = &view.error {
        ui.colored_label(Tokens::get().warning, e);
    }
}

pub fn process(app: &mut SolveApp, view: &mut View) {
    match view.action.take() {
        Some(Action::Calculate(setup)) => {
            view.preview = None;
            view.nc = None;
            view.show_nc = false;
            match app.run("cam.turning.toolpath", json!({"setup":setup})).and_then(|v| {
                let rev = v.get("revision").and_then(|r| r.as_u64()).ok_or("missing revision")?;
                let path: Toolpath = serde_json::from_value(v.get("toolpath").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
                Ok((rev, path))
            }) {
                Ok((rev, path)) => {
                    view.step = path.moves.len();
                    view.preview = Some((rev, setup, path));
                    view.generation = view.generation.wrapping_add(1);
                    view.draft_preview = false;
                    view.show = true;
                    view.error = None;
                }
                Err(e) => view.error = Some(e),
            }
        }
        Some(Action::InspectNc(setup, revision)) => {
            view.nc = None;
            match app
                .run("cam.turning.nc", json!({"setup":setup,"expected_revision":revision}))
                .and_then(|value| value.get("text").and_then(|t| t.as_str()).map(str::to_owned).ok_or_else(|| "missing NC text".to_string()))
            {
                Ok(text) => {
                    let lines = text.lines().map(str::to_owned).collect();
                    view.nc = Some(NcPreview { revision, setup, text, lines });
                    view.show_nc = true;
                    view.show = true;
                    view.error = None;
                }
                Err(error) => view.error = Some(error),
            }
        }
        Some(Action::Export(setup, revision)) => {
            app.pick_save_command("turning-review.nc", &["nc"], "cam.turning.nc", json!({"setup":setup,"expected_revision":revision}));
        }
        None => {}
    }
}

pub fn active(app: &SolveApp) -> bool {
    let state = &app.turning_workspace;
    let selected = app.session.doc.turning.as_ref().and_then(|p| p.setups.get(state.setup));
    state.machining.show && state.machining.preview.as_ref().is_some_and(|(_, id, _)| selected.is_some_and(|s| &s.id == id))
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    egui::Frame::new().fill(Tokens::get().panel).inner_margin(12.0).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        contents(app, ui);
    });
}

pub(super) fn contents(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let view = &mut app.turning_workspace.machining;
    let Some((revision, _, path)) = &view.preview else { return };
    if *revision != app.session.revision {
        ui.heading(tr("cam.path_stale"));
        ui.label(tr("cam.path_recalculate"));
        if ui_kit::button(ui, tr("cam.path_back_model"), ui_kit::Button::Normal).clicked() {
            view.show = false;
        }
        return;
    }
    if view.show_nc
        && let Some(nc) = &view.nc
    {
        if nc.revision != *revision || view.preview.as_ref().is_none_or(|(_, id, _)| id != &nc.setup) {
            ui.label(tr("cam.path_stale"));
            return;
        }
        ui.horizontal(|ui| {
            ui.heading(tr("cam.path_nc_view"));
            let response = ui_kit::button(ui, tr("cam.path_back_paths"), ui_kit::Button::Normal);
            crate::scenario::publish_rect("cam_path_back_paths", response.rect);
            if response.clicked() {
                view.show_nc = false;
            }
            if ui_kit::button(ui, tr("cam.path_nc_copy"), ui_kit::Button::Normal).clicked() {
                ui.ctx().copy_text(nc.text.clone());
            }
        });
        ui.label(tr("cam.path_review_only"));
        ui.separator();
        // Only lay out visible lines, even for a 20,000-move program.
        let height = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both().id_salt("turning_nc_text").show_rows(ui, height, nc.lines.len(), |ui, rows| {
            for index in rows {
                if let Some(line) = nc.lines.get(index) {
                    ui.add(egui::Label::new(egui::RichText::new(line).monospace()).wrap_mode(egui::TextWrapMode::Extend));
                }
            }
        });
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.strong(tr("cam.path_preview"));
        if ui_kit::button(ui, tr("cam.path_back_model"), ui_kit::Button::Normal).clicked() {
            view.show = false;
        }
        for (i, key) in ["cam.path_all", "cam.path_rough", "cam.path_finish_only"].iter().enumerate() {
            controls::value(ui, &mut view.phase, i, tr(key));
        }
        ui_kit::checkbox(ui, tr("cam.path_rapid"), &mut view.show_rapid);
    });
    ui.small(tr("cam.path_nominal"));
    ui_kit::slider(
        ui,
        &mut view.step,
        0..=path.moves.len(),
        ui_kit::Slider { integer: true, step: Some(1.0), format: |n| format!("{n:.0}"), ..Default::default() },
    );
    let height = (ui.available_height() * 0.55).max(180.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let area = rect.shrink2(vec2(45.0, 32.0));
    let zmin = path.job.chuck_front_z_mm;
    let zmax = path.job.stock_front_z_mm + path.job.clearance_mm * 2.0;
    let rmax = path.stock.outer_diameter_mm / 2.0 + path.job.clearance_mm * 2.0;
    let scale = (area.width() / (zmax - zmin) as f32).min(area.height() / rmax as f32);
    let map = |p: ProfilePoint| pos2(area.left() + (p.z_mm - zmin) as f32 * scale, area.bottom() - (p.diameter_mm / 2.0) as f32 * scale);
    let bottom = map(ProfilePoint { z_mm: zmin, diameter_mm: 0.0 });
    painter.line_segment([bottom, map(ProfilePoint { z_mm: zmax, diameter_mm: 0.0 })], Stroke::new(1.0, t.text_dim));
    painter.rect_filled(
        egui::Rect::from_two_pos(
            map(ProfilePoint { z_mm: path.job.stock_front_z_mm - path.stock.length_mm, diameter_mm: path.stock.inner_diameter_mm }),
            map(ProfilePoint { z_mm: path.job.stock_front_z_mm, diameter_mm: path.stock.outer_diameter_mm }),
        ),
        0.0,
        t.text_dim.gamma_multiply(0.18),
    );
    let fill = |profile: &[ProfilePoint], color| {
        for pair in profile.windows(2) {
            let [a, b] = pair else { continue };
            if (a.z_mm - b.z_mm).abs() < 1e-8 {
                continue;
            }
            let low = path.stock.inner_diameter_mm;
            painter.add(egui::Shape::convex_polygon(
                vec![map(*a), map(*b), map(ProfilePoint { z_mm: b.z_mm, diameter_mm: low }), map(ProfilePoint { z_mm: a.z_mm, diameter_mm: low })],
                color,
                Stroke::NONE,
            ));
        }
    };
    if path.inner {
        fill(&path.job.profile, t.warning.gamma_multiply(0.4));
        fill(&path.rough_profile, t.panel);
    } else {
        fill(&path.rough_profile, t.warning.gamma_multiply(0.4));
        fill(&path.job.profile, t.accent_soft);
    }
    if let Some(id) = &path.id_path {
        fill(&id.job.profile, t.warning.gamma_multiply(0.4));
        fill(&id.rough_profile, t.panel);
        for (profile, color) in [(&id.rough_profile, t.warning), (&id.job.profile, t.accent)] {
            for pair in profile.windows(2) {
                let [a, b] = pair else { continue };
                painter.line_segment([map(*a), map(*b)], Stroke::new(2.0, color));
            }
        }
    }
    for (profile, color) in [(&path.rough_profile, t.warning), (&path.job.profile, t.accent)] {
        for pair in profile.windows(2) {
            let [a, b] = pair else { continue };
            painter.line_segment([map(*a), map(*b)], Stroke::new(2.0, color));
        }
    }
    for m in path.moves.iter().take(view.step) {
        let finish = solidcraft_engine::doc::cam::machining::interior::is_finish(m.phase);
        if (view.phase == 1 && finish) || (view.phase == 2 && !finish) || (m.motion == Motion::Rapid && !view.show_rapid) {
            continue;
        }
        let color = if m.motion == Motion::Rapid {
            t.text_dim.gamma_multiply(0.4)
        } else if finish {
            t.sketch_accent
        } else {
            t.warning.gamma_multiply(0.8)
        };
        painter.line_segment([map(m.from), map(m.to)], Stroke::new(1.0, color));
    }
    if let Some(m) = view.step.checked_sub(1).and_then(|n| path.moves.get(n)) {
        painter.circle_filled(map(m.to), 4.0, t.text);
    }
    let font = FontId::proportional(Tokens::get().font_small);
    let mut labels: Vec<egui::Rect> = Vec::new();
    for p in path.job.profile.iter().chain(path.id_path.iter().flat_map(|id| id.job.profile.iter())) {
        let galley = painter.layout_no_wrap(format!("X{:.2} Z{:.2}", p.diameter_mm, p.z_mm), font.clone(), t.text);
        let at = map(*p) + vec2(-galley.size().x * 0.5, -10.0 - galley.size().y);
        let bounds = egui::Rect::from_min_size(at, galley.size()).expand(3.0);
        if labels.len() < 12 && !labels.iter().any(|r| r.intersects(bounds)) {
            painter.galley(at, galley, t.text);
            labels.push(bounds);
        }
    }
    painter.text(bottom, Align2::LEFT_TOP, format!("{} Z{:.2}", tr("cam.path_chuck"), zmin), font.clone(), t.warning);
    painter.text(pos2(area.right(), area.bottom() + 6.0), Align2::RIGHT_TOP, "Z → / X = diameter", font, t.text_dim);
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(t.text_dim, tr("cam.path_stock_legend"));
        ui.colored_label(t.warning, tr("cam.path_rough_legend"));
        ui.colored_label(t.accent, tr("cam.path_finished_legend"));
    });
    if path.source_setup.is_none() {
        ui.small(format!(
            "{} {:.3} cm³   /   {} {:.3} cm³",
            tr("cam.path_rough_volume"),
            path.rough_removed_volume_mm3 / 1000.0,
            tr("cam.path_finish_volume"),
            path.finish_removed_volume_mm3 / 1000.0
        ));
    } else {
        for warning in &path.warnings {
            ui.small(warning);
        }
    }
    ui.small(tr("cam.path_audit_help"));
    if path.id_path.is_some() {
        ui.horizontal(|ui| {
            for (inner, key, handle) in [(false, "cam.profile_od", "cam_audit_od"), (true, "cam.profile_id", "cam_audit_id")] {
                let response = controls::value(ui, &mut view.audit_inner, inner, tr(key));
                crate::scenario::publish_rect(handle, response.rect);
                crate::scenario::publish_handle(handle, response.rect.center());
            }
        });
    }
    egui::ScrollArea::vertical().id_salt("cam_allowance_table").show(ui, |ui| {
        egui::Grid::new("cam_allowances").striped(true).show(ui, |ui| {
            for key in ["cam.path_z", "cam.path_target_diameter", "cam.path_rough_diameter", "cam.path_remaining_radial"] {
                ui.strong(tr(key));
            }
            ui.end_row();
            let (label, rows) = if view.audit_inner
                && let Some(id) = &path.id_path
            {
                ("cam.profile_id", &id.allowance_audit)
            } else {
                (if path.inner { "cam.profile_id" } else { "cam.profile_od" }, &path.allowance_audit)
            };
            ui.strong(tr(label));
            ui.end_row();
            for row in rows {
                // Shoulder probes can be only micrometres apart. Do not round distinct
                // sides to the same displayed Z while showing different remaining stock.
                ui.monospace(format!("{:.8}", row.z_mm));
                for value in [row.finished_diameter_mm, row.rough_diameter_mm, row.radial_remaining_mm] {
                    ui.monospace(format!("{value:.4}"));
                }
                ui.end_row();
            }
        });
    });
}
