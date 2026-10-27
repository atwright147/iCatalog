//! Shared desktop typography. Keep logical sizes independent of monitor DPI;
//! eframe supplies the native scale and egui rasterizes at that resolution.
use eframe::egui;

pub fn configure(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        for (role, size) in [
            (egui::TextStyle::Body, 15.0),
            (egui::TextStyle::Button, 15.0),
            (egui::TextStyle::Small, 13.0),
            (egui::TextStyle::Monospace, 14.0),
            (egui::TextStyle::Heading, 20.0),
        ] {
            if let Some(font) = style.text_styles.get_mut(&role) {
                font.size = size;
            }
        }
        style.spacing.interact_size.y = 24.0;
        let visuals = &mut style.visuals;
        // Hint glyphs to physical pixels and avoid fractional glyph variants,
        // which can soften small text. Preserve theme-specific atlas gamma.
        visuals.text_options.font_hinting = true;
        visuals.text_options.subpixel_binning = false;
        let (text, secondary, selected, selection_fill) = if visuals.dark_mode {
            (
                egui::Color32::from_gray(225),
                egui::Color32::from_gray(185),
                egui::Color32::WHITE,
                egui::Color32::from_rgb(28, 72, 110),
            )
        } else {
            (
                egui::Color32::from_gray(30),
                egui::Color32::from_gray(80),
                egui::Color32::from_gray(20),
                egui::Color32::from_rgb(205, 226, 248),
            )
        };
        visuals.weak_text_color = Some(secondary);
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.fg_stroke.color = text;
        }
        visuals.selection.bg_fill = selection_fill;
        visuals.selection.stroke.color = selected;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
        let luminance = |color: egui::Color32| {
            let linear = |channel: u8| {
                let value = f32::from(channel) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn normal_secondary_and_selected_text_have_readable_contrast_in_both_themes() {
        let ctx = egui::Context::default();
        configure(&ctx);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let style = ctx.style_of(theme);
            let visuals = &style.visuals;
            for background in [
                visuals.panel_fill,
                visuals.window_fill,
                visuals.extreme_bg_color,
                visuals.widgets.inactive.bg_fill,
                visuals.widgets.hovered.bg_fill,
            ] {
                for foreground in [visuals.text_color(), visuals.weak_text_color()] {
                    assert!(
                        contrast(foreground, background) >= 4.5,
                        "{theme:?}: {foreground:?} on {background:?}"
                    );
                }
            }
            assert!(contrast(visuals.selection.stroke.color, visuals.selection.bg_fill) >= 4.5);
        }
    }
}
