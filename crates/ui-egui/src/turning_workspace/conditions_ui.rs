use super::*;
use cam::presets::{Level, MATERIALS};

pub(super) fn show(ui: &mut egui::Ui, plan: &mut TurningPlan, state: &mut State) {
    ui.label(tr("cam.material"));
    let custom = !MATERIALS.contains(&plan.material.as_str());
    let combo = controls::dropdown(ui, "cam_material_list", (if custom { tr("cam.material_custom") } else { &plan.material }).to_string(), |ui| {
        for material in MATERIALS {
            let r = controls::value(ui, &mut plan.material, (*material).into(), *material);
            crate::scenario::publish_rect(&format!("cam_material_{material}"), r.rect);
        }
        if controls::pick(ui, custom, tr("cam.material_custom")).clicked() && !custom {
            plan.material = String::new();
        }
    });
    crate::scenario::publish_rect("cam_material_list", combo.response.rect);
    if custom {
        text(ui, "cam.material", &mut plan.material);
    }
    ui.label(tr("cam.conditions_candidate"));
    let response = controls::dropdown(ui, "cam_condition_level", (tr(level_key(state.condition_level))).to_string(), |ui| {
        for (level, id) in [(Level::Gentle, "gentle"), (Level::Standard, "standard"), (Level::Productive, "productive")] {
            let r = controls::value(ui, &mut state.condition_level, level, tr(level_key(level)));
            crate::scenario::publish_rect(&format!("cam_level_{id}"), r.rect);
        }
    });
    crate::scenario::publish_rect("cam_condition_level", response.response.rect);
    if let Some(c) = cam::presets::candidate(&plan.material, state.condition_level, false) {
        for (inner, c) in [(false, c)].into_iter().chain(cam::presets::candidate(&plan.material, state.condition_level, true).map(|c| (true, c))) {
            ui.label(format!(
                "{}: ap {:.2} / F {:.3} → {:.3}",
                tr(if inner { "cam.profile_id" } else { "cam.profile_od" }),
                c.depth,
                c.rough_feed,
                c.finish_feed
            ));
            if !inner {
                ui.small(format!("{}: {:.2} mm", tr("cam.path_face_depth"), c.face_depth));
            }
        }
        ui.add(egui::Label::new(egui::RichText::new(tr("cam.conditions_units")).small()).wrap());
        let ready = plan.setups.get(state.setup).is_some_and(|s| s.od_job.is_some() || s.id_job.is_some());
        let r = ui.add_enabled_ui(ready, |ui| ui_kit::button(ui, tr("cam.conditions_apply"), ui_kit::Button::Normal)).inner;
        crate::scenario::publish_rect("cam_conditions_apply", r.rect);
        crate::scenario::publish_handle("cam_conditions_apply", r.rect.center());
        if r.clicked()
            && let Some(setup) = plan.setups.get_mut(state.setup)
        {
            cam::presets::apply(setup, &plan.material, state.condition_level);
        }
    }
    ui.add(egui::Label::new(egui::RichText::new(tr("cam.conditions_trial")).small()).wrap());
    ui.separator();
}
fn level_key(level: Level) -> &'static str {
    match level {
        Level::Gentle => "cam.level_gentle",
        Level::Standard => "cam.level_standard",
        Level::Productive => "cam.level_productive",
    }
}
