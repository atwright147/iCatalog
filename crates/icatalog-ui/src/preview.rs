//! Stored-preview presentation. Verification and decoding stay in the engine.
use eframe::egui;
use icatalog_engine::{Outcome, Params, PreviewImage};
use std::path::Path;

#[derive(Default)]
pub(super) struct Viewer {
    open: bool,
    worker: Option<super::worker::Worker>,
    image: Option<Loaded>,
    error: Option<String>,
    actual_size: bool,
}

struct Loaded {
    filename: String,
    texture: egui::TextureHandle,
    pixels: egui::Vec2,
}

impl Viewer {
    #[cfg(test)]
    pub(super) fn blocked_for_test(release: std::sync::mpsc::Receiver<()>) -> Self {
        Self {
            open: true,
            worker: Some(super::worker::Worker::blocked_for_test(release)),
            ..Default::default()
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn is_loading(&self) -> bool {
        self.worker.is_some()
    }

    pub(super) fn start(&mut self, path: &Path, asset_id: i64) {
        // Retain a dismissed worker until it finishes: rapid open/close gestures
        // must never accumulate decode threads or let a stale result appear.
        if self.worker.is_some() || self.open {
            return;
        }
        self.open = true;
        self.image = None;
        self.error = None;
        self.actual_size = false;
        let mut params = Params::new();
        params.set("id", asset_id.to_string());
        match super::worker::Worker::start(path.to_owned(), "preview".into(), params) {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => self.error = Some(format!("Cannot load preview: {error}")),
        }
    }

    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        if let Some(result) = self.worker.as_mut().and_then(super::worker::Worker::poll) {
            self.worker = None;
            if self.open {
                let image = result
                    .map_err(|error| error.to_string())
                    .and_then(|outcome| match outcome {
                        Outcome::Preview(image) => load_texture(ctx, image),
                        _ => Err("Cannot read preview response".into()),
                    });
                match image {
                    Ok(image) => self.image = Some(image),
                    Err(error) => self.error = Some(error),
                }
            }
        }
        if self.worker.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
    }

    pub(super) fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let screen = ctx.content_rect().size();
        let width = (screen.x - 64.0).clamp(160.0, 1000.0);
        let height = (screen.y - 190.0).clamp(80.0, 700.0);
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("asset_preview"))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(12))
            .show(ctx, |ui| {
                super::spacing::dialog(ui);
                ui.set_width(width);
                close = super::dialog::header(ui, "Preview", true);
                if let Some(image) = &self.image {
                    ui.add(egui::Label::new(&image.filename).truncate())
                        .on_hover_text(&image.filename);
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.actual_size, false, "Fit");
                        ui.selectable_value(&mut self.actual_size, true, "100%")
                            .on_hover_text("One image pixel per screen pixel");
                        ui.label(format!(
                            "{} × {} pixels",
                            image.pixels.x as u32, image.pixels.y as u32
                        ));
                    });
                    egui::ScrollArea::both()
                        .id_salt("preview_image")
                        .auto_shrink([false, false])
                        .max_height(height)
                        .min_scrolled_height(height)
                        .show(ui, |ui| {
                            let available = egui::vec2(ui.available_width(), height);
                            let size = if self.actual_size {
                                image.pixels / ctx.pixels_per_point()
                            } else {
                                let scale = (available.x / image.pixels.x)
                                    .min(available.y / image.pixels.y)
                                    .min(1.0 / ctx.pixels_per_point());
                                image.pixels * scale
                            };
                            ui.add(
                                egui::Image::new((image.texture.id(), size))
                                    .alt_text("Stored preview"),
                            );
                        });
                } else if let Some(error) = &self.error {
                    ui.strong("Preview unavailable");
                    egui::ScrollArea::vertical()
                        .max_height(height)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(error).wrap().selectable(true));
                        });
                } else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading stored preview…");
                    });
                }
                ui.separator();
                close |= ui.button("Close").clicked();
            });
        if close || response.should_close() {
            self.dismiss();
        }
    }
    fn dismiss(&mut self) {
        self.open = false;
        self.image = None;
        self.error = None;
    }
}

fn load_texture(ctx: &egui::Context, image: PreviewImage) -> Result<Loaded, String> {
    // ColorImage requires exact RGBA geometry. Guard that boundary even though
    // the engine already validates its typed response before returning it.
    let width = usize::try_from(image.width).map_err(|_| "Invalid preview width")?;
    let height = usize::try_from(image.height).map_err(|_| "Invalid preview height")?;
    if width == 0
        || height == 0
        || width > 1920
        || height > 1920
        || width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(image.rgba.len())
    {
        return Err("Invalid preview pixel dimensions".into());
    }
    let pixels = egui::vec2(image.width as f32, image.height as f32);
    let texture = ctx.load_texture(
        "stored-preview",
        egui::ColorImage::from_rgba_unmultiplied([width, height], &image.rgba),
        egui::TextureOptions::LINEAR,
    );
    Ok(Loaded {
        filename: image.filename,
        texture,
        pixels,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn invalid_typed_pixels_cannot_panic_in_texture_upload() {
        let ctx = egui::Context::default();
        for (width, height, rgba) in [(0, 1, vec![]), (1921, 1, vec![]), (2, 2, vec![0; 4])] {
            assert!(load_texture(
                &ctx,
                PreviewImage {
                    asset_id: 1,
                    filename: "Synthetic.jpg".into(),
                    hash: String::new(),
                    width,
                    height,
                    rgba,
                },
            )
            .is_err());
        }
    }

    #[test]
    fn dismissing_a_load_keeps_one_worker_and_discards_its_result() {
        let (release, receiver) = std::sync::mpsc::channel();
        let mut viewer = Viewer {
            open: true,
            worker: Some(super::super::worker::Worker::blocked_for_test(receiver)),
            ..Default::default()
        };
        viewer.dismiss();
        viewer.start(Path::new("must-not-open.sqlite"), 2);
        assert!(!viewer.is_open());
        assert!(viewer.is_loading());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let ctx = egui::Context::default();
        while viewer.is_loading() {
            assert!(std::time::Instant::now() < deadline);
            viewer.poll(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!viewer.is_open());
        assert!(viewer.image.is_none());
        assert!(viewer.error.is_none());
    }
}
