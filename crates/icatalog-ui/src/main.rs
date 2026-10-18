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
use icatalog_catalog::{Annotation, Asset, AssetMeta, AssetQuery, Folder};
use icatalog_engine::{dispatch_guarded, Context, Params};
use std::collections::{HashMap, HashSet};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    List,
    Grid,
}

enum Action {
    Import(PathBuf),
    Update,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderSelection {
    All,
    Unassigned,
    Folder(i64),
}

struct App {
    ctx: Context,
    path: PathBuf,
    assets: Vec<Asset>,
    folders: Vec<Folder>,
    folder_sel: FolderSelection,
    expanded: HashSet<i64>,
    view_mode: ViewMode,
    selected: Option<i64>,
    textures: HashMap<i64, egui::TextureHandle>,
    annotation: Option<Annotation>,
    keywords: Vec<String>,
    keyword_input: String,
    meta: Option<AssetMeta>,
    detail_for: Option<i64>,
    status: String,
    dirty: bool,
    search: String,
}

impl App {
    fn new(_cc: &eframe::CreationContext<'_>, ctx: Context, path: PathBuf) -> Self {
        let mut app = App {
            ctx,
            path,
            assets: Vec::new(),
            folders: Vec::new(),
            folder_sel: FolderSelection::All,
            expanded: HashSet::new(),
            view_mode: ViewMode::Grid,
            selected: None,
            textures: HashMap::new(),
            annotation: None,
            keywords: Vec::new(),
            keyword_input: String::new(),
            meta: None,
            detail_for: None,
            status: String::new(),
            dirty: true,
            search: String::new(),
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        let q = AssetQuery {
            folder_id: match self.folder_sel {
                FolderSelection::Folder(fid) => Some(fid),
                _ => None,
            },
            folder_id_null: match self.folder_sel {
                FolderSelection::Unassigned => Some(true),
                _ => None,
            },
            text_like: if self.search.trim().is_empty() { None } else { Some(self.search.trim().to_string()) },
            ..Default::default()
        };
        self.assets = self
            .ctx
            .catalog()
            .query_assets(&q)
            .unwrap_or_default();
        self.folders = self.ctx.catalog().all_folders().unwrap_or_default();
        if let Some(id) = self.selected {
            if !self.assets.iter().any(|a| a.id == id) {
                self.selected = None;
            }
        }
        // keep cached textures only for assets that are still in view
        let ids: HashSet<i64> = self.assets.iter().map(|a| a.id).collect();
        self.textures.retain(|k, _| ids.contains(k));
    }

    fn run_command(&mut self, id: &str, params: &Params) {
        self.status = match dispatch_guarded(&mut self.ctx, id, params) {
            Ok(o) => o.render(),
            Err(e) => e.to_string(),
        };
        self.dirty = true;
        self.textures.clear();
        self.detail_for = None;
    }

    /// Load the selected asset's annotation, keywords and metadata once per selection.
    fn ensure_detail(&mut self) {
        let Some(id) = self.selected else {
            self.detail_for = None;
            self.annotation = None;
            self.keywords.clear();
            self.meta = None;
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
        self.meta = self.ctx.catalog().asset_meta(id).ok().flatten();
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
            return;
        };
        if self.textures.contains_key(&id) {
            return;
        }
        if let Some(tex) = self.load_thumbnail(ctx, id) {
            self.textures.insert(id, tex);
        }
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

    fn grid(&mut self, ui: &mut egui::Ui) -> Option<i64> {
        let mut clicked = None;
        let tile = 160.0f32;
        let label_h = 20.0f32;
        let spacing = 8.0f32;
        let available = ui.available_width();
        let cols = ((available / (tile + spacing)).floor() as usize).max(1);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("grid")
                    .num_columns(cols)
                    .spacing([spacing, spacing])
                    .show(ui, |ui| {
                        let mut i = 0;
                        for a in &self.assets {
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(tile, tile + label_h),
                                egui::Sense::click(),
                            );
                            draw_tile(
                                ui,
                                rect,
                                self.textures.get(&a.id),
                                &a.name,
                                self.selected == Some(a.id),
                            );
                            if resp.clicked() {
                                clicked = Some(a.id);
                            }
                            if resp.hovered() {
                                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                            }
                            i += 1;
                            if i % cols == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
        clicked
    }

    // removed unused folder_tree; tree is built inline in left panel

    fn details_panel(&mut self, ui: &mut egui::Ui) -> Option<Params> {
        let Some(id) = self.selected else {
            ui.centered_and_justified(|ui| ui.label("Select an asset"));
            return None;
        };
        let asset = self.assets.iter().find(|a| a.id == id).cloned()?;
        let mut edit: Option<Params> = None;
        ui.heading(&asset.name);
        if let Some(tex) = self.textures.get(&id) {
            let size = tex.size_vec2();
            let scale = (ui.available_width() / size.x.max(1.0)).min(1.0);
            ui.add(egui::Image::new(egui::load::SizedTexture::new(
                tex.id(),
                size * scale,
            )));
        } else {
            ui.label("(no thumbnail)");
            let _ = self.load_thumbnail(ui.ctx(), id).map(|t| self.textures.insert(id, t));
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
        if let Some(mtime) = asset.mtime {
            ui.label(format!("Modified: {}", mtime));
        }
        if asset.missing {
            ui.colored_label(egui::Color32::RED, "missing from disk (run Update)");
        }
        if let Some(m) = &self.meta {
            ui.separator();
            ui.heading("Metadata");
            if let Some(v) = &m.captured_local {
                ui.label(format!("Captured: {v}"));
            }
            let cam = match (&m.make, &m.model) {
                (Some(a), Some(b)) => format!("{a} {b}"),
                (Some(a), None) => a.clone(),
                (None, Some(b)) => b.clone(),
                _ => String::new(),
            };
            if !cam.is_empty() {
                ui.label(format!("Camera: {cam}"));
            }
            if let Some(l) = &m.lens {
                ui.label(format!("Lens: {l}"));
            }
            if m.focal_length_mm.is_some() || m.f_number.is_some() || m.exposure_s.is_some() || m.iso.is_some() {
                let mut s = String::new();
                if let Some(f) = m.focal_length_mm {
                    s.push_str(&format!("{f}mm"));
                }
                if let Some(n) = m.f_number {
                    if !s.is_empty() { s.push_str("  "); }
                    s.push_str(&format!("f/{n}"));
                }
                if let Some(e) = m.exposure_s {
                    if !s.is_empty() { s.push_str("  "); }
                    s.push_str(&format!("{e}s"));
                }
                if let Some(iso) = m.iso {
                    if !s.is_empty() { s.push_str("  "); }
                    s.push_str(&format!("ISO{iso}"));
                }
                if !s.is_empty() {
                    ui.label(s);
                }
            }
            if let Some(d) = m.duration_ms {
                ui.label(format!("Duration: {}ms", d));
            }
            if m.gps_lat.is_some() && m.gps_lon.is_some() {
                ui.label(format!("GPS: {}, {}", m.gps_lat.unwrap_or(0.0), m.gps_lon.unwrap_or(0.0)));
            }
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

/// Paint one grid tile: a square thumbnail box with the filename below it.
/// `rect` is the full cell (image box + label); the image fits inside the
/// square, and the selection stroke surrounds the whole cell.
fn draw_tile(
    ui: &egui::Ui,
    rect: egui::Rect,
    tex: Option<&egui::TextureHandle>,
    name: &str,
    selected: bool,
) {
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.width()));
    ui.painter()
        .rect_filled(img_rect, 3.0, ui.visuals().extreme_bg_color);
    match tex {
        Some(tex) => {
            let size = tex.size_vec2();
            let scale = (img_rect.width() / size.x.max(1.0))
                .min(img_rect.height() / size.y.max(1.0));
            let draw_size = size * scale;
            let dest = egui::Rect::from_center_size(img_rect.center(), draw_size);
            egui::Image::new(egui::load::SizedTexture::new(tex.id(), draw_size))
                .paint_at(ui, dest);
        }
        None => {
            ui.painter().text(
                img_rect.center(),
                egui::Align2::CENTER_CENTER,
                "(no thumb)",
                egui::FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
        }
    }
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), img_rect.bottom() + 3.0),
        egui::pos2(rect.right(), rect.bottom()),
    );
    let clipped = ui
        .painter()
        .with_clip_rect(text_rect.intersect(ui.clip_rect()));
    clipped.text(
        text_rect.left_top(),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(11.0),
        ui.visuals().text_color(),
    );
    if selected {
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(2.0, egui::Color32::LIGHT_BLUE),
            egui::StrokeKind::Middle,
        );
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
        egui::Panel::left("folders")
            .default_size(220.0)
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
                // folder tree
                let mut sel = self.folder_sel;
                ui.label("Folders");
                if ui.selectable_label(matches!(sel, FolderSelection::All), "All").clicked() {
                    sel = FolderSelection::All;
                }
                if ui.selectable_label(matches!(sel, FolderSelection::Unassigned), "Unassigned").clicked() {
                    sel = FolderSelection::Unassigned;
                }
                fn build_tree(
                    ui: &mut egui::Ui,
                    folders: &[Folder],
                    parent: Option<i64>,
                    expanded: &mut HashSet<i64>,
                    selected: &mut FolderSelection,
                    depth: usize,
                ) {
                    for f in folders.iter().filter(|x| x.parent_id == parent) {
                        ui.horizontal(|ui| {
                            ui.add_space(depth as f32 * 12.0);
                            let has_children = folders.iter().any(|x| x.parent_id == Some(f.id));
                            if has_children {
                                let exp = expanded.contains(&f.id);
                                if ui.small_button(if exp { "▼" } else { "▶" }).clicked() {
                                    if exp {
                                        expanded.remove(&f.id);
                                    } else {
                                        expanded.insert(f.id);
                                    }
                                }
                            } else {
                                ui.add_space(16.0);
                            }
                            let is_sel = matches!(selected, FolderSelection::Folder(id) if *id == f.id);
                            if ui.selectable_label(is_sel, &f.name).clicked() {
                                *selected = FolderSelection::Folder(f.id);
                            }
                        });
                        if expanded.contains(&f.id) {
                            build_tree(ui, folders, Some(f.id), expanded, selected, depth + 1);
                        }
                    }
                }
                build_tree(ui, &self.folders, None, &mut self.expanded, &mut sel, 0);
                if sel != self.folder_sel {
                    self.folder_sel = sel;
                    self.dirty = true;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Search");
                    let mut search = self.search.clone();
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut search)
                            .hint_text("name or path")
                            .desired_width(100.0)
                    );
                    if resp.changed() {
                        self.search = search.clone();
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.dirty = true;
                    }
                    if ui.add_enabled(!self.search.is_empty(), egui::Button::new("x").small()).clicked() {
                        self.search.clear();
                        self.dirty = true;
                    }
                    if ui.small_button("Go").clicked() {
                        self.dirty = true;
                    }
                });
                ui.separator();
                ui.label(format!("{} assets", self.assets.len()));
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
        // preload thumbnails for visible assets in grid/list as needed
        let ids: Vec<i64> = self.assets.iter().map(|a| a.id).collect();
        for id in ids {
            if !self.textures.contains_key(&id) {
                if let Some(tex) = self.load_thumbnail(&ctx, id) {
                    self.textures.insert(id, tex);
                }
            }
        }
        let mut edit = None;
        egui::Panel::right("details")
            .default_size(320.0)
            .show(ui, |ui| {
                edit = self.details_panel(ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view_mode, ViewMode::Grid, "Grid");
                ui.selectable_value(&mut self.view_mode, ViewMode::List, "List");
            });
            ui.separator();
            if let Some(id) = match self.view_mode {
                ViewMode::Grid => self.grid(ui),
                ViewMode::List => self.list(ui),
            } {
                self.selected = Some(id);
            }
        });
        if let Some(params) = edit {
            self.run_command("edit", &params);
        }
    }
}
