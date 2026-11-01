//! A labelled, non-collapsing group for related form controls.
use eframe::egui;

pub const HORIZONTAL_MARGIN: i8 = 12;

pub fn show<R>(
    ui: &mut egui::Ui,
    legend: &str,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    let framed = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(HORIZONTAL_MARGIN, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let caption = ui.label(egui::RichText::new(legend).strong());
            let inner = contents(ui);
            let bounds = ui.min_rect();
            ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
                node.set_role(egui::accesskit::Role::Group);
                node.set_label(legend);
                node.set_bounds(egui::accesskit::Rect {
                    x0: f64::from(bounds.left()),
                    y0: f64::from(bounds.top()),
                    x1: f64::from(bounds.right()),
                    y1: f64::from(bounds.bottom()),
                });
            });
            (caption.rect, inner)
        });
    let (caption, inner) = framed.inner;
    let border = framed.response.rect;
    let top = caption.center().y;
    // Leave the top edge open behind the legend, without painting over the
    // parent's background or introducing a second text-only copy of the label.
    ui.painter().add(egui::Shape::line(
        vec![
            egui::pos2((caption.left() - 4.0).max(border.left()), top),
            egui::pos2(border.left(), top),
            border.left_bottom(),
            border.right_bottom(),
            egui::pos2(border.right(), top),
            egui::pos2((caption.right() + 4.0).min(border.right()), top),
        ],
        stroke,
    ));
    egui::InnerResponse {
        inner,
        response: framed.response,
    }
}
