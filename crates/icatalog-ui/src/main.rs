//! The `icatalog-ui` native egui shell: a thin, data-driven desktop client.
//! Browsing reads the local catalogue; every action (init/import/update) is
//! dispatched as an engine command, so the shell holds no logic of its own.
//! Desktop only for now; a wasm build is a later goal (AGENTS M4).
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use eframe::egui;
use icatalog_catalog::{Annotation, Asset, AssetQuery};
use icatalog_engine::{dispatch_guarded, Context, Params};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match std::panic::catch_unwind(run) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(e)) => {
            eprintln!("icatalog-ui: {e}");
            ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!(
                "icatalog-ui: internal error (panic caught); the catalogue was left unchanged"
            );
            ExitCode::from(70)
        }
    }
}

fn run() -> eframe::Result<()> {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("catalogue.sqlite"));
    let mut ctx = match Context::open(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("icatalog-ui: cannot open {}: {e}", path.display());
            return Ok(());
        }
    };
    // A brand-new path is initialised so the window is usable immediately.
    if ctx.catalog().catalogue().ok().flatten().is_none() {
        let _ = dispatch_guarded(&mut ctx, "init", &Params::new());
    }
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "iCatalog",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, ctx, path)))),
    )
}

enum Action {
    Import(PathBuf),
    Update,
}

struct App {
    ctx: Context,
    path: PathBuf,
    assets: Vec<Asset>,
    selected: Option<i64>,
    texture: Option<egui::TextureHandle>,
    texture_for: Option<i64>,
    annotation: Option<Annotation>,
    keywords: Vec<String>,
    keyword_input: String,
    detail_for: Option<i64>,
    status: String,
    dirty: bool,
}

