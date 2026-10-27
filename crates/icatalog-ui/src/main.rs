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

mod list_row;
mod selection;
#[cfg(test)]
mod ui_tests;
mod worker;

use eframe::egui;
use icatalog_catalog::{Annotation, Asset, AssetMeta, AssetQuery, Collection, Folder, Source};
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
                "icatalog-ui: internal error (panic caught); reopen the catalogue to check its state"
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
    let ctx = open_catalogue(&path).map_err(|error| eframe::Error::AppCreation(Box::new(error)))?;
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "iCatalog",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, ctx, path)))),
    )
}

fn open_catalogue(path: &std::path::Path) -> icatalog_engine::Result<Context> {
    let mut ctx = Context::open(path)?;
    if ctx.catalog().catalogue()?.is_none() {
        dispatch_guarded(&mut ctx, "init", &Params::new())?;
    }
    Ok(ctx)
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum FolderSelection {
    All,
    Unassigned,
    Folder(i64),
    /// All assets from every source on one device (e.g. "This Mac").
    Device(String),
    Collections(Vec<i64>),
}

/// Mutable tree state shared by the recursive folder-tree builder.
struct TreeState<'a> {
    expanded: &'a mut HashSet<i64>,
    collapsed_devices: &'a mut HashSet<String>,
    selected: &'a mut FolderSelection,
}

#[cfg(target_os = "macos")]
struct NativeMenu {
    _menu: muda::Menu,
    import: muda::MenuItem,
    update: muda::MenuItem,
}
#[cfg(not(target_os = "macos"))]
type NativeMenu = ();

struct App {
    ctx: Context,
    path: PathBuf,
    assets: Vec<Asset>,
    folders: Vec<Folder>,
    sources: Vec<Source>,
    collections: Vec<Collection>,
    collection_counts: HashMap<i64, i64>,
    create_collection: Option<CreateCollectionDialog>,
    asset_collections: HashSet<i64>,
    selection_collection_members: HashMap<i64, HashSet<i64>>,
    selection_membership_for: Option<HashSet<i64>>,
    selection_membership_loaded: bool,
    folder_sel: FolderSelection,
    collection_anchor: Option<i64>,
    expanded: HashSet<i64>,
    collapsed_devices: HashSet<String>,
    view_mode: ViewMode,
    list_details: bool,
    list_metadata: HashMap<i64, Option<list_row::Metadata>>,
    selection: selection::AssetSelection,
    marquee: Option<selection::Marquee>,
    textures: HashMap<i64, egui::TextureHandle>,
    annotation: Option<Annotation>,
    keywords: Vec<String>,
    keyword_input: String,
    meta: Option<AssetMeta>,
    detail_for: Option<i64>,
    status: String,
    dirty: bool,
    search: String,
    /// Native (macOS) menu bar; kept alive for the app's lifetime. `None` when
    /// it could not be installed, in which case the in-app menu bar is shown.
    menu: Option<NativeMenu>,
    /// Menu-item ids delivered from the native menu.
    menu_events: std::sync::mpsc::Receiver<String>,
    worker: Option<worker::Worker>,
    attempted_thumbnails: HashSet<i64>,
    visible_thumbnails: HashSet<i64>,
}

#[derive(Default)]
struct CreateCollectionDialog {
    name: String,
    error: Option<String>,
    pending: bool,
    focus_name: bool,
}

struct DraggedAsset {
    ids: Vec<i64>,
    name: String,
}

