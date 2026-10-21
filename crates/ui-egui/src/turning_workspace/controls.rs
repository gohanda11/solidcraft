//! CAM adapters retain selection/change semantics and automation handles over the shared kit.
use crate::{theme::Tokens, ui_kit};

pub(super) fn pick(ui: &mut egui::Ui, selected: bool, label: impl AsRef<str>) -> egui::Response {
    let label = label.as_ref();
    let width = if ui.layout().main_dir().is_horizontal() {
        let t = Tokens::get();
        (ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(t.font_label), t.text).size().x + t.space_24)
            .min(ui.available_width())
    } else {
        ui.available_width()
    };
    ui.allocate_ui_with_layout(egui::vec2(width, Tokens::get().control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        ui_kit::list_picker(ui, label, label, selected)
    })
    .inner
}

pub(super) fn value<T: PartialEq>(ui: &mut egui::Ui, current: &mut T, candidate: T, label: impl AsRef<str>) -> egui::Response {
    let mut response = pick(ui, *current == candidate, label);
    if response.clicked() && *current != candidate {
        *current = candidate;
        response.mark_changed();
    }
    response
}

pub(super) fn dropdown(ui: &mut egui::Ui, id: impl egui::AsIdSalt, label: String, add: impl FnOnce(&mut egui::Ui)) -> egui::InnerResponse<()> {
    let response = ui_kit::dropdown_menu(ui, id, &label, add);
    egui::InnerResponse { inner: (), response }
}

pub(super) struct Section {
    pub header_rect: egui::Rect,
}

pub(super) fn section(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) -> Section {
    let t = Tokens::get();
    let header_rect =
        egui::Rect::from_min_size(ui.next_widget_position() + egui::vec2(0.0, t.space_2), egui::vec2(ui.available_width(), t.control_height));
    ui_kit::section(ui, label, label, false, add);
    Section { header_rect }
}
