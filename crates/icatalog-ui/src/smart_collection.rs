//! Presentation and parameter transport only; rule validation lives below UI.
use eframe::egui;
use icatalog_catalog::SmartRules;
use icatalog_engine::Params;

#[derive(Default)]
pub(super) struct RuleInputs {
    format: String,
    from: String,
    to: String,
    camera: String,
    lens: String,
    rating: String,
    label: String,
    keyword: String,
    text: String,
}

impl RuleInputs {
    pub(super) fn from_rules(r: &SmartRules) -> Self {
        Self {
            format: r.format.clone().unwrap_or_default(),
            from: r.captured_from.clone().unwrap_or_default(),
            to: r.captured_to.clone().unwrap_or_default(),
            camera: r.camera.clone().unwrap_or_default(),
            lens: r.lens.clone().unwrap_or_default(),
            rating: r.min_rating.map(|v| v.to_string()).unwrap_or_default(),
            label: r.label.map(|v| v.to_string()).unwrap_or_default(),
            keyword: r.keyword.clone().unwrap_or_default(),
            text: r.text.clone().unwrap_or_default(),
        }
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        ui.label("Match all filled rules. Blank fields match any value.");
        egui::Grid::new("smart_rule_fields")
            .num_columns(2)
            .show(ui, |ui| {
                for (label, value, hint) in [
                    ("Format", &mut self.format, "JPEG, NEF, PNG, …"),
                    ("Captured From", &mut self.from, "YYYY-MM-DD"),
                    ("Captured To", &mut self.to, "YYYY-MM-DD (inclusive)"),
                    ("Camera Contains", &mut self.camera, "Camera make or model"),
                    ("Lens Contains", &mut self.lens, "Lens name"),
                    ("Minimum Rating", &mut self.rating, "0–5"),
                    ("Label Number", &mut self.label, "0–7"),
                    ("Keyword", &mut self.keyword, "Exact keyword name"),
                    ("Filename or Path Contains", &mut self.text, "Text"),
                ] {
                    let label = ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .hint_text(hint)
                            .char_limit(1024)
                            .desired_width(240.0),
                    )
                    .labelled_by(label.id);
                    ui.end_row();
                }
            });
    }

    pub(super) fn apply(&self, params: &mut Params) {
        for (key, value) in [
            ("format", &self.format),
            ("captured-from", &self.from),
            ("captured-to", &self.to),
            ("camera", &self.camera),
            ("lens", &self.lens),
            ("min-rating", &self.rating),
            ("label", &self.label),
            ("keyword", &self.keyword),
            ("text", &self.text),
        ] {
            if !value.trim().is_empty() {
                params.set(key, value.trim());
            }
        }
    }
}