impl App {
    fn new(_cc: &eframe::CreationContext<'_>, ctx: Context, path: PathBuf) -> Self {
        let mut app = App {
            ctx,
            path,
            assets: Vec::new(),
            selected: None,
            texture: None,
            texture_for: None,
            annotation: None,
            keywords: Vec::new(),
            keyword_input: String::new(),
            detail_for: None,
            status: String::new(),
            dirty: true,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.assets = self
            .ctx
            .catalog()
            .query_assets(&AssetQuery::default())
            .unwrap_or_default();
        if let Some(id) = self.selected {
            if !self.assets.iter().any(|a| a.id == id) {
                self.selected = None;
            }
        }
    }

    fn run_command(&mut self, id: &str, params: &Params) {
        self.status = match dispatch_guarded(&mut self.ctx, id, params) {
            Ok(o) => o.render(),
            Err(e) => e.to_string(),
        };
        self.dirty = true;
        self.texture_for = None;
        self.detail_for = None;
    }

    /// Load the selected asset's annotation and keywords once per selection.
    fn ensure_detail(&mut self) {
        let Some(id) = self.selected else {
            self.detail_for = None;
            self.annotation = None;
            self.keywords.clear();
            return;
        };
        if self.detail_for == Some(id) {
            return;
        }
        self.detail_for = Some(id);
        self.annotation = self.ctx.catalog().annotation(id).ok().flatten();
        self.keywords = self
            .ctx
            .catalog()
            .asset_keywords(id)
            .map(|v| v.into_iter().map(|k| k.name).collect())
            .unwrap_or_default();
        self.keyword_input.clear();
    }

    fn list(&self, ui: &mut egui::Ui) -> Option<i64> {
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for a in &self.assets {
                    if ui
                        .selectable_label(self.selected == Some(a.id), &a.name)
                        .clicked()
                    {
                        clicked = Some(a.id);
                    }
                }
            });
        clicked
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        let Some(id) = self.selected else {
            self.texture = None;
            self.texture_for = None;
            return;
        };
        if self.texture_for == Some(id) {
            return;
        }
        self.texture_for = Some(id);
        self.texture = self.load_thumbnail(ctx, id);
    }

    fn load_thumbnail(&self, ctx: &egui::Context, id: i64) -> Option<egui::TextureHandle> {
        let t = self
            .ctx
            .catalog()
            .thumbnail(id, i64::from(icatalog_media::GRID_LONG_EDGE))
            .ok()
            .flatten()?;
        let img = icatalog_media::decode_rgba(&t.bytes).ok()?;
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width as usize, img.height as usize],
            &img.rgba,
        );
        Some(ctx.load_texture(format!("thumb-{id}"), color, egui::TextureOptions::LINEAR))
    }

    fn central(&mut self, ui: &mut egui::Ui) -> Option<Params> {
        let Some(id) = self.selected else {
            ui.centered_and_justified(|ui| ui.label("Select an asset in the list"));
            return None;
        };
        let asset = self.assets.iter().find(|a| a.id == id).cloned()?;
        let mut edit: Option<Params> = None;
        ui.heading(&asset.name);
        if let Some(tex) = &self.texture {
            let size = tex.size_vec2();
            let scale = (ui.available_width() / size.x.max(1.0)).min(1.0);
            ui.add(egui::Image::new(egui::load::SizedTexture::new(
                tex.id(),
                size * scale,
            )));
        } else {
            ui.label("(no thumbnail)");
        }
        ui.separator();
        ui.label(format!("Path: {}", asset.path));
        let dims = match (asset.width, asset.height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "dimensions unknown".to_string(),
        };
        ui.label(format!(
            "Format: {}  {}",
            asset.format.as_deref().unwrap_or("?"),
            dims
        ));
        ui.label(format!("Size: {} bytes", asset.file_size.unwrap_or(0)));
        if asset.missing {
            ui.colored_label(egui::Color32::RED, "missing from disk (run Update)");
        }

        ui.separator();
        ui.heading("Annotations");
        let rating = self.annotation.as_ref().map(|a| a.rating).unwrap_or(0);
        let label = self.annotation.as_ref().map(|a| a.label).unwrap_or(0);
        ui.horizontal(|ui| {
            ui.label("Rating");
            for r in 0..=5 {
                if ui.selectable_label(rating == r, r.to_string()).clicked() && rating != r {
                    edit = Some(edit_params(id, "rating", &r.to_string()));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Label");
            for l in 0..=7 {
                if ui.selectable_label(label == l, l.to_string()).clicked() && label != l {
                    edit = Some(edit_params(id, "label", &l.to_string()));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Keyword");
            ui.add(
                egui::TextEdit::singleline(&mut self.keyword_input)
                    .hint_text("add")
                    .desired_width(150.0),
            );
            if ui.button("Add").clicked() {
                let name = self.keyword_input.trim().to_owned();
                if !name.is_empty() {
                    edit = Some(edit_params(id, "keyword", &name));
                    self.keyword_input.clear();
                }
            }
            if !self.keywords.is_empty() && ui.button("Clear all").clicked() {
                edit = Some(edit_params(id, "clear-keywords", "true"));
            }
        });
        let keywords = self.keywords.clone();
        for kw in &keywords {
            ui.horizontal(|ui| {
                ui.label(kw);
                if ui.small_button("remove").clicked() {
                    edit = Some(edit_params(id, "remove-keyword", kw));
                }
            });
        }
        edit
    }
}

/// Build `edit` params for a single change to one asset.
fn edit_params(id: i64, key: &str, value: &str) -> Params {
    let mut p = Params::new();
    p.set("id", id.to_string());
    p.set(key, value.to_owned());
    p
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.dirty {
            self.refresh();
            self.dirty = false;
        }
        let ctx = ui.ctx().clone();

        let mut action = None;
        egui::Panel::left("assets")
            .default_size(260.0)
            .show(ui, |ui| {
                ui.heading("iCatalog");
                ui.label(self.path.display().to_string());
                ui.horizontal(|ui| {
                    if ui.button("Import…").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            action = Some(Action::Import(dir));
                        }
                    }
                    if ui.button("Update").clicked() {
                        action = Some(Action::Update);
                    }
                });
                ui.separator();
                ui.label(format!("{} assets", self.assets.len()));
                if let Some(id) = self.list(ui) {
                    self.selected = Some(id);
                }
            });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(if self.status.is_empty() {
                "Ready".to_string()
            } else {
                self.status.clone()
            });
        });

        if let Some(a) = action {
            match a {
                Action::Import(dir) => {
                    let mut p = Params::new();
                    p.set("path", dir.display().to_string());
                    self.run_command("import", &p);
                }
                Action::Update => self.run_command("update", &Params::new()),
            }
        }

        self.ensure_texture(&ctx);
        self.ensure_detail();
        let mut edit = None;
        egui::CentralPanel::default().show(ui, |ui| {
            edit = self.central(ui);
        });
        if let Some(params) = edit {
            self.run_command("edit", &params);
        }
    }
}
