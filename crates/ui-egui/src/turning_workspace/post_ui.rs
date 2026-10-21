//! Saved output settings use the same turning-plan command as every other CAM edit.
use super::*;
use cam::post::{NlPost, SpeedMode, XSide};

fn number(ui: &mut egui::Ui, key: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, unit: &str) {
    ui.label(tr(key));
    ui_kit::number_field(ui, value, range, 1., unit);
}

pub(super) fn show(ui: &mut egui::Ui, setup: &mut Setup, selected_operation: &mut usize) {
    ui.label(tr("cam.post_format"));
    let mut nl = setup.post.is_some();
    let response = controls::dropdown(ui, "cam_post_format", tr(if nl { "cam.post_nl" } else { "cam.post_legacy" }).to_string(), |ui| {
        let r = controls::value(ui, &mut nl, true, tr("cam.post_nl"));
        crate::scenario::publish_rect("cam_post_nl", r.rect);
        controls::value(ui, &mut nl, false, tr("cam.post_legacy"));
    });
    crate::scenario::publish_rect("cam_post_format", response.response.rect);
    if !nl {
        setup.post = None;
        return;
    }
    let post = setup.post.get_or_insert_with(NlPost::default);
    ui.small(tr("cam.post_example_values"));
    controls::section(ui, tr("cam.post_settings"), |ui| {
        ui.label(tr("cam.post_program"));
        ui_kit::number_field(ui, &mut post.program_number, 1..=9999, 1., "");
        ui.label(tr("cam.post_x_side"));
        controls::dropdown(
            ui,
            "cam_post_x_side",
            tr(if post.x_side == XSide::Negative { "cam.post_x_negative" } else { "cam.post_x_positive" }).into(),
            |ui| {
                let r = controls::value(ui, &mut post.x_side, XSide::Negative, tr("cam.post_x_negative"));
                crate::scenario::publish_rect("cam_post_x_negative", r.rect);
                let r = controls::value(ui, &mut post.x_side, XSide::Positive, tr("cam.post_x_positive"));
                crate::scenario::publish_rect("cam_post_x_positive", r.rect);
            },
        );
        ui.label(tr("cam.post_work_offset"));
        controls::dropdown(ui, "cam_post_work_offset", format!("G{}", post.work_offset), |ui| {
            for n in 54..=59 {
                controls::value(ui, &mut post.work_offset, n, format!("G{n}"));
            }
        });
        ui.label(tr("cam.post_max_rpm"));
        ui_kit::number_field(ui, &mut post.max_rpm, 1..=60_000, 50., " rpm");
        number(ui, "cam.post_rough_speed", &mut post.rough_surface_m_min, 0.1..=10_000., " m/min");
        number(ui, "cam.post_finish_speed", &mut post.finish_surface_m_min, 0.1..=10_000., " m/min");
        ui.label(tr("cam.post_drill_rpm"));
        ui_kit::number_field(ui, &mut post.drill_rpm, 1..=60_000, 50., " rpm");
        ui.label(tr("cam.post_groove_rpm"));
        ui_kit::number_field(ui, &mut post.groove_rpm, 1..=60_000, 50., " rpm");
        ui_kit::checkbox(ui, tr("cam.post_same_offsets"), &mut post.example_same_number_offsets);
        ui_kit::checkbox(ui, tr("cam.post_coolant"), &mut post.coolant);
        ui_kit::checkbox(ui, tr("cam.post_optional_stop"), &mut post.optional_stop);
        ui_kit::checkbox(ui, tr("cam.post_machine_zero"), &mut post.machine_zero_return);
        ui_kit::checkbox(ui, tr("cam.post_auxiliary"), &mut post.example_m86_m85);
        ui.small(tr("cam.post_unverified"));
    });
    controls::section(ui, tr("cam.post_operation_spindle"), |ui| {
        *selected_operation = (*selected_operation).min(setup.operations.len().saturating_sub(1));
        let label = setup.operations.get(*selected_operation).map_or_else(|| tr("cam.no_operations").to_string(), |o| o.name.clone());
        controls::dropdown(ui, "cam_post_operation", label, |ui| {
            for (index, op) in setup.operations.iter().enumerate() {
                controls::value(ui, selected_operation, index, &op.name);
            }
        });
        let Some(op) = setup.operations.get_mut(*selected_operation) else { return };
        let mut own = op.spindle.is_some();
        if ui_kit::checkbox(ui, tr("cam.post_override_spindle"), &mut own).changed() {
            op.spindle = own.then(|| post.spindle(op.kind));
        }
        let Some(spindle) = &mut op.spindle else { return };
        let before = spindle.mode;
        controls::dropdown(
            ui,
            "cam_post_spindle_mode",
            tr(if before == SpeedMode::SurfaceSpeed { "cam.post_css" } else { "cam.post_rpm" }).into(),
            |ui| {
                controls::value(ui, &mut spindle.mode, SpeedMode::SurfaceSpeed, tr("cam.post_css"));
                controls::value(ui, &mut spindle.mode, SpeedMode::Rpm, tr("cam.post_rpm"));
            },
        );
        if before != spindle.mode {
            spindle.speed = if spindle.mode == SpeedMode::Rpm { f64::from(post.drill_rpm) } else { post.finish_surface_m_min };
        }
        let css = spindle.mode == SpeedMode::SurfaceSpeed;
        number(ui, "cam.post_speed", &mut spindle.speed, if css { 0.1..=10_000. } else { 1.0..=60_000. }, if css { " m/min" } else { " rpm" });
        ui.label(tr("cam.post_max_rpm"));
        ui_kit::number_field(ui, &mut spindle.max_rpm, 1..=60_000, 50., " rpm");
    });
}