/// Build the native macOS menu bar. Returns `None` on other platforms or if
/// the menu cannot be installed, so the in-app menu bar remains the fallback.
fn install_native_menu(
    ctx: &egui::Context,
    tx: std::sync::mpsc::Sender<String>,
) -> Option<NativeMenu> {
    #[cfg(target_os = "macos")]
    {
        use muda::accelerator::Accelerator;
        use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

        let menu = Menu::new();
        let app_menu = Submenu::new("iCatalog", true);
        let file_menu = Submenu::new("File", true);
        let import = MenuItem::with_id("file.import", "Import…", true, None::<Accelerator>);
        let update = MenuItem::with_id("file.update", "Update", true, None::<Accelerator>);

        // Keep the standard app menu (Quit/⌘Q): installing our menu replaces
        // NSApp's main menu wholesale.
        let built = app_menu.append(&PredefinedMenuItem::quit(None)).is_ok()
            && file_menu.append_items(&[&import, &update]).is_ok()
            && menu.append_items(&[&app_menu, &file_menu]).is_ok();
        if !built {
            return None;
        }

        // muda reports clicks on its own channel; forward them to the UI
        // thread and wake egui so the action runs without another input event.
        let repaint = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = tx.send(event.id().0.clone());
            repaint.request_repaint();
        }));
        menu.init_for_nsapp();
        // Ensure the app menu shows "iCatalog" (eframe/muda may derive it from
        // the process/bundle name in some environments).
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSApplication;
            use objc2_foundation::{MainThreadMarker, NSString};
            let mtm = MainThreadMarker::new()?;
            let app = NSApplication::sharedApplication(mtm);
            if let Some(app_menu) = app
                .mainMenu()
                .and_then(|menu| menu.itemAtIndex(0))
                .and_then(|item| item.submenu())
            {
                app_menu.setTitle(&NSString::from_str("iCatalog"));
            }
        }
        Some(NativeMenu {
            _menu: menu,
            import,
            update,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (ctx, tx);
        None
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, ctx: Context, path: PathBuf) -> Self {
        let (menu_tx, menu_events) = std::sync::mpsc::channel();
        let menu = install_native_menu(&cc.egui_ctx, menu_tx);
        Self::with_menu(ctx, path, menu, menu_events)
    }

    fn with_menu(
        ctx: Context,
        path: PathBuf,
        menu: Option<NativeMenu>,
        menu_events: std::sync::mpsc::Receiver<String>,
    ) -> Self {
        let mut app = App {
            ctx,
            path,
            assets: Vec::new(),
            folders: Vec::new(),
            sources: Vec::new(),
            collections: Vec::new(),
            collection_counts: HashMap::new(),
            create_collection: None,
            asset_collections: HashSet::new(),
            selection_collection_members: HashMap::new(),
            selection_membership_for: None,
            selection_membership_loaded: false,
            folder_sel: FolderSelection::All,
            collection_anchor: None,
            expanded: HashSet::new(),
            collapsed_devices: HashSet::new(),
            view_mode: ViewMode::Grid,
            list_details: false,
            list_metadata: HashMap::new(),
            selection: selection::AssetSelection::default(),
            marquee: None,
            textures: HashMap::new(),
            annotation: None,
            keywords: Vec::new(),
            keyword_input: String::new(),
            meta: None,
            detail_for: None,
            status: String::new(),
            dirty: true,
            search: String::new(),
            menu,
            menu_events,
            worker: None,
            attempted_thumbnails: HashSet::new(),
            visible_thumbnails: HashSet::new(),
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        if let Err(error) = self.try_refresh() {
            self.status = format!("Cannot refresh catalogue: {error}");
        }
    }

    fn try_refresh(&mut self) -> icatalog_engine::Result<()> {
        let folders = self.ctx.catalog().all_folders()?;
        let sources = self.ctx.catalog().all_sources()?;
        let collections = self.ctx.catalog().all_collections()?;
        let collection_counts = self
            .ctx
            .catalog()
            .collection_asset_counts()?
            .into_iter()
            .collect();
        let source_ids = match &self.folder_sel {
            FolderSelection::Device(device) => Some(
                sources
                    .iter()
                    .filter(|s| &s.device().0 == device)
                    .map(|s| s.id)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let q = AssetQuery {
            collection_ids: match &self.folder_sel {
                FolderSelection::Collections(ids) => Some(ids.clone()),
                _ => None,
            },
            folder_id: match &self.folder_sel {
                FolderSelection::Folder(fid) => Some(*fid),
                _ => None,
            },
            folder_id_null: match &self.folder_sel {
                FolderSelection::Unassigned => Some(true),
                _ => None,
            },
            source_ids,
            text_like: if self.search.trim().is_empty() {
                None
            } else {
                Some(self.search.trim().to_string())
            },
            ..Default::default()
        };
        let assets = self.ctx.catalog().query_assets(&q)?;
        self.folders = folders;
        self.sources = sources;
        self.collections = collections;
        self.collection_counts = collection_counts;
        self.assets = assets;
        self.list_metadata.clear();
        self.selection.retain(&self.assets);
        self.selection_membership_for = None;
        self.selection_membership_loaded = false;
        self.marquee = None;
        // keep cached textures only for assets that are still in view
        let ids: HashSet<i64> = self.assets.iter().map(|a| a.id).collect();
        self.textures.retain(|k, _| ids.contains(k));
        Ok(())
    }

    fn run_command(&mut self, id: &str, params: &Params) {
        if self.worker.is_some() {
            return;
        }
        match worker::Worker::start(self.path.clone(), id.to_string(), params.clone()) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.selection_membership_for = None;
                self.selection_membership_loaded = false;
                self.set_menu_enabled(false);
                self.status = format!("Running {id}…");
            }
            Err(error) => self.status = format!("Cannot start {id}: {error}"),
        }
    }

    fn set_menu_enabled(&self, enabled: bool) {
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.menu {
            menu.import.set_enabled(enabled);
            menu.update.set_enabled(enabled);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = enabled;
    }

    fn poll_worker(&mut self) {
        let Some(result) = self.worker.as_mut().and_then(worker::Worker::poll) else {
            return;
        };
        self.worker = None;
        if let Some(dialog) = self
            .create_collection
            .as_mut()
            .filter(|dialog| dialog.pending)
        {
            match &result {
                Ok(_) => self.create_collection = None,
                Err(error) => {
                    dialog.pending = false;
                    dialog.error = Some(error.to_string());
                    dialog.focus_name = true;
                }
            }
        }
        self.set_menu_enabled(self.create_collection.is_none());
        self.status = match result {
            Ok(outcome) => outcome.render(),
            Err(error) => error.to_string(),
        };
        self.dirty = true;
        self.textures.clear();
        self.attempted_thumbnails.clear();
        self.detail_for = None;
    }

    /// Load the selected asset's annotation, keywords and metadata once per selection.
    fn ensure_detail(&mut self) {
        if self.selection.ids.len() > 1 {
            self.detail_for = None;
            return;
        }
        let Some(id) = self.selection.primary else {
            self.detail_for = None;
            self.annotation = None;
            self.keywords.clear();
            self.asset_collections.clear();
            self.meta = None;
            return;
        };
        if self.detail_for == Some(id) {
            return;
        }
        if self.worker.is_some() {
            return;
        }
        let result = (|| -> icatalog_catalog::Result<_> {
            Ok((
                self.ctx.catalog().annotation(id)?,
                self.ctx.catalog().asset_keywords(id)?,
                self.ctx.catalog().asset_meta(id)?,
                self.ctx.catalog().asset_collection_ids(id)?,
            ))
        })();
        self.detail_for = Some(id);
        match result {
            Ok((annotation, keywords, meta, collections)) => {
                self.annotation = annotation;
                self.keywords = keywords.into_iter().map(|k| k.name).collect();
                self.meta = meta;
                self.asset_collections = collections.into_iter().collect();
            }
            Err(error) => {
                self.annotation = None;
                self.keywords.clear();
                self.asset_collections.clear();
                self.meta = None;
                self.status = format!("Cannot load asset details: {error}");
            }
        }
        self.keyword_input.clear();
    }

    fn ensure_selection_collection_membership(&mut self, retry_failed: bool) {
        if self.worker.is_some()
            || self.create_collection.is_some()
            || (self.selection_membership_for.as_ref() == Some(&self.selection.ids)
                && (self.selection_membership_loaded || !retry_failed))
        {
            return;
        }
        // Cache failed attempts until the menu is explicitly reopened, so a
        // read error does not retry a blocked database on every frame.
        self.selection_membership_for = Some(self.selection.ids.clone());
        self.selection_membership_loaded = false;
        self.selection_collection_members.clear();
        let ids = self.selection.ordered(&self.assets);
        match self.ctx.catalog().collection_memberships(&ids) {
            Ok(memberships) => {
                for (asset_id, collection_id) in memberships {
                    self.selection_collection_members
                        .entry(collection_id)
                        .or_default()
                        .insert(asset_id);
                }
                self.selection_membership_loaded = true;
            }
            Err(error) => {
                self.status = format!("Cannot load collection membership: {error}");
            }
        }
    }

    fn select_collection(&mut self, id: i64, modifiers: egui::Modifiers) {
        let current = match &self.folder_sel {
            FolderSelection::Collections(ids) => ids.clone(),
            _ => Vec::new(),
        };
        let current_set: HashSet<i64> = current.iter().copied().collect();
        let toggle = modifiers.ctrl || modifiers.command;
        let selected = if modifiers.shift {
            let anchor = self.collection_anchor.unwrap_or(id);
            let start = self.collections.iter().position(|c| c.id == anchor);
            let end = self.collections.iter().position(|c| c.id == id);
            let range = match (start, end) {
                (Some(a), Some(b)) => self.collections.get(a.min(b)..=a.max(b)).unwrap_or(&[]),
                _ => &[],
            };
            let ids: Vec<i64> = range.iter().map(|c| c.id).collect();
            let ids = if ids.is_empty() { vec![id] } else { ids };
            if self.collection_anchor.is_none() || start.is_none() {
                self.collection_anchor = Some(id);
            }
            if toggle {
                let range_set: HashSet<i64> = ids.iter().copied().collect();
                self.collections
                    .iter()
                    .filter(|c| current_set.contains(&c.id) || range_set.contains(&c.id))
                    .map(|c| c.id)
                    .collect()
            } else {
                ids
            }
        } else if toggle {
            self.collection_anchor = Some(id);
            self.collections
                .iter()
                .filter(|c| {
                    if c.id == id {
                        !current_set.contains(&id)
                    } else {
                        current_set.contains(&c.id)
                    }
                })
                .map(|c| c.id)
                .collect()
        } else {
            self.collection_anchor = Some(id);
            if current == [id] {
                Vec::new()
            } else {
                vec![id]
            }
        };
        self.folder_sel = if selected.is_empty() {
            self.collection_anchor = None;
            FolderSelection::All
        } else {
            FolderSelection::Collections(selected)
        };
        self.dirty = true;
    }

    fn collections_panel(&mut self, ui: &mut egui::Ui) -> Option<Params> {
        let mut command = None;
        let mut clicked = None;
        let selected_ids: HashSet<i64> = match &self.folder_sel {
            FolderSelection::Collections(ids) => ids.iter().copied().collect(),
            _ => HashSet::new(),
        };
        ui.separator();
        ui.label("Collections");
        if ui
            .add_enabled(
                self.worker.is_none() && self.create_collection.is_none(),
                egui::Button::new("New Collection…"),
            )
            .clicked()
        {
            self.create_collection = Some(CreateCollectionDialog {
                focus_name: true,
                ..Default::default()
            });
            self.set_menu_enabled(false);
        }
        egui::ScrollArea::vertical()
            .id_salt("collection_list")
            .auto_shrink([false, true])
            .max_height(180.0)
            .show(ui, |ui| {
                for collection in &self.collections {
                    ui.push_id(collection.id, |ui| {
                        let selected = selected_ids.contains(&collection.id);
                        let count = self
                            .collection_counts
                            .get(&collection.id)
                            .copied()
                            .unwrap_or(0);
                        let row = collection_row(ui, &collection.name, count, selected)
                            .response
                            .on_hover_text(format!(
                                "{} (Collection {})",
                                collection.name, collection.id
                            ));
                        if row.clicked() {
                            clicked = Some((collection.id, ui.input(|input| input.modifiers)));
                        }
                        if self.worker.is_none() && self.create_collection.is_none() {
                            if row.dnd_hover_payload::<DraggedAsset>().is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
                                ui.painter().rect_stroke(
                                    row.rect.expand(2.0),
                                    3.0,
                                    ui.visuals().selection.stroke,
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if let Some(asset) = row.dnd_release_payload::<DraggedAsset>() {
                                command = Some(collection_params("add", collection.id, &asset.ids));
                            }
                        }
                    });
                }
            });
        if let Some((id, modifiers)) = clicked {
            self.select_collection(id, modifiers);
        }
        command
    }

    fn create_collection_modal(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.create_collection else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("create_collection")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.heading("New Collection");
            ui.add_enabled_ui(!dialog.pending, |ui| {
                let label = ui.label("Name");
                let name = ui
                    .add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .desired_width(f32::INFINITY)
                            .char_limit(1024),
                    )
                    .labelled_by(label.id);
                if dialog.focus_name {
                    name.request_focus();
                    dialog.focus_name = false;
                }
                let enter = name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(error) = &dialog.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                ui.horizontal(|ui| {
                    cancel = ui.button("Cancel").clicked();
                    let valid = !dialog.name.trim().is_empty();
                    submit = ui.add_enabled(valid, egui::Button::new("Create")).clicked()
                        || (valid && enter);
                });
            });
            if dialog.pending {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Creating collection…");
                });
            }
        });
        if !dialog.pending && (cancel || response.should_close()) {
            self.create_collection = None;
            self.set_menu_enabled(self.worker.is_none());
        } else if submit && !dialog.pending {
            let mut params = Params::new();
            params
                .set("action", "create")
                .set("name", dialog.name.trim());
            self.run_command("collection", &params);
            if let Some(dialog) = &mut self.create_collection {
                dialog.pending = self.worker.is_some();
                if !dialog.pending {
                    dialog.error = Some(self.status.clone());
                }
            }
        }
    }

    fn asset_interactions(
        &mut self,
        response: &egui::Response,
        id: i64,
        name: &str,
    ) -> Option<Params> {
        let can_edit = self.worker.is_none() && self.create_collection.is_none();
        // egui may pick the nearest widget when a press lands in a narrow gap.
        // An asset drag must start inside its cell; gaps belong to rectangle selection.
        let asset_drag = self.marquee.is_none()
            && response.drag_started_by(egui::PointerButton::Primary)
            && response.ctx.input(|i| {
                i.pointer
                    .press_origin()
                    .is_some_and(|p| response.rect.contains(p))
            });
        if response.clicked() {
            self.selection
                .click(id, &self.assets, response.ctx.input(|i| i.modifiers));
        } else if response.secondary_clicked() || asset_drag {
            self.selection.focus(id);
        }
        if can_edit && asset_drag {
            response.dnd_set_drag_payload(DraggedAsset {
                ids: self.selection.ordered(&self.assets),
                name: if self.selection.ids.len() == 1 {
                    name.into()
                } else {
                    format!("{} assets", self.selection.ids.len())
                },
            });
        }
        if response.secondary_clicked() || response.context_menu_opened() {
            self.ensure_detail();
            self.ensure_selection_collection_membership(response.secondary_clicked());
        }
        let mut command = None;
        response.context_menu(|ui| {
            let ids = self.selection.ordered(&self.assets);
            ui.add_enabled_ui(can_edit && self.selection_membership_loaded, |ui| {
                ui.menu_button("Collections", |ui| {
                    if ids.len() > 1 {
                        ui.label(format!("{} assets selected", ids.len()));
                    }
                    if self.collections.is_empty() {
                        ui.label("Create a collection in the sidebar first.");
                    }
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            for collection in &self.collections {
                                let member_count = self
                                    .selection_collection_members
                                    .get(&collection.id)
                                    .map_or(0, HashSet::len);
                                let checked = !ids.is_empty() && member_count == ids.len();
                                let indeterminate = member_count > 0 && member_count < ids.len();
                                let mut value = checked;
                                if ui
                                    .add(
                                        egui::Checkbox::new(&mut value, &collection.name)
                                            .indeterminate(indeterminate),
                                    )
                                    .on_hover_text(format!("Collection {}", collection.id))
                                    .clicked()
                                {
                                    let (action, target_ids) = if checked {
                                        ("remove", ids.clone())
                                    } else if indeterminate {
                                        let members =
                                            self.selection_collection_members.get(&collection.id);
                                        let missing: Vec<i64> = ids
                                            .iter()
                                            .copied()
                                            .filter(|id| {
                                                !members.is_some_and(|members| members.contains(id))
                                            })
                                            .collect();
                                        ("add", missing)
                                    } else {
                                        ("add", ids.clone())
                                    };
                                    command =
                                        Some(collection_params(action, collection.id, &target_ids));
                                    ui.close();
                                }
                            }
                        });
                });
            });
        });
        command
    }

    fn list(&mut self, ui: &mut egui::Ui) -> Option<Params> {
        // Keep horizontal overflow discoverable in narrow windows/with Details open.
        ui.style_mut().spacing.scroll.floating = false;
        let mut command = None;
        let height = list_row::HEIGHT;
        let spacing = ui.spacing().item_spacing;
        let mut origin = None;
        let mut cell = egui::vec2(1.0, height);
        let mut rects = Vec::new();
        let output = egui::ScrollArea::both()
            .id_salt("asset_list")
            .auto_shrink([false, false])
            .show_rows(ui, height, self.assets.len(), |ui, rows| {
                ui.set_min_width(list_row::MIN_WIDTH);
                for index in rows {
                    if let Some(asset) = self.assets.get(index).cloned() {
                        let (id, name) = (asset.id, asset.name.clone());
                        self.ensure_texture(ui.ctx(), id);
                        if self.worker.is_none() && !self.list_metadata.contains_key(&id) {
                            let metadata = list_row::Metadata::load(self.ctx.catalog(), id);
                            let metadata = match metadata {
                                Ok(metadata) => Some(metadata),
                                Err(error) => {
                                    self.status = format!("Cannot load list metadata: {error}");
                                    None
                                }
                            };
                            self.list_metadata.insert(id, metadata);
                        }
                        let response = list_row::draw(
                            ui,
                            &asset,
                            self.textures.get(&id),
                            self.list_metadata.get(&id).and_then(Option::as_ref),
                            self.selection.ids.contains(&id),
                            index,
                        );
                        rects.push(response.rect);
                        cell = response.rect.size();
                        origin.get_or_insert(
                            response.rect.min
                                - egui::vec2(0.0, index as f32 * (height + spacing.y)),
                        );
                        if let Some(params) = self.asset_interactions(&response, id, &name) {
                            command = Some(params);
                        }
                    }
                }
            });
        self.handle_marquee(
            ui,
            &output,
            selection::AssetLayout {
                origin: origin.unwrap_or(output.inner_rect.min - output.state.offset),
                columns: 1,
                cell,
                spacing,
            },
            &rects,
        );
        command
    }

    fn ensure_texture(&mut self, ctx: &egui::Context, id: i64) {
        self.visible_thumbnails.insert(id);
        if self.worker.is_some() || !self.attempted_thumbnails.insert(id) {
            return;
        }
        if let Some(texture) = self.load_thumbnail(ctx, id) {
            self.textures.insert(id, texture);
        }
    }

    fn load_thumbnail(&self, ctx: &egui::Context, id: i64) -> Option<egui::TextureHandle> {
        let t = self
            .ctx
            .catalog()
            .thumbnail(id, i64::from(icatalog_media::GRID_LONG_EDGE))
            .ok()
            .flatten()?;
        let orientation = if t.orientation_applied {
            None
        } else {
            self.ctx
                .catalog()
                .asset_meta(id)
                .ok()?
                .and_then(|m| m.orientation)
        };
        let img = icatalog_media::decode_rgba_with_orientation(&t.bytes, orientation).ok()?;
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width as usize, img.height as usize],
            &img.rgba,
        );
        Some(ctx.load_texture(format!("thumb-{id}"), color, egui::TextureOptions::LINEAR))
    }

    fn grid(&mut self, ui: &mut egui::Ui) -> Option<Params> {
        let mut command = None;
        let tile = 160.0;
        let height = 180.0;
        let cols =
            ((ui.available_width() / (tile + ui.spacing().item_spacing.x)).floor() as usize).max(1);
        let row_count = self.assets.len().div_ceil(cols);
        let spacing = ui.spacing().item_spacing;
        let mut origin = None;
        let mut rects = Vec::new();
        let output = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, height, row_count, |ui, rows| {
                for row in rows {
                    ui.horizontal(|ui| {
                        for index in row.saturating_mul(cols)
                            ..row
                                .saturating_add(1)
                                .saturating_mul(cols)
                                .min(self.assets.len())
                        {
                            let Some(asset) = self.assets.get(index) else {
                                continue;
                            };
                            let (id, name) = (asset.id, asset.name.clone());
                            self.ensure_texture(ui.ctx(), id);
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(tile, height),
                                egui::Sense::click_and_drag(),
                            );
                            rects.push(rect);
                            origin.get_or_insert(
                                rect.min
                                    - egui::vec2(
                                        (index % cols) as f32 * (tile + spacing.x),
                                        row as f32 * (height + spacing.y),
                                    ),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    ui.is_enabled(),
                                    self.selection.ids.contains(&id),
                                    &name,
                                )
                            });
                            draw_tile(
                                ui,
                                rect,
                                self.textures.get(&id),
                                &name,
                                self.selection.ids.contains(&id),
                            );
                            if let Some(params) = self.asset_interactions(&response, id, &name) {
                                command = Some(params);
                            }
                            response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        }
                    });
                }
            });
        self.handle_marquee(
            ui,
            &output,
            selection::AssetLayout {
                origin: origin.unwrap_or(output.inner_rect.min - output.state.offset),
                columns: cols,
                cell: egui::vec2(tile, height),
                spacing,
            },
            &rects,
        );
        command
    }

    fn handle_marquee(
        &mut self,
        ui: &mut egui::Ui,
        output: &egui::scroll_area::ScrollAreaOutput<()>,
        mut layout: selection::AssetLayout,
        cells: &[egui::Rect],
    ) {
        if self.worker.is_some() || self.create_collection.is_some() {
            return;
        }
        let (pressed, down, released, pointer, modifiers) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos(),
                i.modifiers,
            )
        });
        let mut viewport = output.inner_rect;
        let content_origin = viewport.min - output.state.offset;
        if output.content_size.y > viewport.height() {
            // Floating scrollbars overlap inner_rect. Keep their drag surface
            // separate from rectangle selection, including while expanded.
            viewport.max.x = (viewport.max.x - ui.spacing().scroll.bar_width).max(viewport.min.x);
        }
        layout.origin -= content_origin.to_vec2();
        if pressed && !egui::Popup::is_any_open(ui.ctx()) && ui.rect_contains_pointer(viewport) {
            if let Some(pointer) = pointer.filter(|p| !cells.iter().any(|r| r.contains(*p))) {
                self.marquee = Some(selection::Marquee {
                    start: pointer - content_origin.to_vec2(),
                    base: self.selection.clone(),
                    modifiers,
                });
                // egui can assign a nearby row as the potential drag target
                // for a press in a gap. That ghost asset drag blocks wheel
                // scrolling even though rectangle selection owns the gesture.
                ui.ctx().stop_dragging();
            }
        }
        if self.marquee.is_some() && ui.input_mut(|i| i.consume_key(i.modifiers, egui::Key::Escape))
        {
            if let Some(marquee) = self.marquee.take() {
                self.selection = marquee.base;
            }
            return;
        }
        if let Some(marquee) = &self.marquee {
            if let Some(pointer) = pointer {
                let current = viewport.clamp(pointer) - content_origin.to_vec2();
                let rect = egui::Rect::from_two_pos(marquee.start, current);
                let hits = layout.hits(rect, &self.assets);
                self.selection
                    .rectangle(&marquee.base, hits, marquee.modifiers, &self.assets);
                if marquee.start.distance(current) > 3.0 {
                    let screen = rect.translate(content_origin.to_vec2()).intersect(viewport);
                    ui.painter().rect_filled(
                        screen,
                        0.0,
                        ui.visuals().selection.bg_fill.gamma_multiply(0.25),
                    );
                    ui.painter().rect_stroke(
                        screen,
                        0.0,
                        ui.visuals().selection.stroke,
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        if released || !down {
            self.marquee = None;
        }
    }

    fn details_panel(&mut self, ui: &mut egui::Ui) -> Option<Params> {
        if self.selection.ids.len() > 1 {
            ui.heading(format!("{} assets selected", self.selection.ids.len()));
            ui.label(
                "Drag the selection onto a collection, or right-click for collection actions.",
            );
            return None;
        }
        let Some(id) = self.selection.primary else {
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
        }
        ui.separator();
        ui.label(format!("Path: {}", asset.path));
        if let Some(src) = asset
            .folder_id
            .and_then(|fid| self.folders.iter().find(|f| f.id == fid))
            .and_then(|f| f.source_id)
            .and_then(|sid| self.sources.iter().find(|s| s.id == sid))
        {
            let (_, device) = src.device();
            ui.label(format!("Imported from: {device} · {}", src.label));
        }
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
            if m.focal_length_mm.is_some()
                || m.f_number.is_some()
                || m.exposure_s.is_some()
                || m.iso.is_some()
            {
                let mut s = String::new();
                if let Some(f) = m.focal_length_mm {
                    s.push_str(&format!("{f}mm"));
                }
                if let Some(n) = m.f_number {
                    if !s.is_empty() {
                        s.push_str("  ");
                    }
                    s.push_str(&format!("f/{n}"));
                }
                if let Some(e) = m.exposure_s {
                    if !s.is_empty() {
                        s.push_str("  ");
                    }
                    s.push_str(&format!("{e}s"));
                }
                if let Some(iso) = m.iso {
                    if !s.is_empty() {
                        s.push_str("  ");
                    }
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
                ui.label(format!(
                    "GPS: {}, {}",
                    m.gps_lat.unwrap_or(0.0),
                    m.gps_lon.unwrap_or(0.0)
                ));
            }
        }
        ui.separator();
        ui.heading("Annotations");
        let rating = self.annotation.as_ref().map(|a| a.rating).unwrap_or(0);
        let label = self.annotation.as_ref().map(|a| a.label).unwrap_or(0);
        ui.horizontal(|ui| {
            ui.label("Rating");
            for r in 0..=5 {
                let response = ui.selectable_label(rating == r, r.to_string());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        ui.is_enabled(),
                        rating == r,
                        format!("Rating {r}"),
                    )
                });
                if response.clicked() && rating != r {
                    edit = Some(edit_params(id, "rating", &r.to_string()));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Label");
            for l in 0..=7 {
                let response = ui.selectable_label(label == l, l.to_string());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        ui.is_enabled(),
                        label == l,
                        format!("Label {l}"),
                    )
                });
                if response.clicked() && label != l {
                    edit = Some(edit_params(id, "label", &l.to_string()));
                }
            }
        });
        ui.horizontal(|ui| {
            let keyword_label = ui.label("Keyword");
            ui.add(
                egui::TextEdit::singleline(&mut self.keyword_input)
                    .hint_text("add")
                    .desired_width(150.0),
            )
            .labelled_by(keyword_label.id);
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

/// A disclosure arrow button for a collapsible section or folder row.
///
/// The triangle is painted rather than drawn from a text glyph: egui's bundled
/// proportional fonts cover the right-pointing triangle (`▶`) but not the
/// down-pointing one (`▼`), so a text arrow left a missing-character box on
/// screen whenever the section was open. Painting keeps both states the same
/// size and weight.
fn disclosure_arrow(ui: &mut egui::Ui, open: bool) -> egui::Response {
    let response = ui.add(
        egui::Button::new("")
            .small()
            .min_size(egui::vec2(18.0, ui.spacing().interact_size.y)),
    );
    let icon_rect = egui::Rect::from_center_size(
        response.rect.center(),
        egui::Vec2::splat(ui.spacing().icon_width_inner),
    );
    let icon_response = response.clone().with_new_rect(icon_rect);
    egui::collapsing_header::paint_default_icon(ui, if open { 1.0 } else { 0.0 }, &icon_response);
    response
}

/// A single clickable row, with fixed count space and a shrinking name.
fn collection_row(
    ui: &mut egui::Ui,
    name: &str,
    count: i64,
    selected: bool,
) -> egui::AtomLayoutResponse {
    let mut classes = egui::widget_style::Classes::default();
    if selected {
        egui::widget_style::HasClasses::add_class(&mut classes, egui::widget_style::SELECTED_CLASS);
    }
    // Resolve the same previous-pass state that Button uses for its name.
    let state = ui
        .ctx()
        .read_response(ui.next_auto_id())
        .map(|response| response.widget_state())
        .unwrap_or_default();
    let text_style = ui.style().button_style(&classes, state).text_style;
    let count_text =
        ui.painter()
            .layout_no_wrap(count.to_string(), text_style.font_id, text_style.color);
    let count_id = egui::Id::new("collection_count");
    let name = egui::Atom {
        id: Some(egui::Id::new("collection_name")),
        ..egui::Atom::from(name)
    };
    let row = egui::Button::selectable(
        selected,
        (
            name,
            egui::Atom::grow(),
            egui::Atom::custom(count_id, count_text.size()),
        ),
    )
    .min_size(egui::vec2(
        ui.available_width(),
        ui.spacing().interact_size.y,
    ))
    .truncate()
    .atom_ui(ui);
    if let Some(rect) = row.rect(count_id) {
        ui.painter().galley(rect.min, count_text, text_style.color);
    }
    row
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
            let scale =
                (img_rect.width() / size.x.max(1.0)).min(img_rect.height() / size.y.max(1.0));
            let draw_size = size * scale;
            let dest = egui::Rect::from_center_size(img_rect.center(), draw_size);
            egui::Image::new(egui::load::SizedTexture::new(tex.id(), draw_size)).paint_at(ui, dest);
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

fn collection_params(action: &str, collection: i64, assets: &[i64]) -> Params {
    let mut params = Params::new();
    params
        .set("action", action)
        .set("id", collection.to_string());
    if assets.len() == 1 {
        if let Some(asset) = assets.first() {
            params.set("asset", asset.to_string());
        }
    } else {
        params.set(
            "assets",
            assets
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    params
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_worker();
        self.visible_thumbnails.clear();
        if self.worker.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
            if ui.ctx().input(|input| input.viewport().close_requested()) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.status = "A command is running; wait for it to finish before closing.".into();
            }
        }
        if self.dirty && self.worker.is_none() {
            self.refresh();
            self.dirty = false;
        }
        let ctx = ui.ctx().clone();
        if self.worker.is_some() || self.create_collection.is_some() {
            egui::DragAndDrop::clear_payload(&ctx);
            if let Some(marquee) = self.marquee.take() {
                self.selection = marquee.base;
            }
        }
        self.ensure_detail();

        let mut action = None;
        let mut collection_command = None;
        // Native macOS menu clicks arrive through the handler installed in
        // `install_native_menu`; drain them before drawing.
        while let Ok(id) = self.menu_events.try_recv() {
            if self.worker.is_some() || self.create_collection.is_some() {
                continue;
            }
            match id.as_str() {
                "file.import" => {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        action = Some(Action::Import(dir));
                    }
                }
                "file.update" => action = Some(Action::Update),
                _ => {}
            }
        }
        // Fallback toolbar menu when no native menu bar could be installed
        // (non-macOS today).
        if self.menu.is_none() {
            egui::Panel::top("menu").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui
                            .add_enabled(
                                self.worker.is_none() && self.create_collection.is_none(),
                                egui::Button::new("Import…"),
                            )
                            .clicked()
                        {
                            ui.close();
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                action = Some(Action::Import(dir));
                            }
                        }
                        if ui
                            .add_enabled(
                                self.worker.is_none() && self.create_collection.is_none(),
                                egui::Button::new("Update"),
                            )
                            .clicked()
                        {
                            ui.close();
                            action = Some(Action::Update);
                        }
                    });
                });
            });
        }
        egui::Panel::left("folders")
            .default_size(220.0)
            .show(ui, |ui| {
                // folder tree
                let mut sel = self.folder_sel.clone();
                ui.label("Folders");
                if ui.selectable_label(matches!(sel, FolderSelection::All), "All").clicked() {
                    sel = FolderSelection::All;
                }
                if ui.selectable_label(matches!(sel, FolderSelection::Unassigned), "Unassigned").clicked() {
                    sel = FolderSelection::Unassigned;
                }
                fn folder_device(
                    f: &Folder,
                    devices: &HashMap<i64, (String, String)>,
                ) -> (Option<String>, String) {
                    match f.source_id.and_then(|s| devices.get(&s)) {
                        Some((id, label)) => (Some(id.clone()), label.clone()),
                        None => (None, "Other".to_string()),
                    }
                }
                fn build_tree(
                    ui: &mut egui::Ui,
                    folders: &[Folder],
                    parent: Option<i64>,
                    state: &mut TreeState<'_>,
                    depth: usize,
                    devices: &HashMap<i64, (String, String)>,
                ) {
                    if depth >= 128 {
                        ui.label("Folder nesting limit reached");
                        return;
                    }
                    let mut children: Vec<&Folder> = folders
                        .iter()
                        .filter(|x| x.parent_id == parent)
                        .collect();
                    if parent.is_none() {
                        // Group sources under their device (This Mac, disc, share).
                        children.sort_by(|a, b| {
                            folder_device(a, devices)
                                .1
                                .cmp(&folder_device(b, devices).1)
                                .then_with(|| {
                                    a.path
                                        .as_deref()
                                        .unwrap_or(&a.name)
                                        .cmp(b.path.as_deref().unwrap_or(&b.name))
                                })
                        });
                    }
                    let mut last_device: Option<String> = None;
                    for f in children {
                        if parent.is_none() {
                            let (did, dev) = folder_device(f, devices);
                            if last_device.as_deref() != Some(dev.as_str()) {
                                last_device = Some(dev.clone());
                                ui.add_space(6.0);
                                let key = did.clone().unwrap_or_else(|| "Other".to_string());
                                ui.horizontal(|ui| {
                                    if did.is_some() {
                                        // Accordion: the arrow collapses the
                                        // device; its name selects every source
                                        // on that device.
                                        let open = !state.collapsed_devices.contains(&key);
                                        if disclosure_arrow(ui, open).clicked() {
                                            if open {
                                                state.collapsed_devices.insert(key.clone());
                                            } else {
                                                state.collapsed_devices.remove(&key);
                                            }
                                        }
                                        let is_sel = matches!(
                                            state.selected,
                                            FolderSelection::Device(d) if *d == key
                                        );
                                        if ui
                                            .selectable_label(
                                                is_sel,
                                                egui::RichText::new(&dev).strong(),
                                            )
                                            .clicked()
                                        {
                                            state.collapsed_devices.remove(&key);
                                            *state.selected = FolderSelection::Device(key.clone());
                                        }
                                    } else {
                                        ui.add_space(16.0);
                                        ui.label(egui::RichText::new(&dev).strong());
                                    }
                                });
                            }
                            if let Some(id) = &did {
                                if state.collapsed_devices.contains(id) {
                                    continue;
                                }
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.add_space(8.0 + depth as f32 * 12.0);
                            let has_children = folders.iter().any(|x| x.parent_id == Some(f.id));
                            if has_children {
                                let exp = state.expanded.contains(&f.id);
                                if disclosure_arrow(ui, exp).clicked() {
                                    if exp {
                                        state.expanded.remove(&f.id);
                                    } else {
                                        state.expanded.insert(f.id);
                                    }
                                }
                            } else {
                                ui.add_space(16.0);
                            }
                            let is_sel = matches!(state.selected, FolderSelection::Folder(id) if *id == f.id);
                            if ui.selectable_label(is_sel, &f.name).clicked() {
                                *state.selected = FolderSelection::Folder(f.id);
                            }
                        });
                        if state.expanded.contains(&f.id) {
                            build_tree(ui, folders, Some(f.id), state, depth + 1, devices);
                        }
                    }
                }
                let devices: HashMap<i64, (String, String)> = self
                    .sources
                    .iter()
                    .map(|s| (s.id, s.device()))
                    .collect();
                let mut tree = TreeState {
                    expanded: &mut self.expanded,
                    collapsed_devices: &mut self.collapsed_devices,
                    selected: &mut sel,
                };
                build_tree(ui, &self.folders, None, &mut tree, 0, &devices);
                if sel != self.folder_sel {
                    self.folder_sel = sel;
                    self.collection_anchor = None;
                    self.dirty = true;
                }
                collection_command = self.collections_panel(ui);
                ui.separator();
                ui.label(format!("{} assets", self.assets.len()));
            });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.worker.is_some() {
                    ui.spinner();
                }
                ui.label(if self.status.is_empty() {
                    "Ready".to_string()
                } else {
                    self.status.clone()
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let name = self
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| self.path.display().to_string());
                    ui.label(egui::RichText::new(name).weak())
                        .on_hover_text(self.path.display().to_string());
                });
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

        if let Some(id) = self.selection.primary {
            self.ensure_texture(&ctx, id);
        }
        self.ensure_detail();
        let mut edit = None;
        if self.view_mode == ViewMode::Grid || self.list_details {
            egui::Panel::right("details")
                .default_size(320.0)
                .show(ui, |ui| {
                    ui.add_enabled_ui(self.worker.is_none(), |ui| {
                        edit = self.details_panel(ui);
                    });
                });
        }

        egui::CentralPanel::default().show(ui, |ui| {
            // Full-width toolbar: view switch on the left, search on the right.
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view_mode, ViewMode::Grid, "Grid");
                ui.selectable_value(&mut self.view_mode, ViewMode::List, "List");
                if self.view_mode == ViewMode::List {
                    ui.toggle_value(&mut self.list_details, "Details");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Go").clicked() {
                        self.dirty = true;
                    }
                    if ui
                        .add_enabled(!self.search.is_empty(), egui::Button::new("x").small())
                        .clicked()
                    {
                        self.search.clear();
                        self.dirty = true;
                    }
                    let mut search = self.search.clone();
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut search)
                            .hint_text("Search name or path")
                            .desired_width(220.0),
                    );
                    resp.widget_info(|| {
                        let mut info = egui::WidgetInfo::text_edit(
                            ui.is_enabled(),
                            &self.search,
                            &search,
                            "Search name or path",
                        );
                        info.label = Some("Search name or path".into());
                        info
                    });
                    if resp.changed() {
                        self.search = search.clone();
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.dirty = true;
                    }
                });
            });
            ui.separator();
            if let Some(params) = match self.view_mode {
                ViewMode::Grid => self.grid(ui),
                ViewMode::List => self.list(ui),
            } {
                collection_command = Some(params);
            }
        });
        self.textures
            .retain(|id, _| self.visible_thumbnails.contains(id));
        self.attempted_thumbnails
            .retain(|id| self.visible_thumbnails.contains(id));
        self.list_metadata
            .retain(|id, _| self.visible_thumbnails.contains(id));
        if let Some(params) = edit {
            self.run_command("edit", &params);
        }
        if let Some(params) = collection_command {
            self.run_command("collection", &params);
        }
        self.create_collection_modal(&ctx);
        if let Some(asset) = egui::DragAndDrop::payload::<DraggedAsset>(&ctx) {
            if let Some(pointer) = ctx.pointer_hover_pos() {
                egui::Area::new(egui::Id::new("dragged_asset"))
                    .order(egui::Order::Tooltip)
                    .interactable(false)
                    .fixed_pos(pointer + egui::vec2(12.0, 12.0))
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.set_max_width(240.0);
                            ui.add(egui::Label::new(&asset.name).truncate());
                        });
                    });
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn app() -> App {
        let ctx = open_catalogue(std::path::Path::new(":memory:")).unwrap();
        ctx.catalog().connection().execute_batch(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10000)
             INSERT INTO asset(uid,path,name) SELECT 'uid-'||x, '/synthetic/'||x||'.png', x||'.png' FROM n;"
        ).unwrap();
        let (_, receiver) = std::sync::mpsc::channel();
        App::with_menu(ctx, ":memory:".into(), None, receiver)
    }

    #[test]
    fn grid_only_requests_visible_thumbnails_in_large_catalogue() {
        let mut app = app();
        let context = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        };
        let mut output = context.run_ui(input.clone(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.grid(ui);
            });
        });
        output.textures_delta.clear();
        let attempted = app.attempted_thumbnails.len();
        assert!(
            attempted > 0 && attempted < 100,
            "requested {attempted} of 10,000 thumbnails"
        );
        let mut output = context.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.grid(ui);
            });
        });
        output.textures_delta.clear();
        assert_eq!(app.attempted_thumbnails.len(), attempted);
        // Missing thumbnails are remembered rather than queried every frame.
        assert!(app.textures.is_empty());
    }

    #[test]
    fn refresh_error_preserves_previous_results_and_reports_failure() {
        let mut app = app();
        app.ctx
            .catalog()
            .connection()
            .execute_batch("ALTER TABLE source RENAME TO unavailable_source")
            .unwrap();
        app.refresh();
        assert_eq!(app.assets.len(), 10000);
        assert!(app.status.contains("Cannot refresh catalogue"));
    }

    #[test]
    fn opening_invalid_catalogue_is_an_error() {
        let path =
            std::env::temp_dir().join(format!("icatalog-ui-invalid-{}.sqlite", std::process::id()));
        std::fs::write(&path, b"not a SQLite database").unwrap();
        assert!(open_catalogue(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
