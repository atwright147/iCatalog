//! Presentation and parameter transport only; rule validation lives below UI.
use eframe::egui;
use icatalog_catalog::SmartRules;
use icatalog_engine::{Outcome, Params};
use std::time::{Duration, Instant};

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct RuleInputs {
    format: String,
    from: String,
    to: String,
    camera: String,
    lens: String,
    rating: Option<i64>,
    max_rating: Option<i64>,
    label: String,
    keyword: String,
    text: String,
}

/// One background read at a time. Changed inputs immediately hide old results;
/// an older in-flight result can never overwrite the current draft's preview.
#[derive(Default)]
pub(super) struct RulePreview {
    inputs: Option<RuleInputs>,
    running_inputs: Option<RuleInputs>,
    worker: Option<super::worker::Worker>,
    changed_at: Option<Instant>,
    result: Option<Result<(String, String), String>>,
}

impl RulePreview {
    #[cfg(test)]
    pub(super) fn is_running(&self) -> bool {
        self.worker.is_some()
    }

    pub(super) fn is_pending(&self) -> bool {
        self.result.is_none()
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui, path: &std::path::Path, inputs: &RuleInputs) {
        if self.inputs.as_ref() != Some(inputs) {
            self.inputs = Some(inputs.clone());
            self.changed_at = Some(Instant::now());
            self.result = None;
        }
        if let Some(result) = self.worker.as_mut().and_then(super::worker::Worker::poll) {
            self.worker = None;
            if self.running_inputs.as_ref() == Some(inputs) {
                self.result = Some(result.map_err(|e| e.to_string()).and_then(|outcome| {
                    if let Outcome::Text(text) = outcome {
                        if let Some((count, summary)) = text.split_once('\n') {
                            return Ok((count.to_owned(), summary.to_owned()));
                        }
                    }
                    Err("Cannot read rule preview response".into())
                }));
            }
        }
        if self.worker.is_none()
            && self.is_pending()
            && self
                .changed_at
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(200))
        {
            let mut params = Params::new();
            params.set("action", "preview-smart");
            inputs.apply(&mut params);
            match super::worker::Worker::start(path.to_owned(), "collection".into(), params) {
                Ok(worker) => {
                    self.worker = Some(worker);
                    self.running_inputs = Some(inputs.clone());
                }
                Err(error) => self.result = Some(Err(format!("Cannot preview rules: {error}"))),
            }
        }
        ui.separator();
        // Bound long summaries/errors so every field and the buttons remain
        // reachable in an 800x600 window. The full text is selectable/scrollable.
        egui::ScrollArea::vertical()
            .id_salt("rule_preview")
            .max_height(64.0)
            .min_scrolled_height(64.0)
            .auto_shrink([false, false])
            .show(ui, |ui| match &self.result {
                Some(Ok((count, summary))) => {
                    ui.strong(count);
                    ui.add(egui::Label::new(summary).wrap().selectable(true));
                }
                Some(Err(error)) => {
                    ui.label("Preview unavailable");
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                None => {
                    ui.label("Checking matches…");
                }
            });
        ui.label("Across the whole catalogue. All filled rules must match.");
        if self.is_pending() {
            ui.ctx().request_repaint_after(Duration::from_millis(25));
        }
    }
}

impl RuleInputs {
    pub(super) fn label_column_width(ui: &egui::Ui) -> f32 {
        // Share the widest rule caption with the separate Name row.
        ui.painter()
            .layout_no_wrap(
                "Filename or Path Contains".into(),
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().text_color(),
            )
            .size()
            .x
    }

    pub(super) fn from_rules(r: &SmartRules) -> Self {
        Self {
            format: r.format.clone().unwrap_or_default(),
            from: r.captured_from.clone().unwrap_or_default(),
            to: r.captured_to.clone().unwrap_or_default(),
            camera: r.camera.clone().unwrap_or_default(),
            lens: r.lens.clone().unwrap_or_default(),
            rating: r.min_rating,
            max_rating: r.max_rating,
            label: r.label.map(|v| v.to_string()).unwrap_or_default(),
            keyword: r.keyword.clone().unwrap_or_default(),
            text: r.text.clone().unwrap_or_default(),
        }
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        ui.label("Match all filled rules. Blank fields match any value.");
        egui::Grid::new("smart_rule_fields")
            .num_columns(2)
            .min_col_width(Self::label_column_width(ui))
            .spacing(egui::vec2(16.0, 4.0))
            .show(ui, |ui| {
                for (label, value, hint) in [
                    ("Format", &mut self.format, "JPEG, NEF, PNG, …"),
                    ("Captured From", &mut self.from, "YYYY-MM-DD"),
                    ("Captured To", &mut self.to, "YYYY-MM-DD (inclusive)"),
                    ("Camera Contains", &mut self.camera, "Camera make or model"),
                    ("Lens Contains", &mut self.lens, "Lens name"),
                ] {
                    field(ui, label, value, hint);
                }
                let label = ui.label("Minimum Rating");
                super::rating::editor(ui, &mut self.rating, "Minimum Rating").labelled_by(label.id);
                ui.end_row();
                let label = ui.label("Maximum Rating");
                super::rating::editor(ui, &mut self.max_rating, "Maximum Rating")
                    .labelled_by(label.id);
                ui.end_row();
                for (label, value, hint) in [
                    ("Label Number", &mut self.label, "0–7"),
                    ("Keyword", &mut self.keyword, "Exact keyword name"),
                    ("Filename or Path Contains", &mut self.text, "Text"),
                ] {
                    field(ui, label, value, hint);
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
            ("label", &self.label),
            ("keyword", &self.keyword),
            ("text", &self.text),
        ] {
            if !value.trim().is_empty() {
                params.set(key, value.trim());
            }
        }
        if let Some(rating) = self.rating {
            params.set("min-rating", rating.to_string());
        }
        if let Some(rating) = self.max_rating {
            params.set("max-rating", rating.to_string());
        }
    }
}

fn field(ui: &mut egui::Ui, caption: &str, value: &mut String, hint: &str) {
    let label = ui.label(caption);
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .char_limit(1024)
            .desired_width(240.0),
    )
    .labelled_by(label.id);
    ui.end_row();
}
