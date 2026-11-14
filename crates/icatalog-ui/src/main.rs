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

mod dates;
mod dialog;
mod fieldset;
mod formatting;
mod interaction;
mod list_row;
mod middle_ellipsis;
mod preview;
mod rating;
mod selection;
mod smart_collection;
mod spacing;
mod sync_ui;
mod typography;
#[cfg(test)]
mod ui_tests;
mod worker;

use eframe::egui;
use icatalog_catalog::{
    Annotation, Asset, AssetMeta, AssetQuery, Collection, Folder, Source, APP_NAME,
};
use icatalog_engine::{dispatch_guarded, Context, Params};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
    let argument = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions::default();
    #[cfg(target_os = "macos")]
    {
        // Finder delivers documents as Apple events, often during launch and
        // never as an argv path. Keep the handler alive until winit returns.
        let (registration, inbox) = fmv_macos_events::Registration::install();
        let result = eframe::run_native(
            APP_NAME,
            options,
            Box::new(move |cc| {
                inbox.set_wake({
                    let repaint = cc.egui_ctx.clone();
                    move || repaint.request_repaint()
                });
                let mut finder_paths = Vec::new();
                let mut quit_requested = false;
                for event in inbox.drain() {
                    match event {
                        fmv_macos_events::Event::Open(paths) => finder_paths.extend(paths),
                        fmv_macos_events::Event::Quit => quit_requested = true,
                    }
                }
                let finder_count = finder_paths.len();
                let finder_path = finder_paths.pop();
                let default = || default_catalogue_path(&macos_default_directory());
                let bundled = std::env::current_exe()
                    .ok()
                    .is_some_and(|exe| is_macos_bundle(&exe));
                let explicit = argument
                    .filter(|path| !ignore_finder_launch_argument(path, bundled, path.exists()));
                let (path, ctx, error) = if let Some(path) = finder_path {
                    match Context::open_existing(&path) {
                        Ok(ctx) => (path, ctx, None),
                        Err(err) => {
                            let fallback = explicit.unwrap_or_else(default);
                            let ctx = open_catalogue(&fallback)?;
                            (
                                fallback,
                                ctx,
                                Some(format!("Cannot open Finder catalogue: {err}")),
                            )
                        }
                    }
                } else {
                    let path = explicit.unwrap_or_else(default);
                    let ctx = open_catalogue(&path)?;
                    (path, ctx, None)
                };
                let mut app = App::new(cc, ctx, path);
                app.mac_events = Some(inbox);
                if let Some(error) = error {
                    app.status = error;
                } else if finder_count > 1 && app.status.is_empty() {
                    app.status = format!(
                        "Opened the last of {finder_count} catalogues sent by Finder; only one can be open."
                    );
                }
                if quit_requested {
                    cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Ok(Box::new(app))
            }),
        );
        drop(registration);
        result
    }
    #[cfg(not(target_os = "macos"))]
    {
        let path = argument.unwrap_or_else(|| default_catalogue_path(Path::new(".")));
        let ctx =
            open_catalogue(&path).map_err(|error| eframe::Error::AppCreation(Box::new(error)))?;
        eframe::run_native(
            APP_NAME,
            options,
            Box::new(move |cc| Ok(Box::new(App::new(cc, ctx, path)))),
        )
    }
}

#[cfg(target_os = "macos")]
fn macos_default_directory() -> PathBuf {
    let bundled = std::env::current_exe()
        .ok()
        .is_some_and(|exe| is_macos_bundle(&exe));
    if bundled {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(".")
    }
}

#[cfg(target_os = "macos")]
fn is_macos_bundle(executable: &Path) -> bool {
    executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .is_some_and(|bundle| bundle.extension().is_some_and(|ext| ext == "app"))
}

#[cfg(target_os = "macos")]
fn is_finder_process_serial_number(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Some(value) = path.as_os_str().as_bytes().strip_prefix(b"-psn_") else {
        return false;
    };
    let mut parts = value.split(|byte| *byte == b'_');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(first), Some(second), None) => {
            !first.is_empty()
                && !second.is_empty()
                && first.iter().all(u8::is_ascii_digit)
                && second.iter().all(u8::is_ascii_digit)
        }
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn ignore_finder_launch_argument(path: &Path, bundled: bool, exists: bool) -> bool {
    bundled && !exists && is_finder_process_serial_number(path)
}

fn default_catalogue_path(dir: &Path) -> PathBuf {
    let preferred = dir.join("catalogue.icatalog");
    if preferred.exists() {
        return preferred;
    }
    let older = dir.join("catalogue.sqlite");
    if older.exists() {
        return older;
    }
    preferred
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
    _quit: muda::MenuItem,
    import: muda::MenuItem,
    update: muda::MenuItem,
    connect: muda::MenuItem,
    sync: muda::MenuItem,
}
#[cfg(not(target_os = "macos"))]
type NativeMenu = ();

struct App {
    ctx: Context,
    dates: dates::DateFormatter,
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
    pending_collection_toggle: Option<PendingCollectionToggle>,
    expanded: HashSet<i64>,
    collapsed_devices: HashSet<String>,
    view_mode: ViewMode,
    list_details: bool,
    list_metadata: HashMap<i64, Option<list_row::Metadata>>,
    selection: selection::AssetSelection,
    pending_asset_selection: Option<selection::SelectedUids>,
    marquee: Option<selection::Marquee>,
    textures: HashMap<i64, egui::TextureHandle>,
    preview: preview::Viewer,
    annotation: Option<Annotation>,
    keywords: Vec<String>,
    keyword_input: String,
    meta: Option<AssetMeta>,
    detail_for: Option<i64>,
    status: String,
    dirty: bool,
    replica: bool,
    search: String,
    /// Native (macOS) menu bar; kept alive for the app's lifetime. `None` when
    /// it could not be installed, in which case the in-app menu bar is shown.
    menu: Option<NativeMenu>,
    /// Menu-item ids delivered from the native menu.
    menu_events: std::sync::mpsc::Receiver<String>,
    worker: Option<worker::Worker>,
    sync_dialog: Option<sync_ui::SyncDialog>,
    sync_job: Option<sync_ui::SyncJob>,
    sync_progress: Option<icatalog_engine::SyncProgress>,
    opening_check: bool,
    opening_recovery: bool,
    recovering_snapshot: bool,
    opening_check_pending: bool,
    sync_decision: Option<icatalog_engine::SyncCheckResult>,
    recovery_conflicts: Vec<icatalog_engine::SyncConflict>,
    recovery_backup: Option<PathBuf>,
    recovery_session_id: Option<String>,
    recovery_total_conflicts: usize,
    recovery_page: usize,
    recovery_page_size: usize,
    recovery_all_choices_made: bool,
    conflict_page_pending: Option<usize>,
    recovery_phase: Option<String>,
    recovery_pending: bool,
    recovery_selected_assets: Option<selection::SelectedUids>,
    recovery_selected_collections: Option<Vec<String>>,
    backup_return_path: Option<PathBuf>,
    conflict_dialog: Option<sync_ui::ConflictDialog>,
    conflict_choice_pending: Option<(String, String, icatalog_engine::SyncChoice)>,
    sync_url: String,
    sync_preferences: sync_ui::SyncPreferences,
    sync_read_only: bool,
    sync_latch_error: Option<String>,
    sync_check_error: Option<String>,
    recovery_error: Option<String>,
    offline_editable: bool,
    catalogue_error: Option<String>,
    attempted_thumbnails: HashSet<i64>,
    visible_thumbnails: HashSet<i64>,
    #[cfg(target_os = "macos")]
    mac_events: Option<fmv_macos_events::Inbox>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogueAccessState {
    Checking,
    Editable,
    OfflineEditable,
    SyncRequired,
    CheckFailed,
    Syncing,
    ResolutionRequired,
    RecoveryRequired,
    BackupReadOnly,
}

struct PendingCollectionToggle {
    id: i64,
    deadline: f64,
}

#[derive(Default)]
struct CreateCollectionDialog {
    name: String,
    smart: bool,
    editing_id: Option<i64>,
    rename_id: Option<i64>,
    rules: smart_collection::RuleInputs,
    preview: smart_collection::RulePreview,
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
        use muda::accelerator::{Accelerator, Code, Modifiers};
        use muda::{Menu, MenuEvent, MenuItem, Submenu};

