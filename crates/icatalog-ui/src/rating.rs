//! Shared five-star presentation and editing; persistence stays with commands.
use eframe::egui;

fn stars(value: i64) -> String {
    (1..=5)
        .map(|star| {
            if star <= value.clamp(0, 5) {
                '★'
            } else {
                '☆'
            }
        })
        .collect()
}

/// A display-only rating that leaves asset-row selection and dragging intact.
pub fn display(ui: &mut egui::Ui, value: i64, size: f32, color: egui::Color32) -> egui::Response {
    let text = format!("Rating: {}", stars(value));
    ui.add(
        egui::Label::new(egui::RichText::new(&text).size(size).color(color))
            .truncate()
            .sense(egui::Sense::hover()),
    )
    .on_hover_text(text)
}

/// Five accessible star buttons. None means clear; an explicit zero is retained.
pub fn editor(ui: &mut egui::Ui, value: &mut Option<i64>, caption: &str) -> egui::Response {
    let before = *value;
    let mut response = ui.push_id(caption, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding = egui::vec2(2.0, 0.0);
            for star in 1..=5 {
                let filled = star <= value.unwrap_or(0).clamp(0, 5);
                let response = ui.add(egui::Button::new(
                    egui::RichText::new(if filled { "★" } else { "☆" }).size(20.0),
                ).frame(false).min_size(egui::vec2(24.0, 24.0)));
                if response.clicked() {
                    *value = if *value == Some(star) { None } else { Some(star) };
                    response.request_focus();
                }
                if ui.is_enabled() && response.has_focus() {
                    keyboard(ui, value);
                }
                response.widget_info(|| egui::WidgetInfo::selected(
                    egui::WidgetType::Button, ui.is_enabled(), *value == Some(star),
                    format!("{caption} {star}"),
                ));
                response.on_hover_text(format!(
                    "{caption}: {star} out of 5. Click the selected star again to clear. Keys 0–5 set the value; Delete clears."
                ));
            }
        }).response
    }).inner;
    if *value != before {
        response.mark_changed();
    }
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), caption);
        info.current_text_value =
            Some(value.map_or_else(|| "Not set".into(), |v| format!("{v} out of 5")));
        info
    });
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::accesskit::Role::Group);
    });
    response
}

fn keyboard(ui: &mut egui::Ui, value: &mut Option<i64>) {
    for (key, rating) in [
        (egui::Key::Num0, 0),
        (egui::Key::Num1, 1),
        (egui::Key::Num2, 2),
        (egui::Key::Num3, 3),
        (egui::Key::Num4, 4),
        (egui::Key::Num5, 5),
    ] {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
            *value = Some(rating);
        }
    }
    for key in [egui::Key::Backspace, egui::Key::Delete] {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
            *value = None;
        }
    }
    for (key, increment) in [
        (egui::Key::ArrowRight, 1),
        (egui::Key::ArrowUp, 1),
        (egui::Key::ArrowLeft, -1),
        (egui::Key::ArrowDown, -1),
    ] {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
            *value = Some(
                value
                    .unwrap_or(0)
                    .clamp(0, 5)
                    .saturating_add(increment)
                    .clamp(0, 5),
            );
        }
    }
    for (key, rating) in [(egui::Key::Home, 0), (egui::Key::End, 5)] {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
            *value = Some(rating);
        }
    }
}
