//! Stored-preview presentation. Verification and decoding stay in the engine.
use eframe::egui;
use icatalog_engine::{Outcome, Params, PreviewImage};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct Viewer {
    open: bool,
    worker: Option<super::worker::Worker>,
    image: Option<Loaded>,
    error: Option<String>,
    actual_size: bool,
    fetch: Option<FetchSource>,
    fetching: bool,
}

pub(super) struct FetchSource {
    pub path: PathBuf,
    pub asset_id: i64,
    pub hash: String,
    pub url: String,
    pub token_file: Option<PathBuf>,
    pub ca_file: Option<PathBuf>,
    pub disabled_reason: Option<String>,
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

    #[cfg(test)]
    pub(super) fn set_fetch_files_for_test(&mut self, token: Option<PathBuf>, ca: Option<PathBuf>) {
        if let Some(source) = &mut self.fetch {
            source.token_file = token;
            source.ca_file = ca;
        }
    }

    #[cfg(test)]
    pub(super) fn block_fetch_for_test(&mut self, release: std::sync::mpsc::Receiver<()>) {
        self.worker = Some(super::worker::Worker::blocked_for_test(release));
        self.fetching = true;
        self.error = None;
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn is_loading(&self) -> bool {
        self.worker.is_some()
    }

    pub(super) fn is_fetching(&self) -> bool {
        self.fetching && self.worker.is_some()
    }

    pub(super) fn start(&mut self, path: &Path, asset_id: i64, fetch: Option<FetchSource>) {
        // Retain a dismissed worker until it finishes: rapid open/close gestures
        // must never accumulate decode threads or let a stale result appear.
        if self.worker.is_some() || self.open {
            return;
        }
        self.open = true;
        self.image = None;
        self.error = None;
        self.actual_size = false;
        self.fetch = fetch;
        self.fetching = false;
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
            if self.fetching {
                self.fetching = false;
                if let Err(error) = result {
                    self.error = Some(format!("Cannot fetch preview: {error}"));
                } else if let Some(source) = &self.fetch {
                    let mut params = Params::new();
                    params.set("id", source.asset_id.to_string());
                    match super::worker::Worker::start(
                        source.path.clone(),
                        "preview".into(),
                        params,
                    ) {
                        Ok(worker) => self.worker = Some(worker),
                        Err(error) => {
                            self.error = Some(format!("Cannot load fetched preview: {error}"))
                        }
                    }
                }
            } else if self.open {
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
        let mut fetch = false;
        let fetching = self.is_fetching();
        let response = egui::Modal::new(egui::Id::new("asset_preview"))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(12))
            .show(ctx, |ui| {
                super::spacing::dialog(ui);
                ui.set_width(width);
                close = super::dialog::header(ui, "Preview", !fetching);
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
                        .max_height(if self.fetch.is_some() {
                            (height - 180.0).max(60.0)
                        } else {
                            height
                        })
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(error).wrap().selectable(true));
                        });
                    if let Some(source) = &mut self.fetch {
                        ui.separator();
                        ui.add(egui::Label::new(format!("Server: {}", source.url)).truncate());
                        ui.add_enabled_ui(source.disabled_reason.is_none(), |ui| {
                            egui::CollapsingHeader::new("Security settings").show(ui, |ui| {
                                super::sync_ui::security_file(
                                    ui,
                                    "Choose token file…",
                                    "Clear token file",
                                    &mut source.token_file,
                                );
                                super::sync_ui::security_file(
                                    ui,
                                    "Choose CA file…",
                                    "Clear CA file",
                                    &mut source.ca_file,
                                );
                                ui.small("These selections apply only to this preview dialog.");
                            });
                            fetch = ui.button("Fetch preview").clicked();
                        });
                        if let Some(reason) = &source.disabled_reason {
                            ui.add(egui::Label::new(reason).wrap());
                        }
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(if fetching {
                            "Fetching preview from server…"
                        } else {
                            "Loading stored preview…"
                        });
                    });
                }
                ui.separator();
                close |= ui
                    .add_enabled(!fetching, egui::Button::new("Close"))
                    .clicked();
            });
        if !fetching && (close || response.should_close()) {
            self.dismiss();
        } else if fetch {
            self.start_fetch();
        }
    }
    fn start_fetch(&mut self) {
        let Some(source) = &self.fetch else {
            return;
        };
        if self.worker.is_some() || source.disabled_reason.is_some() {
            return;
        }
        let prepared = (|| -> Result<Params, String> {
            if source.url.len() > 8192 {
                return Err("server address exceeds 8 KiB".into());
            }
            icatalog_sync::validate_server_url(&source.url).map_err(|error| error.to_string())?;
            let mut params = Params::new();
            params.set("url", &source.url);
            params.set("preview", &source.hash);
            for (key, path) in [
                ("token-file", source.token_file.as_deref()),
                ("ca-file", source.ca_file.as_deref()),
            ] {
                if let Some(path) = path {
                    let value = path
                        .to_str()
                        .ok_or_else(|| "Security file path must be valid UTF-8".to_owned())?;
                    if value.len() > 8192 {
                        return Err("Security file path exceeds 8 KiB".into());
                    }
                    params.set(key, value);
                }
            }
            Ok(params)
        })();
        match prepared.and_then(|params| {
            super::worker::Worker::start(source.path.clone(), "sync".into(), params)
                .map_err(|error| error.to_string())
        }) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.fetching = true;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Cannot fetch preview: {error}")),
        }
    }

    fn dismiss(&mut self) {
        self.open = false;
        self.image = None;
        self.error = None;
        self.fetch = None;
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
        viewer.start(Path::new("must-not-open.sqlite"), 2, None);
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