        let menu = Menu::new();
        let app_menu = Submenu::new(APP_NAME, true);
        let file_menu = Submenu::new("File", true);
        let quit = MenuItem::with_id(
            "app.quit",
            format!("Quit {APP_NAME}"),
            true,
            Some(Accelerator::new(Modifiers::META, Code::KeyQ)),
        );
        let import = MenuItem::with_id("file.import", "Import…", true, None::<Accelerator>);
        let update = MenuItem::with_id("file.update", "Update", true, None::<Accelerator>);
        let connect = MenuItem::with_id(
            "file.connect",
            "Connect to Server…",
            true,
            None::<Accelerator>,
        );
        let sync = MenuItem::with_id("file.sync", "Sync…", true, None::<Accelerator>);

        // Route Quit/⌘Q through the same pending-worker close guard as the
        // window button. The predefined item calls NSApp terminate directly.
        let built = app_menu.append(&quit).is_ok()
            && file_menu
                .append_items(&[&import, &update, &connect, &sync])
                .is_ok()
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
        // AppKit can replace the top-level item's title with the executable
        // name during installation. Set both it and the submenu title afterward.
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSApplication;
            use objc2_foundation::{MainThreadMarker, NSString};
            let mtm = MainThreadMarker::new()?;
            let app = NSApplication::sharedApplication(mtm);
            if let Some(item) = app.mainMenu().and_then(|menu| menu.itemAtIndex(0)) {
                let title = NSString::from_str(APP_NAME);
                item.setTitle(&title);
                if let Some(app_menu) = item.submenu() {
                    app_menu.setTitle(&title);
                }
            }
        }
        Some(NativeMenu {
            _menu: menu,
            _quit: quit,
            import,
            update,
            connect,
            sync,
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
        typography::configure(&cc.egui_ctx);
        interaction::configure(&cc.egui_ctx);
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
        Self::with_menu_and_refresh_result(ctx, path, menu, menu_events).0
    }

    fn with_menu_and_refresh_result(
        ctx: Context,
        path: PathBuf,
        menu: Option<NativeMenu>,
        menu_events: std::sync::mpsc::Receiver<String>,
    ) -> (Self, icatalog_engine::Result<()>) {
        let catalogue_id = ctx
            .catalog()
            .catalogue()
            .ok()
            .flatten()
            .map(|row| row.id)
            .unwrap_or_default();
        let saved_preferences = sync_ui::load_preferences(&path, &catalogue_id);
        let mut app = App {
            ctx,
            dates: dates::DateFormatter::system(),
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
            pending_collection_toggle: None,
            expanded: HashSet::new(),
            collapsed_devices: HashSet::new(),
            view_mode: ViewMode::Grid,
            list_details: false,
            list_metadata: HashMap::new(),
            selection: selection::AssetSelection::default(),
            pending_asset_selection: None,
            marquee: None,
            textures: HashMap::new(),
            preview: preview::Viewer::default(),
            annotation: None,
            keywords: Vec::new(),
            keyword_input: String::new(),
            meta: None,
            detail_for: None,
            status: String::new(),
            dirty: true,
            replica: false,
            search: String::new(),
            menu,
            menu_events,
            worker: None,
            sync_dialog: None,
            sync_job: None,
            sync_progress: None,
            opening_check: false,
            opening_recovery: false,
            recovering_snapshot: false,
            opening_check_pending: false,
            sync_decision: None,
            recovery_conflicts: Vec::new(),
            recovery_backup: None,
            recovery_session_id: None,
            recovery_total_conflicts: 0,
            recovery_page: 0,
            recovery_page_size: 25,
            recovery_all_choices_made: false,
            conflict_page_pending: None,
            recovery_phase: None,
            recovery_pending: false,
            recovery_selected_assets: None,
            recovery_selected_collections: None,
            backup_return_path: None,
            conflict_dialog: None,
            conflict_choice_pending: None,
            sync_url: saved_preferences
                .as_ref()
                .map(|prefs| prefs.url.clone())
                .unwrap_or_default(),
            sync_preferences: saved_preferences.as_ref().cloned().unwrap_or_else(|_| {
                sync_ui::SyncPreferences {
                    catalogue_id: catalogue_id.clone(),
                    ..Default::default()
                }
            }),
            sync_read_only: false,
            sync_latch_error: None,
            sync_check_error: None,
            recovery_error: None,
            offline_editable: false,
            catalogue_error: None,
            attempted_thumbnails: HashSet::new(),
            visible_thumbnails: HashSet::new(),
            #[cfg(target_os = "macos")]
            mac_events: None,
        };
        let refresh_result = app.try_refresh();
        if let Err(error) = &refresh_result {
            app.status = format!("Cannot refresh catalogue: {error}");
        }
        if let Err(error) = saved_preferences {
            let detail = error.to_string();
            app.sync_read_only = true;
            app.sync_check_error = Some(detail.clone());
            app.status = format!("Cannot read saved sync settings: {detail}");
        }
        app.refresh_sync_latch();
        app.opening_check_pending = app.sync_check_error.is_none()
            && app.sync_preferences.check_on_open
            && !app.sync_url.is_empty();
        let has_sync_state = app.ctx.sync_state_path().is_some_and(|path| path.is_file());
        if has_sync_state {
            let mut params = Params::new();
            params.set("action", "list");
            match worker::Worker::start(app.path.clone(), "sync-resolve".into(), params) {
                Ok(worker) => {
                    app.worker = Some(worker);
                    app.opening_recovery = true;
                    app.status = "Checking for saved sync recovery…".into();
                }
                Err(error) => {
                    app.sync_read_only = true;
                    app.recovery_error = Some(error.to_string());
                    app.status = format!("Cannot check saved sync recovery: {error}");
                }
            }
        } else if app.opening_check_pending {
            app.start_opening_sync_check();
        }
        app.set_menu_enabled(app.worker.is_none());
        (app, refresh_result)
    }

    fn refresh(&mut self) {
        if let Err(error) = self.try_refresh() {
            self.status = format!("Cannot refresh catalogue: {error}");
        }
    }

    fn try_refresh(&mut self) -> icatalog_engine::Result<()> {
        let replica = self.ctx.catalog().is_replica()?;
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
            folder_recursive: true,
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
        self.replica = replica;
        self.set_menu_enabled(
            self.worker.is_none() && self.create_collection.is_none() && self.sync_dialog.is_none(),
        );
        self.list_metadata.clear();
        if let Some(selected) = self.pending_asset_selection.take() {
            self.selection.restore_uids(selected, &self.assets);
        } else {
            self.selection.retain(&self.assets);
        }
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
        if matches!(id, "sync" | "sync-resolve") {
            if self.ctx.is_read_only() {
                self.status = "This recovery backup is read-only.".into();
                return;
            }
        } else if !self.can_edit_catalogue() {
            self.status = match self.catalogue_access_state() {
                CatalogueAccessState::ResolutionRequired => {
                    "Resolve saved sync conflicts before editing.".into()
                }
                CatalogueAccessState::CheckFailed => {
                    "The sync check failed; this catalogue remains read-only.".into()
                }
                _ => {
                    "This catalogue is open read-only. Sync with the server before editing.".into()
                }
            };
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

    fn catalogue_access_state(&self) -> CatalogueAccessState {
        if self.ctx.is_read_only() || self.backup_return_path.is_some() {
            CatalogueAccessState::BackupReadOnly
        } else if self.opening_check || self.opening_recovery {
            CatalogueAccessState::Checking
        } else if self.sync_job.is_some() || self.recovering_snapshot {
            CatalogueAccessState::Syncing
        } else if self.recovery_pending {
            match self.recovery_phase.as_deref() {
                Some("snapshot_ready" | "retry" | "applying" | "applied") => {
                    CatalogueAccessState::RecoveryRequired
                }
                _ => CatalogueAccessState::CheckFailed,
            }
        } else if !self.recovery_conflicts.is_empty() {
            CatalogueAccessState::ResolutionRequired
        } else if self.catalogue_error.is_some()
            || self.sync_latch_error.is_some()
            || self.sync_check_error.is_some()
            || self.recovery_error.is_some()
        {
            CatalogueAccessState::CheckFailed
        } else if self.sync_read_only {
            CatalogueAccessState::SyncRequired
        } else if self.offline_editable {
            CatalogueAccessState::OfflineEditable
        } else {
            CatalogueAccessState::Editable
        }
    }

    fn can_edit_catalogue(&self) -> bool {
        matches!(
            self.catalogue_access_state(),
            CatalogueAccessState::Editable | CatalogueAccessState::OfflineEditable
        )
    }

    fn can_edit(&self) -> bool {
        self.can_edit_catalogue()
            && self.worker.is_none()
            && self.create_collection.is_none()
            && !self.preview.is_open()
    }

    fn can_open_preview(&self) -> bool {
        self.worker.is_none()
            && self.create_collection.is_none()
            && self.sync_dialog.is_none()
            && self.conflict_dialog.is_none()
            && self.sync_decision.is_none()
            && self.catalogue_error.is_none()
            && !self.preview.is_open()
            && !self.preview.is_loading()
    }

    fn open_preview(&mut self, id: i64) {
        if self.can_open_preview() {
            self.pending_collection_toggle = None;
            self.preview.start(&self.path, id);
        }
    }

    fn set_check_on_open(&mut self, enabled: bool) {
        let previous = self.sync_preferences.check_on_open;
        self.sync_preferences.check_on_open = enabled;
        if enabled
            && (self.sync_preferences.token_file.is_some()
                || self.sync_preferences.ca_file.is_some())
        {
            self.sync_preferences.remember_security_files = true;
        }
        if let Err(error) = sync_ui::save_preferences(&self.path, &self.sync_preferences) {
            self.sync_preferences.check_on_open = previous;
            self.status = format!("Cannot save sync preferences: {error}");
        }
    }

    fn set_read_only(&mut self, enabled: bool) {
        if !enabled {
            if self.sync_check_error.is_some() || self.recovery_error.is_some() {
                self.status =
                    "Sync status or recovery could not be verified; editing stays disabled.".into();
                self.sync_read_only = true;
                return;
            }
            match self.ctx.sync_required() {
                Ok(false) => {}
                Ok(true) => {
                    self.status = "Sync with the server before editing this catalogue.".into();
                    return;
                }
                Err(error) => {
                    self.sync_latch_error = Some(error.to_string());
                    self.sync_read_only = true;
                    self.status =
                        format!("Cannot verify sync requirement: {error}; editing stays disabled.");
                    return;
                }
            }
        }
        self.sync_read_only = enabled;
        if enabled {
            self.offline_editable = false;
        }
        self.set_menu_enabled(self.worker.is_none());
    }

    fn refresh_sync_latch(&mut self) {
        match self.ctx.sync_required() {
            Ok(true) => self.sync_read_only = true,
            Ok(false) => {
                self.sync_latch_error = None;
                if self.sync_check_error.is_none() && self.recovery_error.is_none() {
                    self.sync_read_only = false;
                }
            }
            Err(error) => {
                self.sync_read_only = true;
                self.sync_latch_error = Some(error.to_string());
                self.status =
                    format!("Cannot verify sync requirement: {error}; catalogue opened read-only.");
            }
        }
    }

    fn start_opening_sync_check(&mut self) {
        self.sync_check_error = None;
        self.offline_editable = false;
        let mut params = Params::new();
        params.set("url", self.sync_url.clone());
        if self.sync_preferences.remember_security_files {
            if let Some(path) = &self.sync_preferences.token_file {
                params.set("token-file", path);
            }
            if let Some(path) = &self.sync_preferences.ca_file {
                params.set("ca-file", path);
            }
        }
        match worker::Worker::start(self.path.clone(), "sync-check".into(), params) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.opening_check = true;
                self.opening_check_pending = false;
                self.status = "Checking for server changes…".into();
            }
            Err(error) => {
                self.opening_check_pending = false;
                self.sync_read_only = true;
                self.sync_check_error = Some(error.to_string());
                self.status = format!("Cannot start opening sync check: {error}");
            }
        }
    }

    fn show_conflict_resolution(&mut self) {
        if (self.recovery_conflicts.is_empty()
            && self.recovery_phase.as_deref() != Some("applying"))
            || self.conflict_dialog.is_some()
            || self.worker.is_some()
        {
            return;
        }
        self.conflict_dialog = Some(sync_ui::ConflictDialog {
            conflicts: self.recovery_conflicts.clone(),
            session_id: self.recovery_session_id.clone(),
            total_conflicts: self
                .recovery_total_conflicts
                .max(self.recovery_conflicts.len()),
            page: self.recovery_page,
            page_size: self.recovery_page_size,
            selected_conflict: 0,
            all_choices_made: self.recovery_all_choices_made,
            recovery_backup: self.recovery_backup.clone(),
            url: self.sync_url.clone(),
            error: None,
            token_file: self
                .sync_preferences
                .token_file
                .as_deref()
                .map(PathBuf::from),
            ca_file: self.sync_preferences.ca_file.as_deref().map(PathBuf::from),
            remember_security_files: self.sync_preferences.remember_security_files,
        });
        if self.recovery_session_id.is_none() {
            self.start_conflict_page(0);
        }
    }

    fn open_recovery_backup(&mut self, backup: PathBuf) {
        if self.worker.is_some() {
            return;
        }
        match Context::open_read_only(&backup) {
            Ok(ctx) => {
                self.backup_return_path = Some(self.path.clone());
                self.path = backup;
                self.ctx = ctx;
                self.sync_read_only = true;
                self.sync_latch_error = None;
                self.sync_check_error = None;
                self.recovery_error = None;
                self.conflict_dialog = None;
                self.folder_sel = FolderSelection::All;
                self.selection = Default::default();
                self.collection_anchor = None;
                self.search.clear();
                self.dirty = true;
                self.status = "Opened recovery backup in read-only mode.".into();
                self.set_menu_enabled(true);
            }
            Err(error) => self.status = format!("Cannot open recovery backup: {error}"),
        }
    }

    fn return_from_recovery_backup(&mut self) {
        let Some(path) = self.backup_return_path.take() else {
            return;
        };
        match worker::open_initialized_existing(&path) {
            Ok(ctx) => {
                self.path = path;
                self.ctx = ctx;
                self.sync_read_only = false;
                self.sync_latch_error = None;
                self.sync_check_error = None;
                self.recovery_error = None;
                self.refresh_sync_latch();
                self.dirty = true;
                self.status = "Returned to the current catalogue.".into();
                self.set_menu_enabled(true);
            }
            Err(error) => {
                self.backup_return_path = Some(path);
                self.status = format!("Cannot return to current catalogue: {error}");
            }
        }
    }

    fn set_menu_enabled(&self, enabled: bool) {
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.menu {
            menu.import
                .set_enabled(enabled && !self.replica && self.can_edit_catalogue());
            menu.update
                .set_enabled(enabled && !self.replica && self.can_edit_catalogue());
            menu.connect
                .set_enabled(enabled && self.catalogue_error.is_none() && !self.ctx.is_read_only());
            menu.sync
                .set_enabled(enabled && self.catalogue_error.is_none() && !self.ctx.is_read_only());
        }
        #[cfg(not(target_os = "macos"))]
        let _ = enabled;
    }

    fn close_allowed(&mut self) -> bool {
        if self.worker.is_some() {
            self.status = "A command is running; wait for it to finish before closing.".into();
            false
        } else {
            true
        }
    }

    #[cfg(target_os = "macos")]
    fn handle_mac_events(&mut self, egui_ctx: &egui::Context) {
        let Some(inbox) = &self.mac_events else {
            return;
        };
        for event in inbox.drain() {
            match event {
                fmv_macos_events::Event::Open(paths) => {
                    let count = paths.len();
                    if let Some(path) = paths.into_iter().last() {
                        self.open_finder_catalogue(path);
                        if count > 1 {
                            self.status.push_str(&format!(
                                " Finder sent {count} catalogues; only the last was opened."
                            ));
                        }
                    }
                }
                fmv_macos_events::Event::Quit => {
                    if self.close_allowed() {
                        egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn open_finder_catalogue(&mut self, path: PathBuf) {
        if self.worker.is_some()
            || self.sync_job.is_some()
            || self.create_collection.is_some()
            || self.sync_dialog.is_some()
            || self.catalogue_error.is_some()
            || self.preview.is_open()
            || self.preview.is_loading()
            || !self.keyword_input.is_empty()
        {
            self.status =
                "Finish the current command or draft, then reopen the catalogue from Finder."
                    .into();
            return;
        }
        let ctx = match Context::open_existing(&path) {
            Ok(ctx) => ctx,
            Err(error) => {
                self.status = format!("Cannot open Finder catalogue: {error}");
                return;
            }
        };
        let (_, empty_receiver) = std::sync::mpsc::channel();
        let (mut next, refresh_result) =
            App::with_menu_and_refresh_result(ctx, path, None, empty_receiver);
        if let Err(error) = refresh_result {
            self.status = format!("Cannot open Finder catalogue: {error}");
            return;
        }
        next.menu = self.menu.take();
        let (_, empty_receiver) = std::sync::mpsc::channel();
        next.menu_events = std::mem::replace(&mut self.menu_events, empty_receiver);
        let inbox = self.mac_events.take();
        *self = next;
        self.mac_events = inbox;
        self.set_menu_enabled(true);
        if self.status.is_empty() {
            self.status = "Opened catalogue from Finder.".into();
        }
    }

    fn poll_worker(&mut self) {
        if let Some(progress) = self
            .worker
            .as_ref()
            .and_then(worker::Worker::take_latest_progress)
        {
            self.sync_progress = Some(progress);
        }
        let Some(result) = self.worker.as_mut().and_then(worker::Worker::poll) else {
            return;
        };
        self.worker = None;
        if self.opening_recovery {
            self.opening_recovery = false;
            self.finish_local_recovery(result);
            if self.opening_check_pending && !self.recovery_pending && self.recovery_error.is_none()
            {
                self.start_opening_sync_check();
            }
            return;
        }
        if self.recovering_snapshot {
            self.recovering_snapshot = false;
            self.finish_sync_recovery(result);
            return;
        }
        if self.opening_check {
            self.opening_check = false;
            self.finish_opening_check(result);
            return;
        }
        if let Some(page) = self.conflict_page_pending.take() {
            self.finish_conflict_page(page, result);
            return;
        }
        if self.sync_job.is_some() {
            self.finish_sync(result);
            return;
        }
        if let Some((key, field, choice)) = self.conflict_choice_pending.take() {
            match result {
                Ok(icatalog_engine::Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(
                    resolved,
                ))) => {
                    if let Some(dialog) = &mut self.conflict_dialog {
                        if let Some(conflict) = dialog
                            .conflicts
                            .iter_mut()
                            .find(|conflict| conflict.key == key)
                        {
                            if let Some(value) = conflict
                                .fields
                                .iter_mut()
                                .find(|value| value.field == field)
                            {
                                value.choice = Some(choice);
                            }
                        }
                        dialog.all_choices_made = resolved.all_choices_made;
                        dialog.error = None;
                    }
                    self.recovery_all_choices_made = resolved.all_choices_made;
                }
                Ok(outcome) => {
                    if let Some(dialog) = &mut self.conflict_dialog {
                        dialog.error = Some(format!(
                            "Unexpected conflict choice result: {}",
                            outcome.render()
                        ));
                    }
                }
                Err(error) => {
                    if let Some(dialog) = &mut self.conflict_dialog {
                        dialog.error = Some(error.to_string());
                    }
                }
            }
            self.set_menu_enabled(self.worker.is_none());
            return;
        }
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

    fn finish_local_recovery(
        &mut self,
        result: Result<icatalog_engine::Outcome, icatalog_engine::EngineError>,
    ) {
        match result {
            Ok(icatalog_engine::Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(resolved))) => {
                self.recovery_error = None;
                self.recovery_conflicts = resolved.conflicts;
                self.recovery_backup = resolved.recovery_backup;
                self.recovery_session_id = resolved.session_id;
                self.recovery_total_conflicts = resolved.total_conflicts;
                self.recovery_page = resolved.page;
                self.recovery_page_size = resolved.page_size.max(1);
                self.recovery_all_choices_made = resolved.all_choices_made;
                self.recovery_phase = resolved.recovery_phase;
                self.recovery_pending = resolved.recovery_pending;
                if self.recovery_pending {
                    self.sync_read_only = true;
                    if !matches!(
                        self.recovery_phase.as_deref(),
                        Some("snapshot_ready" | "retry" | "applying" | "applied")
                    ) {
                        self.recovery_error =
                            Some("Unknown recovery phase; editing remains disabled.".into());
                    }
                    self.status = format!(
                        "Interrupted sync recovery needs attention{}.",
                        self.recovery_phase
                            .as_deref()
                            .map(|phase| format!(" ({phase})"))
                            .unwrap_or_default()
                    );
                    return;
                }
                if !self.recovery_conflicts.is_empty() {
                    self.sync_read_only = true;
                    self.status = "Saved conflicts need resolution before editing.".into();
                }
            }
            Ok(outcome) => {
                self.recovery_pending = false;
                self.recovery_phase = None;
                let error = format!("Unexpected recovery status: {}", outcome.render());
                self.sync_read_only = true;
                self.recovery_error = Some(error.clone());
                self.status = error;
            }
            Err(error) => {
                self.recovery_pending = false;
                self.recovery_phase = None;
                self.sync_read_only = true;
                self.recovery_error = Some(error.to_string());
                self.status = format!("Cannot read saved sync recovery: {error}");
            }
        }
    }

    fn finish_opening_check(
        &mut self,
        result: Result<icatalog_engine::Outcome, icatalog_engine::EngineError>,
    ) {
        self.sync_progress = None;
        match result {
            Ok(icatalog_engine::Outcome::Sync(icatalog_engine::SyncOutcome::Check(check))) => {
                self.sync_check_error = None;
                self.recovery_conflicts = check.conflicts.clone();
                self.recovery_backup = check.recovery_backup.clone();
                match &check.status {
                    icatalog_engine::SyncCheckStatus::Untracked => {
                        self.status = "Opening sync check needs a local replica; use File → Connect to Server to create one.".into();
                    }
                    icatalog_engine::SyncCheckStatus::Unreachable(reason) => {
                        self.status =
                            format!("Opening sync check could not reach the server: {reason}");
                        self.refresh_sync_latch();
                        self.offline_editable =
                            !self.sync_read_only && self.sync_latch_error.is_none();
                    }
                    icatalog_engine::SyncCheckStatus::Reachable => {
                        if !check.conflicts.is_empty() {
                            self.set_read_only(true);
                            self.offline_editable = false;
                            self.status = format!(
                                "{} conflict group(s) are saved for resolution.",
                                check.conflicts.len()
                            );
                            return;
                        }
                        let needs_sync = check.replica && check.sync_required;
                        if needs_sync {
                            self.set_read_only(true);
                            self.offline_editable = false;
                            self.sync_decision = Some(check);
                        } else {
                            self.sync_check_error = None;
                            self.offline_editable = false;
                            self.set_read_only(false);
                            self.status = "Catalogue is up to date with the server.".into();
                        }
                    }
                }
            }
            Ok(outcome) => {
                let error = format!("Unexpected opening check result: {}", outcome.render());
                self.sync_read_only = true;
                self.sync_check_error = Some(error.clone());
                self.status = error;
            }
            Err(error) => {
                self.status = format!("Opening sync check failed: {error}");
                self.sync_read_only = true;
                self.sync_check_error = Some(error.to_string());
            }
        }
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
        let mut double_clicked = None;
        let mut edit_rules = None;
        let mut rename = None;
        let selected_ids: HashSet<i64> = match &self.folder_sel {
            FolderSelection::Collections(ids) => ids.iter().copied().collect(),
            _ => HashSet::new(),
        };
        ui.separator();
        ui.label("Collections");
        if ui
            .add_enabled(self.can_edit(), egui::Button::new("New Collection…"))
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
            .auto_shrink([false, false])
            .max_height(ui.available_height())
            .show(ui, |ui| {
                for collection in &self.collections {
                    ui.push_id(collection.id, |ui| {
                        let selected = selected_ids.contains(&collection.id);
                        let count = self
                            .collection_counts
                            .get(&collection.id)
                            .copied()
                            .unwrap_or(0);
                        let display_name = if collection.rules.is_some() {
                            format!("{} (Smart)", collection.name)
                        } else {
                            collection.name.clone()
                        };
                        let row = collection_row(ui, &display_name, count, selected)
                            .response
                            .on_hover_text(format!(
                                "{} ({} {})",
                                collection.name,
                                if collection.rules.is_some() {
                                    "Smart Collection"
                                } else {
                                    "Collection"
                                },
                                collection.id
                            ));
                        if row.double_clicked() && self.can_edit() {
                            double_clicked = Some(collection.id);
                            if let Some(rules) = &collection.rules {
                                edit_rules =
                                    Some((collection.id, collection.name.clone(), rules.clone()));
                            } else {
                                rename = Some((collection.id, collection.name.clone()));
                            }
                        } else if row.clicked() {
                            clicked = Some((collection.id, ui.input(|input| input.modifiers)));
                        }
                        row.context_menu(|ui| {
                            let enabled = self.can_edit();
                            if ui
                                .add_enabled(enabled, egui::Button::new("Rename…"))
                                .clicked()
                            {
                                rename = Some((collection.id, collection.name.clone()));
                                ui.close();
                            }
                            if let Some(rules) = &collection.rules {
                                if ui
                                    .add_enabled(self.can_edit(), egui::Button::new("Edit Rules…"))
                                    .clicked()
                                {
                                    edit_rules = Some((
                                        collection.id,
                                        collection.name.clone(),
                                        rules.clone(),
                                    ));
                                    ui.close();
                                }
                            }
                        });
                        if self.can_edit() && collection.rules.is_none() {
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
        if let Some(id) = double_clicked {
            self.pending_collection_toggle = None;
            self.folder_sel = FolderSelection::Collections(vec![id]);
            self.collection_anchor = Some(id);
            self.dirty = true;
        } else if let Some((id, modifiers)) = clicked {
            let selected_collection =
                matches!(&self.folder_sel, FolderSelection::Collections(ids) if ids == &[id]);
            if selected_collection
                && !modifiers.ctrl
                && !modifiers.command
                && !modifiers.shift
                && self.can_edit()
            {
                let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
                let delay = if delay.is_finite() && delay > 0.0 {
                    delay
                } else {
                    0.5
                };
                self.pending_collection_toggle = Some(PendingCollectionToggle {
                    id,
                    deadline: ui.input(|i| i.time) + delay,
                });
            } else {
                self.pending_collection_toggle = None;
                self.select_collection(id, modifiers);
            }
        }
        if let Some((id, name, rules)) = edit_rules {
            self.create_collection = Some(CreateCollectionDialog {
                name,
                smart: true,
                editing_id: Some(id),
                rules: smart_collection::RuleInputs::from_rules(&rules),
                ..Default::default()
            });
            self.set_menu_enabled(false);
        }
        if let Some((id, name)) = rename {
            self.create_collection = Some(CreateCollectionDialog {
                name,
                rename_id: Some(id),
                focus_name: true,
                ..Default::default()
            });
            self.set_menu_enabled(false);
        }
        self.finish_collection_toggle(ui.ctx());
        command
    }

    fn finish_collection_toggle(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_collection_toggle else {
            return;
        };
        if self.worker.is_some()
            || self.create_collection.is_some()
            || !matches!(&self.folder_sel, FolderSelection::Collections(ids) if ids == &[pending.id])
        {
            self.pending_collection_toggle = None;
        } else if ctx.input(|i| i.time) >= pending.deadline {
            let id = pending.id;
            self.pending_collection_toggle = None;
            self.select_collection(id, egui::Modifiers::NONE);
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
    }

    fn create_collection_modal(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.create_collection else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("create_collection"))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(12))
            .show(ctx, |ui| {
                spacing::dialog(ui);
                ui.set_width(if dialog.editing_id.is_some() {
                    464.0
                } else if dialog.smart {
                    440.0
                } else {
                    300.0
                });
                cancel = dialog::header(
                    ui,
                    if dialog.rename_id.is_some() {
                        "Rename Collection"
                    } else if dialog.editing_id.is_some() {
                        "Edit Smart Collection"
                    } else {
                        "New Collection"
                    },
                    !dialog.pending,
                );
                ui.add_enabled_ui(!dialog.pending, |ui| {
                    let name = if dialog.rename_id.is_some() || dialog.editing_id.is_some() {
                        let label_width = if dialog.editing_id.is_some() {
                            smart_collection::RuleInputs::label_column_width(ui)
                                + f32::from(fieldset::HORIZONTAL_MARGIN)
                        } else {
                            0.0
                        };
                        egui::Grid::new("collection_name")
                            .num_columns(2)
                            .min_col_width(label_width)
                            .spacing(egui::vec2(16.0, 6.0))
                            .show(ui, |ui| {
                                let label = ui.label("Name");
                                let name = ui
                                    .add(
                                        egui::TextEdit::singleline(&mut dialog.name)
                                            .desired_width(240.0)
                                            .char_limit(1024),
                                    )
                                    .labelled_by(label.id);
                                ui.end_row();
                                name
                            })
                            .inner
                    } else {
                        let label = ui.label("Name");
                        ui.add(
                            egui::TextEdit::singleline(&mut dialog.name)
                                .desired_width(f32::INFINITY)
                                .char_limit(1024),
                        )
                        .labelled_by(label.id)
                    };
                    if dialog.focus_name {
                        name.request_focus();
                        if dialog.rename_id.is_some() {
                            if let Some(mut state) =
                                egui::text_edit::TextEditState::load(ui.ctx(), name.id)
                            {
                                state
                                    .cursor
                                    .set_char_range(Some(egui::text::CCursorRange::two(
                                        egui::text::CCursor::new(0),
                                        egui::text::CCursor::new(dialog.name.chars().count()),
                                    )));
                                state.store(ui.ctx(), name.id);
                            }
                        }
                        dialog.focus_name = false;
                    }
                    let enter = name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if dialog.editing_id.is_none() && dialog.rename_id.is_none() {
                        ui.checkbox(&mut dialog.smart, "Smart Collection");
                    }
                    if dialog.smart {
                        if dialog.editing_id.is_some() {
                            fieldset::show(ui, "Rules", |ui| dialog.rules.show(ui));
                        } else {
                            dialog.rules.show(ui);
                        }
                        dialog.preview.show(ui, &self.path, &dialog.rules);
                    }
                    if let Some(error) = &dialog.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        cancel |= ui.button("Cancel").clicked();
                        let valid = !dialog.name.trim().is_empty();
                        submit = ui
                            .add_enabled(
                                valid,
                                egui::Button::new(if dialog.rename_id.is_some() {
                                    "Rename"
                                } else if dialog.editing_id.is_some() {
                                    "Save"
                                } else {
                                    "Create"
                                }),
                            )
                            .clicked()
                            || (valid && enter);
                    });
                });
                if dialog.pending {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(if dialog.rename_id.is_some() {
                            "Renaming collection…"
                        } else if dialog.editing_id.is_some() {
                            "Saving collection…"
                        } else {
                            "Creating collection…"
                        });
                    });
                }
            });
        if !dialog.pending && (cancel || response.should_close()) {
            self.create_collection = None;
            self.set_menu_enabled(self.worker.is_none());
        } else if submit && !dialog.pending {
            let mut params = Params::new();
            if let Some(id) = dialog.rename_id {
                params
                    .set("action", "rename")
                    .set("id", id.to_string())
                    .set("name", dialog.name.trim());
            } else if let Some(id) = dialog.editing_id {
                params
                    .set("action", "set-rules")
                    .set("id", id.to_string())
                    .set("name", dialog.name.trim());
            } else {
                params
                    .set(
                        "action",
                        if dialog.smart {
                            "create-smart"
                        } else {
                            "create"
                        },
                    )
                    .set("name", dialog.name.trim());
            }
            if dialog.smart {
                dialog.rules.apply(&mut params);
            }
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
        let can_edit = self.can_edit();
        if response.double_clicked() && response.ctx.input(|input| input.modifiers.is_none()) {
            self.open_preview(id);
        }
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
            if ui
                .add_enabled(self.can_open_preview(), egui::Button::new("View preview"))
                .clicked()
            {
                self.open_preview(id);
                ui.close();
            }
            ui.separator();
            let ids = self.selection.ordered(&self.assets);
            ui.add_enabled_ui(can_edit && self.selection_membership_loaded, |ui| {
                ui.menu_button("Collections", |ui| {
                    if ids.len() > 1 {
                        ui.label(format!("{} assets selected", ids.len()));
                    }
                    if !self.collections.iter().any(|c| c.rules.is_none()) {
                        ui.label("Create a collection in the sidebar first.");
                    }
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            for collection in &self.collections {
                                if collection.rules.is_some() {
                                    continue;
                                }
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
                            &self.dates,
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
        let height = tile + ui.text_style_height(&egui::TextStyle::Body) + 6.0;
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
        if self.worker.is_some() || self.create_collection.is_some() || self.preview.is_open() {
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
        spacing::section(ui);
        middle_ellipsis::label(ui, &format!("Path: {}", asset.path));
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
        ui.label(format!("Size: {}", formatting::file_size(asset.file_size)));
        if let Some(mtime) = asset.mtime {
            ui.label(format!("Modified: {}", self.dates.modified(Some(mtime))));
        }
        if asset.missing {
            ui.colored_label(egui::Color32::RED, "missing from disk (run Update)");
        }
        if let Some(m) = &self.meta {
            spacing::section(ui);
            ui.heading("Metadata");
            if let Some(v) = &m.captured_local {
                ui.label(format!("Captured: {}", self.dates.captured(Some(v))));
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
                    s.push_str(&formatting::exposure_time(e));
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
        spacing::section(ui);
        ui.heading("Annotations");
        let rating = self.annotation.as_ref().map(|a| a.rating).unwrap_or(0);
        let label = self.annotation.as_ref().map(|a| a.label).unwrap_or(0);
        ui.horizontal_wrapped(|ui| {
            let caption = ui.label("Rating");
            let mut value = Some(rating);
            let response = rating::editor(ui, &mut value, "Rating").labelled_by(caption.id);
            if response.changed() {
                let value = value.unwrap_or(0);
                if value != rating {
                    edit = Some(edit_params(id, "rating", &value.to_string()));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
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
        ui.horizontal_wrapped(|ui| {
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
                    edit = Some(edit_params(id, "keyword-exact", &name));
                    self.keyword_input.clear();
                }
            }
            if !self.keywords.is_empty() && ui.button("Clear all").clicked() {
                edit = Some(edit_params(id, "clear-keywords", "true"));
            }
        });
        let keywords = self.keywords.clone();
        for kw in &keywords {
            ui.horizontal_wrapped(|ui| {
                ui.label(kw);
                if ui.small_button("remove").clicked() {
                    edit = Some(edit_params(id, "remove-keyword-exact", kw));
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

/// Fill the remaining sidebar width while keeping the name left aligned.
fn folder_row<'a>(
    ui: &mut egui::Ui,
    selected: bool,
    name: impl Into<egui::Atom<'a>>,
) -> egui::Response {
    ui.add(
        egui::Button::selectable(selected, (name.into(), egui::Atom::grow()))
            .min_size(egui::vec2(
                ui.available_width(),
                ui.spacing().interact_size.y,
            ))
            .truncate(),
    )
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
                egui::TextStyle::Small.resolve(ui.style()),
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
        egui::TextStyle::Body.resolve(ui.style()),
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

fn import_params(dir: &Path) -> std::result::Result<Params, &'static str> {
    let path = dir
        .to_str()
        .ok_or("Cannot import: directory path contains non-UTF-8 bytes")?;
    let mut params = Params::new();
    params.set("path", path);
    Ok(params)
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
        self.preview.poll(ui.ctx());
        #[cfg(target_os = "macos")]
        self.handle_mac_events(ui.ctx());
        // A new press either starts the second click or another interaction;
        // neither may leave a delayed deselection behind.
        if ui.input(|i| {
            i.pointer.any_pressed()
                || i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key { pressed: true, .. } | egui::Event::MouseWheel { .. }
                    )
                })
        }) {
            self.pending_collection_toggle = None;
        }
        self.visible_thumbnails.clear();
        if self.worker.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
            if ui.ctx().input(|input| input.viewport().close_requested()) && !self.close_allowed() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.sync_job.is_some() || self.catalogue_error.is_some() || self.recovering_snapshot {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.heading(APP_NAME);
                if let Some(error) = self.catalogue_error.clone() {
                    ui.label(error);
                    if ui.button("Reopen catalogue").clicked() {
                        self.reopen_after_sync();
                    }
                } else {
                    ui.spinner();
                    ui.label(if self.recovering_snapshot {
                        "Resuming interrupted sync recovery…"
                    } else {
                        "Working with the sync server…"
                    });
                    if self.opening_check || self.recovering_snapshot {
                        if let Some(progress) = self.sync_progress {
                            sync_ui::progress_widget(ui, progress);
                        }
                    }
                }
            });
            self.sync_modal(ui.ctx());
            self.sync_check_consent(ui.ctx());
            return;
        }
        if self.dirty && self.worker.is_none() {
            self.refresh();
            self.dirty = false;
        }
        let ctx = ui.ctx().clone();
        if self.worker.is_some() || self.create_collection.is_some() || self.preview.is_open() {
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
            if id == "app.quit" {
                if self.close_allowed() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                continue;
            }
            if self.worker.is_some()
                || self.create_collection.is_some()
                || self.sync_dialog.is_some()
                || self.preview.is_open()
            {
                continue;
            }
            match id.as_str() {
                "file.import" if !self.replica && self.can_edit_catalogue() => {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        action = Some(Action::Import(dir));
                    }
                }
                "file.update" if !self.replica && self.can_edit_catalogue() => {
                    action = Some(Action::Update)
                }
                "file.connect" => self.open_sync_dialog(true),
                "file.sync" => self.open_sync_dialog(false),
                _ => {}
            }
        }
        // Fallback toolbar menu when no native menu bar could be installed
        // (non-macOS today).
        if self.menu.is_none() {
            egui::Panel::top("menu").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        let available = self.worker.is_none()
                            && self.create_collection.is_none()
                            && self.sync_dialog.is_none()
                            && !self.ctx.is_read_only();
                        if ui
                            .add_enabled(available, egui::Button::new("Connect to Server…"))
                            .clicked()
                        {
                            ui.close();
                            self.open_sync_dialog(true);
                        }
                        if ui
                            .add_enabled(available, egui::Button::new("Sync…"))
                            .clicked()
                        {
                            ui.close();
                            self.open_sync_dialog(false);
                        }
                        ui.add_enabled_ui(available && !self.sync_url.trim().is_empty(), |ui| {
                            let mut check_on_open = self.sync_preferences.check_on_open;
                            if ui
                                .checkbox(&mut check_on_open, "Check for updates when opening")
                                .changed()
                            {
                                self.set_check_on_open(check_on_open);
                            }
                        });
                        ui.separator();
                        if ui
                            .add_enabled(
                                self.can_edit() && !self.replica,
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
                                self.can_edit() && !self.replica,
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
                if folder_row(ui, matches!(sel, FolderSelection::All), "All").clicked() {
                    sel = FolderSelection::All;
                }
                if folder_row(ui, matches!(sel, FolderSelection::Unassigned), "Unassigned").clicked() {
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
                            let left_device = folder_device(a, devices);
                            let right_device = folder_device(b, devices);
                            left_device
                                .1
                                .cmp(&right_device.1)
                                .then_with(|| left_device.0.cmp(&right_device.0))
                                .then_with(|| {
                                    a.path
                                        .as_deref()
                                        .unwrap_or(&a.name)
                                        .cmp(b.path.as_deref().unwrap_or(&b.name))
                                })
                        });
                    }
                    let mut last_device: Option<(Option<String>, String)> = None;
                    for f in children {
                        if parent.is_none() {
                            let (did, dev) = folder_device(f, devices);
                            if last_device.as_ref() != Some(&(did.clone(), dev.clone())) {
                                last_device = Some((did.clone(), dev.clone()));
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
                                        if folder_row(
                                                ui,
                                                is_sel,
                                                egui::RichText::new(&dev).strong(),
                                            )
                                            .on_hover_text(&dev)
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
                            if folder_row(ui, is_sel, f.name.as_str()).on_hover_text(&f.name).clicked() {
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
                egui::ScrollArea::vertical()
                    .id_salt("folder_tree")
                    .auto_shrink([false, true])
                    .max_height(ui.available_height() * 0.45)
                    .show(ui, |ui| {
                        build_tree(ui, &self.folders, None, &mut tree, 0, &devices);
                    });
                if sel != self.folder_sel {
                    self.folder_sel = sel;
                    self.collection_anchor = None;
                    self.dirty = true;
                }
                collection_command = self.collections_panel(ui);
            });

        egui::Panel::bottom("status").show(ui, |ui| {
            match self.catalogue_access_state() {
                CatalogueAccessState::BackupReadOnly => {
                    ui.horizontal(|ui| {
                        ui.label("Recovery backup open read-only.");
                        if ui.button("Return to current catalogue").clicked() {
                            self.return_from_recovery_backup();
                        }
                    });
                }
                CatalogueAccessState::CheckFailed => {
                    ui.horizontal(|ui| {
                        ui.label("Read-only: the sync status could not be verified.");
                        if ui
                            .add_enabled(
                                !self.sync_url.trim().is_empty() && self.worker.is_none(),
                                egui::Button::new("Retry check"),
                            )
                            .clicked()
                        {
                            self.start_opening_sync_check();
                        }
                        if ui.button("Connection settings…").clicked() {
                            self.open_sync_dialog(false);
                        }
                    });
                    ui.collapsing("Details", |ui| {
                        for detail in [
                            self.sync_check_error.as_deref(),
                            self.recovery_error.as_deref(),
                            self.sync_latch_error.as_deref(),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            ui.label(detail);
                        }
                        if let Some(phase) = &self.recovery_phase {
                            ui.label(format!("Recovery phase: {phase}"));
                        }
                    });
                }
                CatalogueAccessState::RecoveryRequired => {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Read-only: an interrupted sync needs recovery{}.",
                            self.recovery_phase
                                .as_deref()
                                .map(|phase| format!(" ({phase})"))
                                .unwrap_or_default()
                        ));
                        match self.recovery_phase.as_deref() {
                            Some("snapshot_ready") => {
                                if ui
                                    .add_enabled(
                                        self.worker.is_none(),
                                        egui::Button::new("Resume interrupted sync"),
                                    )
                                    .clicked()
                                {
                                    self.start_sync_recovery();
                                }
                            }
                            Some("retry" | "applied") => {
                                if ui.button("Sync now…").clicked() {
                                    self.open_sync_dialog(false);
                                }
                            }
                            Some("applying")
                                if ui.button("Apply saved choices and sync…").clicked() =>
                            {
                                self.show_conflict_resolution();
                            }
                            Some("applying") => {}
                            _ => {}
                        }
                    });
                    if let Some(error) = self.recovery_error.clone() {
                        ui.collapsing("Details", |ui| ui.label(error));
                    }
                }
                CatalogueAccessState::ResolutionRequired => {
                    ui.horizontal(|ui| {
                        ui.label("Read-only: saved sync conflicts need choices before editing.");
                        if ui.button("Resolve conflicts…").clicked() {
                            self.show_conflict_resolution();
                        }
                        if let Some(backup) = self.recovery_backup.clone() {
                            if ui.button("Open recovery backup read-only").clicked() {
                                self.open_recovery_backup(backup);
                            }
                        }
                    });
                }
                CatalogueAccessState::SyncRequired => {
                    ui.horizontal(|ui| {
                        ui.label("Read-only: sync with the server before editing.");
                        if ui.button("Sync now…").clicked() {
                            self.open_sync_dialog(false);
                        }
                        if ui.button("Connection settings…").clicked() {
                            self.open_sync_dialog(false);
                        }
                        if let Some(backup) = self.recovery_backup.clone() {
                            if ui.button("Open recovery backup read-only").clicked() {
                                self.open_recovery_backup(backup);
                            }
                        }
                    });
                }
                CatalogueAccessState::OfflineEditable => {
                    ui.horizontal(|ui| {
                        ui.label("The server couldn't be reached. You're editing offline.");
                        if ui
                            .add_enabled(
                                !self.sync_url.trim().is_empty() && self.worker.is_none(),
                                egui::Button::new("Retry check"),
                            )
                            .clicked()
                        {
                            self.start_opening_sync_check();
                        }
                        if ui.button("Sync…").clicked() {
                            self.open_sync_dialog(false);
                        }
                    });
                }
                CatalogueAccessState::Checking => {
                    ui.label("Checking local and server sync state…");
                }
                CatalogueAccessState::Syncing | CatalogueAccessState::Editable => {}
            }
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
                Action::Import(dir) => match import_params(&dir) {
                    Ok(params) => self.run_command("import", &params),
                    Err(message) => self.status = message.into(),
                },
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
                    egui::Frame::NONE.inner_margin(6).show(ui, |ui| {
                        spacing::inspector(ui);
                        egui::ScrollArea::vertical()
                            .id_salt("details_content")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add_enabled_ui(self.can_edit(), |ui| {
                                    edit = self.details_panel(ui);
                                });
                            });
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
        self.sync_modal(&ctx);
        self.conflict_modal(&ctx);
        self.preview.show(&ctx);
        self.sync_check_consent(&ctx);
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
        let mut ctx = Context::open_in_memory().unwrap();
        dispatch_guarded(&mut ctx, "init", &Params::new()).unwrap();
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

    #[test]
    fn no_argument_startup_reopens_the_previous_default() {
        let dir = tempfile::tempdir().unwrap();
        let older = dir.path().join("catalogue.sqlite");
        assert_eq!(
            default_catalogue_path(dir.path()),
            dir.path().join("catalogue.icatalog")
        );
        std::fs::write(&older, b"existing").unwrap();
        assert_eq!(default_catalogue_path(dir.path()), older);
        let preferred = dir.path().join("catalogue.icatalog");
        std::fs::write(&preferred, b"new").unwrap();
        assert_eq!(default_catalogue_path(dir.path()), preferred);
    }
}
