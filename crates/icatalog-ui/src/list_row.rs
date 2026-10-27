//! List-row presentation and a viewport-sized cache of catalogue display data.
use eframe::egui;
use icatalog_catalog::{Annotation, Asset, AssetMeta, Catalog};

pub const HEIGHT: f32 = 122.0;
pub const MIN_WIDTH: f32 = 760.0;

pub struct Metadata {
    annotation: Option<Annotation>,
    meta: Option<AssetMeta>,
    color: Option<String>,
    profile: Option<String>,
}

impl Metadata {
    pub fn load(catalog: &Catalog, id: i64) -> icatalog_catalog::Result<Self> {
        let raw = catalog.metadata_raw_for_asset_scheme(id, "image_info")?;
        let value = |key| {
            raw.iter()
                .find(|m| m.scheme == "image_info" && m.key == key)
                .and_then(|m| std::str::from_utf8(&m.value).ok())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
        };
        Ok(Self {
            annotation: catalog.annotation(id)?,
            meta: catalog.asset_meta(id)?,
            color: value("color_model"),
            profile: value("icc_profile"),
        })
    }
}

pub fn draw(
    ui: &mut egui::Ui,
    asset: &Asset,
    texture: Option<&egui::TextureHandle>,
    metadata: Option<&Metadata>,
    selected: bool,
    index: usize,
) -> egui::Response {
    // Reserve a trailing gutter for rectangle selection and the vertical scrollbar.
    let width = (ui.available_width() - 24.0).max(MIN_WIDTH - 24.0);
    let (id, rect) = ui.allocate_space(egui::vec2(width, HEIGHT));
    let hovered = ui.rect_contains_pointer(rect);
    let visuals = if hovered {
        ui.visuals().widgets.hovered
    } else {
        ui.visuals().widgets.inactive
    };
    let fill = if selected {
        ui.visuals().selection.bg_fill
    } else if hovered {
        visuals.bg_fill
    } else if index.is_multiple_of(2) {
        ui.visuals().faint_bg_color
    } else {
        ui.visuals().panel_fill
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    if selected {
        ui.painter().rect_stroke(
            rect,
            4.0,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
    let inner = rect.shrink(10.0);
    let thumb = egui::Rect::from_center_size(
        egui::pos2(inner.left() + 48.0, inner.center().y),
        egui::vec2(96.0, 96.0),
    );
    ui.painter()
        .rect_filled(thumb, 3.0, ui.visuals().extreme_bg_color);
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        let scale = (thumb.width() / size.x.max(1.0)).min(thumb.height() / size.y.max(1.0));
        let size = size * scale;
        egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
            .paint_at(ui, egui::Rect::from_center_size(thumb.center(), size));
    } else {
        ui.painter().text(
            thumb.center(),
            egui::Align2::CENTER_CENTER,
            "No thumbnail",
            egui::FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
    }
    let x = thumb.right() + 16.0;
    let available = (inner.right() - x - 32.0).max(1.0);
    let widths = [available * 0.38, available * 0.34, available * 0.28];
    let meta = metadata.and_then(|m| m.meta.as_ref());
    let rating = metadata
        .and_then(|m| m.annotation.as_ref())
        .map_or(0, |a| a.rating)
        .clamp(0, 5);
    let stars: String = (0..5).map(|i| if i < rating { '★' } else { '☆' }).collect();
    let captured = meta
        .and_then(|m| m.captured_local.as_deref())
        .unwrap_or("—")
        .replace('T', " ");
    let filename = vec![
        asset.name.clone(),
        format!("Rating: {stars}"),
        format!("Captured: {captured}"),
        format!("Modified: {}", modified_date(asset.mtime)),
        format!(
            "Size: {}{}",
            file_size(asset.file_size),
            if asset.missing { " · Missing" } else { "" }
        ),
    ];
    let camera = meta
        .map(|m| {
            let make = m.make.as_deref().unwrap_or("").trim();
            let model = m.model.as_deref().unwrap_or("").trim();
            if model.to_lowercase().starts_with(&make.to_lowercase()) {
                model.to_owned()
            } else {
                format!("{make} {model}").trim().to_owned()
            }
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "—".into());
    let focal = meta
        .and_then(|m| m.focal_length_mm)
        .filter(|f| f.is_finite() && *f > 0.0)
        .map(|f| format!("{f} mm"))
        .unwrap_or_else(|| "—".into());
    let document = vec![
        format!(
            "Type: {}",
            asset
                .format
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("—")
                .to_uppercase()
        ),
        format!("Camera: {camera}"),
        format!(
            "Lens: {}",
            meta.and_then(|m| m.lens.as_deref())
                .filter(|s| !s.is_empty())
                .unwrap_or("—")
        ),
        format!("Focal length: {focal}"),
    ];
    let dimensions = match (asset.width, asset.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => format!("{w} × {h} px"),
        _ => "Dimensions: —".into(),
    };
    let image = vec![
        dimensions,
        format!(
            "Colour: {}",
            metadata.and_then(|m| m.color.as_deref()).unwrap_or("—")
        ),
        format!(
            "Profile: {}",
            metadata.and_then(|m| m.profile.as_deref()).unwrap_or("—")
        ),
    ];
    let mut left = x;
    for (width, lines) in widths.into_iter().zip([filename, document, image]) {
        let column = egui::Rect::from_min_size(
            egui::pos2(left, inner.top()),
            egui::vec2(width, inner.height()),
        );
        // The full row has already advanced the parent cursor. A scope would
        // advance it again to the shorter column and make successive rows overlap.
        let mut column_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(column)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        {
            let ui = &mut column_ui;
            ui.set_clip_rect(column.intersect(ui.clip_rect()));
            ui.spacing_mut().item_spacing.y = 3.0;
            for (i, line) in lines.iter().enumerate() {
                let text = egui::RichText::new(line).size(12.0).color(if selected {
                    ui.visuals().selection.stroke.color
                } else if i == 0 {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().text_color().gamma_multiply(0.8)
                });
                ui.add(
                    egui::Label::new(if i == 0 { text.strong() } else { text })
                        .truncate()
                        .sense(egui::Sense::hover()),
                )
                .on_hover_text(line);
            }
        }
        left += width + 16.0;
    }
    // Register the row after its display-only children, so the whole row owns
    // clicks and drags even where a metadata label or child UI is underneath.
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            &asset.name,
        )
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn file_size(size: Option<i64>) -> String {
    let Some(size) = size.filter(|s| *s >= 0) else {
        return "—".into();
    };
    if size < 1024 {
        return format!("{size} B");
    }
    let mut value = size as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit}")
}

fn modified_date(seconds: Option<i64>) -> String {
    let Some(seconds) = seconds.filter(|s| (-62_135_596_800..=253_402_300_799).contains(s)) else {
        return "—".into();
    };
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let rem = seconds.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_and_dates_handle_missing_and_hostile_values() {
        assert_eq!(file_size(None), "—");
        assert_eq!(file_size(Some(-1)), "—");
        assert_eq!(file_size(Some(0)), "0 B");
        assert_eq!(file_size(Some(1_572_864)), "1.5 MiB");
        assert_eq!(modified_date(Some(0)), "1970-01-01 00:00:00 UTC");
        assert_eq!(modified_date(Some(-1)), "1969-12-31 23:59:59 UTC");
        assert_eq!(modified_date(Some(951_782_400)), "2000-02-29 00:00:00 UTC");
        for seconds in [None, Some(i64::MIN), Some(i64::MAX)] {
            assert_eq!(modified_date(seconds), "—");
        }
    }
}
