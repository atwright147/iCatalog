//! Local text layouts: keep browsing tiles and sidebar density independent.
use eframe::egui;

pub fn inspector(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
}

pub fn dialog(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = egui::vec2(12.0, 8.0);
    ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
}

pub fn section(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}
