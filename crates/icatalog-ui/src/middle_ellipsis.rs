//! Single-line labels that keep both ends visible when space is limited.
use eframe::egui;
use std::sync::Arc;

/// Fit text to the current width, retaining the full value for hover and accessibility.
pub fn label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let galley = layout(ui, text, ui.available_width());
    let response = ui.add(egui::Label::new(galley).show_tooltip_when_elided(false));
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    response.on_hover_text(text)
}

fn layout(ui: &egui::Ui, text: &str, width: f32) -> Arc<egui::Galley> {
    let font = egui::FontSelection::Default.resolve(ui.style());
    let color = ui.visuals().text_color();
    ui.fonts_mut(|fonts| {
        let mut measure = |text: String| {
            fonts.layout_job(egui::text::LayoutJob::simple_singleline(
                text,
                font.clone(),
                color,
            ))
        };
        let full = measure(text.to_owned());
        if full.size().x <= width {
            return full;
        }

        // Character boundaries keep foreign Unicode paths valid UTF-8. Search
        // by measured glyph width, rather than estimating from string length.
        let count = text.chars().count();
        let mut best = measure("…".into());
        let mut low = 0;
        let mut high = count.saturating_sub(1);
        while low < high {
            let keep = low + (high - low).div_ceil(2);
            let start: String = text.chars().take(keep.div_ceil(2)).collect();
            let end: String = text.chars().skip(count.saturating_sub(keep / 2)).collect();
            let candidate = measure(format!("{start}…{end}"));
            if candidate.size().x <= width {
                low = keep;
                best = candidate;
            } else {
                high = keep.saturating_sub(1);
            }
        }

        // A collapsed panel can be narrower than even the ellipsis. Leave the
        // row empty in that case, so the label never forces the panel to grow.
        if best.size().x > width {
            measure(String::new())
        } else {
            best
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use egui_kittest::{kittest::Queryable, Harness};

    #[test]
    fn labels_fit_measured_width_and_keep_both_ends_at_native_scales() {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            for scale in [1.0, 1.5, 2.0] {
                for text in [
                    "Path: /synthetic/a very long directory/another folder/Photo.jpg",
                    r"Path: C:\Photos\a very long directory\another folder\Photo.jpg",
                    "Path: /写真/長いフォルダーの名前/旅行の写真/雪景色.jpg",
                    "Path: /📷/夏休み/とても長いディレクトリ/🌅.jpg",
                ] {
                    let mut h = Harness::builder()
                        .with_theme(theme)
                        .with_pixels_per_point(scale)
                        .build_ui(|ui| {
                            crate::typography::configure(ui.ctx());
                            for width in [0.0, 1.0, 15.0, 80.0, 160.0, 300.0, 2000.0] {
                                let galley = layout(ui, text, width);
                                assert!(galley.size().x <= width, "{width}: {galley:?}");
                                assert_eq!(galley.rows.len(), 1);
                                if width == 2000.0 {
                                    assert_eq!(galley.text(), text);
                                } else if width >= 80.0 && galley.text() != text {
                                    let (start, end) = galley.text().split_once('…').unwrap();
                                    assert!(!start.is_empty() && !end.is_empty());
                                    assert!(text.starts_with(start) && text.ends_with(end));
                                }
                            }
                            label(ui, text);
                        });
                    h.run();
                    assert!(h.query_by_label(text).is_some());
                }
            }
        }
    }

    #[test]
    fn empty_short_and_multiline_values_stay_on_one_line() {
        let mut h = Harness::builder().build_ui(|ui| {
            for text in ["", "a", "雪", "/Photos/a.jpg", "line\nbreak\r\npath"] {
                for width in [0.0, 1.0, 20.0, 2000.0] {
                    let galley = layout(ui, text, width);
                    assert_eq!(galley.rows.len(), 1);
                    assert!(galley.size().x <= width);
                    if width == 2000.0 {
                        assert_eq!(galley.text(), text);
                    }
                }
            }
        });
        h.run();
    }
}
