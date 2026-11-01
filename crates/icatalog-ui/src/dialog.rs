//! Shared heading and close control for app-owned modal dialogs.
use eframe::egui;

pub fn header(ui: &mut egui::Ui, title: &str, close_enabled: bool) -> bool {
    ui.horizontal(|ui| {
        ui.heading(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(close_enabled, |ui| {
                let response = ui
                    .add(
                        egui::Button::new("")
                            .frame(false)
                            .min_size(egui::vec2(24.0, 24.0)),
                    )
                    .on_hover_text("Close dialog");
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        "Close dialog",
                    )
                });
                let icon = response.rect.shrink(7.0);
                let stroke = ui.style().interact(&response).fg_stroke;
                ui.painter()
                    .line_segment([icon.left_top(), icon.right_bottom()], stroke);
                ui.painter()
                    .line_segment([icon.right_top(), icon.left_bottom()], stroke);
                response.clicked()
            })
            .inner
        })
        .inner
    })
    .inner
}
