//! Desktop sync presentation and local connection preferences. The registered
//! L3 commands own all transfer, validation, conflict and backup behaviour.
use super::{worker, App, Context, FolderSelection, Params};
use eframe::egui;
use icatalog_engine::{EngineError, Outcome};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_URL_BYTES: usize = 8192;
const MAX_SETTINGS_BYTES: usize = MAX_URL_BYTES * 3 + 512;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SyncPreferences {
    pub catalogue_id: String,
    pub url: String,
    pub check_on_open: bool,
    pub remember_security_files: bool,
    pub token_file: Option<String>,
    pub ca_file: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSyncPreferences {
    version: u32,
    catalogue_id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    check_on_open: bool,
    #[serde(default)]
    remember_security_files: bool,
    #[serde(default)]
    token_file: Option<String>,
    #[serde(default)]
    ca_file: Option<String>,
}

pub(super) struct SyncDialog {
    pub clone: bool,
    pub require_replica: bool,
    pub url: String,
    pub destination: Option<PathBuf>,
    pub token_file: Option<PathBuf>,
    pub ca_file: Option<PathBuf>,
    pub remember_security_files: bool,
    pub acknowledge: bool,
    pub pending: bool,
    pub error: Option<String>,
    pub result: Option<String>,
    focus_url: bool,
    last_rect: Option<egui::Rect>,
}

pub(super) struct SyncJob {
    pub(super) path: PathBuf,
    pub(super) url: String,
    pub(super) clone: bool,
    pub(super) untracked: bool,
    pub(super) selected_collections: Option<Vec<String>>,
    pub(super) selected_assets: Option<super::selection::SelectedUids>,
}

pub(super) struct ConflictDialog {
    pub conflicts: Vec<icatalog_engine::SyncConflict>,
    pub session_id: Option<String>,
    pub total_conflicts: usize,
    pub page: usize,
    pub page_size: usize,
    pub selected_conflict: usize,
    pub all_choices_made: bool,
    pub recovery_backup: Option<PathBuf>,
    pub url: String,
    pub error: Option<String>,
    pub token_file: Option<PathBuf>,
    pub ca_file: Option<PathBuf>,
    pub remember_security_files: bool,
}

fn settings_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".sync.toml");
    PathBuf::from(name)
}

fn legacy_settings_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".sync-url");
    PathBuf::from(name)
}

pub(super) fn load_preferences(path: &Path, catalogue_id: &str) -> Result<SyncPreferences, String> {
    let settings = settings_path(path);
    let metadata = match std::fs::symlink_metadata(&settings) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let legacy = legacy_settings_path(path);
            match std::fs::symlink_metadata(&legacy) {
                Ok(m) => {
                    if !m.is_file() || m.len() > MAX_URL_BYTES as u64 {
                        return Err(
                            "connection settings must be a regular file of at most 8 KiB".into(),
                        );
                    }
                    let mut text = String::new();
                    std::fs::File::open(&legacy)
                        .and_then(|f| f.take(MAX_URL_BYTES as u64 + 1).read_to_string(&mut text))
                        .map_err(|e| e.to_string())?;
                    let url = text.trim().to_owned();
                    if !url.is_empty() {
                        icatalog_sync::validate_server_url(&url).map_err(|e| e.to_string())?;
                    }
                    let prefs = SyncPreferences {
                        catalogue_id: catalogue_id.into(),
                        url,
                        ..Default::default()
                    };
                    save_preferences(path, &prefs)?;
                    return Ok(prefs);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(SyncPreferences {
                        catalogue_id: catalogue_id.into(),
                        ..Default::default()
                    });
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_file() || metadata.len() > MAX_SETTINGS_BYTES as u64 {
        return Err("connection settings must be a regular file of at most 24.5 KiB".into());
    }
    let mut text = String::new();
    std::fs::File::open(settings)
        .and_then(|f| {
            f.take(MAX_SETTINGS_BYTES as u64 + 1)
                .read_to_string(&mut text)
        })
        .map_err(|e| e.to_string())?;
    if text.len() > MAX_SETTINGS_BYTES {
        return Err("connection settings exceed 24.5 KiB".into());
    }
    let stored: StoredSyncPreferences =
        toml::from_str(&text).map_err(|_| "invalid sync preferences".to_owned())?;
    if stored.version != 1 {
        return Err("unsupported sync preferences version".into());
    }
    if stored.catalogue_id != catalogue_id {
        return Err("sync preferences belong to a different catalogue".into());
    }
    if !stored.url.is_empty() {
        icatalog_sync::validate_server_url(&stored.url).map_err(|e| e.to_string())?;
    }
    for value in [stored.token_file.as_deref(), stored.ca_file.as_deref()]
        .into_iter()
        .flatten()
    {
        if value.len() > MAX_URL_BYTES || !Path::new(value).is_absolute() {
            return Err("remembered security file paths must be absolute and at most 8 KiB".into());
        }
    }
    Ok(SyncPreferences {
        catalogue_id: stored.catalogue_id,
        url: stored.url,
        check_on_open: stored.check_on_open,
        remember_security_files: stored.remember_security_files,
        token_file: stored.token_file,
        ca_file: stored.ca_file,
    })
}

pub(super) fn save_preferences(path: &Path, prefs: &SyncPreferences) -> Result<(), String> {
    if prefs.url.len() > MAX_URL_BYTES {
        return Err("server address exceeds 8 KiB".into());
    }
    if prefs.catalogue_id.is_empty() || prefs.catalogue_id.len() > 1024 {
        return Err("catalogue identity is invalid".into());
    }
    if !prefs.url.is_empty() {
        icatalog_sync::validate_server_url(&prefs.url).map_err(|e| e.to_string())?;
    }
    let settings = settings_path(path);
    // TempPath reserves a unique sibling name and cleans it on every error.
    let temp =
        icatalog_sync::util::TempPath::reserve(&settings, "settings").map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(temp.path())
        .map_err(|e| e.to_string())?;
    for value in [prefs.token_file.as_deref(), prefs.ca_file.as_deref()]
        .into_iter()
        .flatten()
    {
        if value.len() > MAX_URL_BYTES || !Path::new(value).is_absolute() {
            return Err("security file paths must be absolute and at most 8 KiB".into());
        }
    }
    let stored = StoredSyncPreferences {
        version: 1,
        catalogue_id: prefs.catalogue_id.clone(),
        url: prefs.url.clone(),
        check_on_open: prefs.check_on_open,
        remember_security_files: prefs.remember_security_files,
        token_file: if prefs.remember_security_files {
            prefs.token_file.clone()
        } else {
            None
        },
        ca_file: if prefs.remember_security_files {
            prefs.ca_file.clone()
        } else {
            None
        },
    };
    let body = toml::to_string(&stored).map_err(|_| "cannot encode sync preferences".to_owned())?;
    file.write_all(body.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(temp.path(), settings).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
pub(super) fn load_url(path: &Path, catalogue_id: &str) -> Result<String, String> {
    Ok(load_preferences(path, catalogue_id)?.url)
}

#[cfg(test)]
fn save_url(path: &Path, catalogue_id: &str, url: &str) -> Result<(), String> {
    let settings = settings_path(path);
    let mut prefs = match std::fs::symlink_metadata(&settings) {
        Ok(metadata) if metadata.file_type().is_symlink() => SyncPreferences {
            catalogue_id: catalogue_id.into(),
            ..Default::default()
        },
        Ok(_) => load_preferences(path, catalogue_id)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            load_preferences(path, catalogue_id)?
        }
        Err(error) => return Err(error.to_string()),
    };
    prefs.url = url.to_owned();
    save_preferences(path, &prefs)
}

fn preference_file_path(path: Option<&Path>, remember: bool) -> Result<Option<String>, String> {
    if !remember {
        return Ok(None);
    }
    let Some(path) = path else { return Ok(None) };
    let value = path
        .to_str()
        .ok_or_else(|| "security file path must be valid UTF-8 to remember it".to_owned())?;
    if value.len() > MAX_URL_BYTES || !path.is_absolute() {
        return Err("security file path must be absolute and at most 8 KiB".into());
    }
    Ok(Some(value.to_owned()))
}

// Native pickers supply paths only; the L3 command reads and validates file contents.
fn security_file(ui: &mut egui::Ui, choose: &str, clear: &str, path: &mut Option<PathBuf>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.button(choose).clicked() {
            if let Some(selected) = rfd::FileDialog::new().pick_file() {
                *path = Some(selected);
                changed = true;
            }
        }
        if path.is_some() && ui.button(clear).clicked() {
            *path = None;
            changed = true;
        }
    });
    if let Some(path) = path {
        super::middle_ellipsis::label(ui, &path.display().to_string());
    }
    changed
}

impl App {
    pub(super) fn sync_check_consent(&mut self, ctx: &egui::Context) {
        let Some(check) = self.sync_decision.as_ref() else {
            return;
        };
        let mut sync_now = false;
        let mut keep_read_only = false;
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("opening_sync_decision")).show(ctx, |ui| {
            super::spacing::dialog(ui);
            ui.set_width(430.0);
            close = super::dialog::header(ui, "Server changes are available", true);
            ui.label("Server updates or unsynced edits were found. Sync now or browse this catalogue read-only.");
            if check.sync_required {
                ui.strong(
                    "This catalogue has a required sync that must finish before edits resume.",
                );
            }
            if !check.pending.is_empty() {
                ui.label(format!(
                    "{} local change group(s) are waiting to sync.",
                    check.pending.len()
                ));
            }
            if !check.conflicts.is_empty() {
                ui.label(format!(
                    "{} conflict group(s) need a choice.",
                    check.conflicts.len()
                ));
            }
            ui.horizontal(|ui| {
                sync_now = ui.button("Sync now…").clicked();
                keep_read_only = ui.button("Browse read-only").clicked();
            });
        });
        if sync_now {
            self.sync_decision = None;
            self.open_sync_dialog(false);
            if let Some(dialog) = &mut self.sync_dialog {
                dialog.require_replica = true;
            }
        } else if keep_read_only
            || close
            || response.should_close()
            || ctx.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.sync_decision = None;
            self.set_read_only(true);
            self.status = "Browsing read-only; sync is required before editing.".into();
        }
    }

    pub(super) fn open_sync_dialog(&mut self, clone: bool) {
        if self.preview.is_open() || self.preview.is_loading() {
            return;
        }
        if self.worker.is_some()
            || self.create_collection.is_some()
            || self.sync_dialog.is_some()
            || self.ctx.is_read_only()
        {
            return;
        }
        self.pending_collection_toggle = None;
        self.sync_dialog = Some(SyncDialog {
            clone,
            require_replica: false,
            url: self.sync_url.clone(),
            destination: None,
            token_file: self
                .sync_preferences
                .token_file
                .as_deref()
                .map(PathBuf::from),
            ca_file: self.sync_preferences.ca_file.as_deref().map(PathBuf::from),
            remember_security_files: self.sync_preferences.remember_security_files,
            acknowledge: false,
            pending: false,
            error: None,
            result: None,
            focus_url: true,
            last_rect: None,
        });
        self.set_menu_enabled(false);
    }

    pub(super) fn sync_modal(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.sync_dialog else {
            return;
        };
        let mut submit = false;
        let mut close = false;
        let mut save_settings = false;
        let mut address_changed = false;
        let modal_id = egui::Id::new("client_sync");
        let area = egui::Modal::default_area(modal_id)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 32.0));
        let response = egui::Modal::new(modal_id).area(area)
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(12))
            .show(ctx, |ui| {
            super::spacing::dialog(ui);
            ui.set_width(430.0);
            close = super::dialog::header(ui,
                if dialog.clone { "Connect to Server" } else { "Sync catalogue" },
                !dialog.pending);
            ui.add_enabled_ui(!dialog.pending, |ui| {
                let label = ui.label("Server address");
                let address = ui.add(egui::TextEdit::singleline(&mut dialog.url)
                    .hint_text("https://nas:8443").desired_width(f32::INFINITY)
                    .char_limit(MAX_URL_BYTES)).labelled_by(label.id);
                if dialog.focus_url {
                    address.request_focus();
                    dialog.focus_url = false;
                }
                if address.changed() {
                    dialog.error = None;
                    dialog.result = None;
                    dialog.token_file = None;
                    dialog.ca_file = None;
                    dialog.remember_security_files = false;
                    address_changed = true;
                }
                let enter = address.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.small("Use HTTPS for remote credentials. HTTP is available on trusted networks.");
                egui::CollapsingHeader::new("Security settings").show(ui, |ui| {
                    let changed = security_file(ui, "Choose token file…", "Clear token file", &mut dialog.token_file)
                        | security_file(ui, "Choose CA file…", "Clear CA file", &mut dialog.ca_file);
                    ui.checkbox(&mut dialog.remember_security_files, "Remember token and CA file paths on this computer");
                    ui.small("Only file paths are saved, never credentials. Tokens require HTTPS or a loopback address.");
                    if changed {
                        dialog.error = None;
                        dialog.result = None;
                    }
                });
                super::spacing::section(ui);
                if dialog.clone {
                    ui.label("Download a new local catalogue, ready for browsing and edits.");
                    ui.label("Local catalogue");
                    if let Some(path) = &dialog.destination {
                        ui.add(egui::Label::new(path.display().to_string()).wrap());
                    }
                    if ui.button("Choose local file…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("iCatalog catalogue", &["icatalog", "sqlite"])
                            .set_file_name("Library.icatalog").save_file() {
                            dialog.destination = Some(path);
                            dialog.error = None;
                            dialog.result = None;
                        }
                    }
                    ui.small("Choose a new filename in an existing local folder.");
                } else {
                    ui.label("Send your ratings, labels, keywords and Collection edits, then receive the latest server catalogue.");
                    if !self.replica {
                        ui.label(egui::RichText::new(
                            "First sync replaces this catalogue with server state. Existing local edits are kept in a backup and are not uploaded.").strong());
                        ui.checkbox(&mut dialog.acknowledge,
                            "Use server state and preserve my current catalogue in a backup");
                    }
                }
                if dialog.error.is_some() || dialog.result.is_some() {
                    ui.separator();
                    let result_height = (ctx.content_rect().bottom() - ui.cursor().top() - 92.0)
                        .clamp(40.0, 150.0);
                    egui::ScrollArea::vertical().id_salt("sync_result")
                        .max_height(result_height).min_scrolled_height(result_height).auto_shrink([false, false]).show(ui, |ui| {
                            if let Some(error) = &dialog.error {
                                ui.add(egui::Label::new(egui::RichText::new(error)
                                    .color(ui.visuals().error_fg_color)).wrap().selectable(true));
                            }
                            if let Some(result) = &dialog.result {
                                ui.add(egui::Label::new(result).wrap().selectable(true));
                            }
                        });
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    save_settings = ui.button("Save settings").clicked();
                    close |= ui.button(if dialog.result.is_some() { "Close" } else { "Cancel" }).clicked();
                    let valid = !dialog.url.trim().is_empty()
                        && if dialog.clone { dialog.destination.is_some() && dialog.result.is_none() }
                           else { self.replica || dialog.acknowledge };
                    submit = ui.add_enabled(valid, egui::Button::new(if dialog.clone { "Clone" } else { "Sync now" })).clicked()
                        || (valid && enter);
                });
            });
            if dialog.pending {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if dialog.clone { "Downloading and verifying catalogue…" } else { "Syncing with server…" });
                });
                if let Some(progress) = self.sync_progress {
                    progress_widget(ui, progress);
                }
                ui.small("Wait for this operation to finish before closing iCatalog.");
            }
        });
        if dialog.last_rect.is_none_or(|rect| {
            (rect.min - response.response.rect.min).length() > 0.5
                || (rect.size() - response.response.rect.size()).length() > 0.5
        }) {
            ctx.request_repaint();
            dialog.last_rect = Some(response.response.rect);
        }
        if save_settings {
            let saved = (|| {
                let mut prefs = self.sync_preferences.clone();
                prefs.url = dialog.url.trim().to_owned();
                prefs.remember_security_files = dialog.remember_security_files;
                prefs.token_file = preference_file_path(
                    dialog.token_file.as_deref(),
                    dialog.remember_security_files,
                )?;
                prefs.ca_file = preference_file_path(
                    dialog.ca_file.as_deref(),
                    dialog.remember_security_files,
                )?;
                save_preferences(&self.path, &prefs)?;
                Ok::<_, String>(prefs)
            })();
            match saved {
                Ok(prefs) => {
                    self.sync_preferences = prefs;
                    self.sync_url = self.sync_preferences.url.clone();
                    dialog.error = None;
                }
                Err(error) => dialog.error = Some(format!("Cannot save settings: {error}")),
            }
        }
        if !dialog.pending && (close || response.should_close()) {
            self.sync_dialog = None;
            self.set_menu_enabled(self.worker.is_none());
        } else if submit && !dialog.pending {
            self.start_sync();
        }
        if address_changed {
            self.set_check_on_open(false);
        }
    }

    pub(super) fn conflict_modal(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.conflict_dialog else {
            return;
        };
        let mut close = false;
        let mut apply = false;
        let mut choice_action = None;
        let mut open_backup = None;
        let mut page_action = None;
        let mut selected_action = None;
        let response = egui::Modal::new(egui::Id::new("sync_conflicts"))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(12))
            .show(ctx, |ui| {
                super::spacing::dialog(ui);
                ui.set_width(650.0);
                close = super::dialog::header(ui, "Resolve sync conflicts", self.worker.is_none());
                ui.label("Review each conflict and choose the value to keep. Apply choices and sync sends them to the server.");
                let url_label = ui.label("Server address");
                ui.add_enabled_ui(self.worker.is_none(), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut dialog.url).hint_text("https://nas:8443").desired_width(f32::INFINITY)).labelled_by(url_label.id);
                });
                if let Some(path) = &dialog.recovery_backup {
                    ui.collapsing("Recovery backup path", |ui| {
                        ui.add(egui::Label::new(path.display().to_string()).wrap().selectable(true));
                    });
                }
                let page_count = dialog.total_conflicts.div_ceil(dialog.page_size.max(1));
                ui.horizontal(|ui| {
                    ui.label(format!("Conflict {} of {}", dialog.page.saturating_mul(dialog.page_size).saturating_add(dialog.selected_conflict).saturating_add(1), dialog.total_conflicts));
                    if ui.add_enabled(dialog.page > 0 && self.worker.is_none(), egui::Button::new("Previous conflicts")).clicked() {
                        page_action = Some(dialog.page - 1);
                    }
                    if ui.add_enabled(dialog.page + 1 < page_count && self.worker.is_none(), egui::Button::new("Next conflicts")).clicked() {
                        page_action = Some(dialog.page + 1);
                    }
                });
                ui.columns(2, |columns| {
                    columns[0].set_min_width(190.0);
                    columns[0].strong("Conflicts");
                    egui::ScrollArea::vertical().max_height(220.0).show(&mut columns[0], |ui| {
                        for (index, conflict) in dialog.conflicts.iter().enumerate() {
                            let label = format!("{} · {}", conflict_kind(conflict.kind), conflict.display_name);
                            if ui.selectable_label(index == dialog.selected_conflict, label).clicked() {
                                selected_action = Some(index);
                            }
                        }
                    });
                    if let Some(conflict) = dialog.conflicts.get(dialog.selected_conflict) {
                        columns[1].strong(format!("{} · {}", conflict_kind(conflict.kind), conflict.display_name));
                        columns[1].label(format!("Stable ID: {}", conflict.key));
                        egui::ScrollArea::vertical().max_height(220.0).show(&mut columns[1], |ui| {
                            for shared in &conflict.shared {
                                ui.group(|ui| {
                                    ui.strong(format!("{} · shared value", shared.field));
                                    ui.label(value_label(&shared.value));
                                });
                            }
                            for field in &conflict.fields {
                                ui.group(|ui| {
                                    ui.strong(&field.field);
                                    ui.label("Your saved edits");
                                    ui.add(egui::Label::new(value_label(&field.local)).wrap());
                                    ui.label("Server version from the last sync");
                                    ui.add(egui::Label::new(value_label(&field.server)).wrap());
                                    let current = field.choice;
                                    ui.horizontal_wrapped(|ui| {
                                        for (label, choice) in [
                                            (format!("Keep your saved edits for {}", field.field), icatalog_engine::SyncChoice::Local),
                                            (format!("Keep server version for {}", field.field), icatalog_engine::SyncChoice::Server),
                                        ] {
                                            if ui.add_enabled(self.worker.is_none(), egui::Button::selectable(current == Some(choice), label)).clicked() {
                                                choice_action = Some((conflict.key.clone(), field.field.clone(), choice));
                                            }
                                        }
                                        if matches!(field.local, icatalog_engine::SyncValue::Set(_)) && matches!(field.server, icatalog_engine::SyncValue::Set(_)) {
                                            let choice = icatalog_engine::SyncChoice::Combined;
                                            if ui.add_enabled(self.worker.is_none(), egui::Button::selectable(current == Some(choice), format!("Combine both lists for {}", field.field))).clicked() {
                                                choice_action = Some((conflict.key.clone(), field.field.clone(), choice));
                                            }
                                        }
                                    });
                                    if matches!(field.local, icatalog_engine::SyncValue::Set(_)) && matches!(field.server, icatalog_engine::SyncValue::Set(_)) {
                                        ui.small("Combining makes a union of both lists; duplicate entries are removed.");
                                    }
                                });
                            }
                        });
                    } else {
                        columns[1].label("Loading conflict details…");
                    }
                });
                if let Some(error) = &dialog.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.collapsing("Security settings", |ui| {
                    ui.add_enabled_ui(self.worker.is_none(), |ui| {
                        security_file(ui, "Choose token file…", "Clear token file", &mut dialog.token_file);
                        security_file(ui, "Choose CA file…", "Clear CA file", &mut dialog.ca_file);
                        ui.checkbox(&mut dialog.remember_security_files, "Remember token and CA file paths on this computer");
                    });
                });
                ui.horizontal(|ui| {
                    close |= ui.add_enabled(self.worker.is_none(), egui::Button::new("Resolve later")).clicked();
                    if let Some(path) = &dialog.recovery_backup {
                        if ui.add_enabled(self.worker.is_none(), egui::Button::new("Open recovery backup read-only")).clicked() {
                            open_backup = Some(path.clone());
                        }
                    }
                    apply = ui.add_enabled(dialog.all_choices_made && !dialog.url.trim().is_empty() && dialog.session_id.is_some() && self.worker.is_none(), egui::Button::new("Apply choices and sync")).clicked();
                });
                if self.worker.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Saving your choice…");
                    });
                    if let Some(progress) = self.sync_progress {
                        progress_widget(ui, progress);
                    }
                }
            });
        if let Some(index) = selected_action {
            dialog.selected_conflict = index;
        } else if let Some(page) = page_action {
            self.start_conflict_page(page);
        } else if let Some((key, field, choice)) = choice_action {
            self.queue_conflict_choice(&key, &field, choice);
        } else if let Some(path) = open_backup {
            self.open_recovery_backup(path);
        } else if apply {
            self.start_resolve_apply();
        }
        if (close || response.should_close()) && self.worker.is_none() {
            self.conflict_dialog = None;
        }
    }

    pub(super) fn start_conflict_page(&mut self, page: usize) {
        if self.worker.is_some() || self.recovery_pending {
            return;
        }
        let mut params = Params::new();
        params
            .set("action", "list")
            .set("page", page.to_string())
            .set("page-size", "25");
        if let Some(session_id) = self.recovery_session_id.as_deref() {
            params.set("session-id", session_id);
        }
        match worker::Worker::start(self.path.clone(), "sync-resolve".into(), params) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.conflict_page_pending = Some(page);
                self.set_menu_enabled(false);
            }
            Err(error) => {
                if let Some(dialog) = &mut self.conflict_dialog {
                    dialog.error = Some(format!("Cannot load conflict page: {error}"));
                }
            }
        }
    }

    pub(super) fn finish_conflict_page(
        &mut self,
        page: usize,
        result: Result<Outcome, icatalog_engine::EngineError>,
    ) {
        match result {
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(resolved))) => {
                self.recovery_conflicts = resolved.conflicts.clone();
                self.recovery_session_id = resolved.session_id.clone();
                self.recovery_total_conflicts = resolved.total_conflicts;
                self.recovery_page = resolved.page;
                self.recovery_page_size = resolved.page_size.max(1);
                self.recovery_all_choices_made = resolved.all_choices_made;
                self.recovery_phase = resolved.recovery_phase.clone();
                self.recovery_pending = resolved.recovery_pending;
                if self.recovery_pending {
                    self.sync_read_only = true;
                    self.conflict_dialog = None;
                }
                self.recovery_backup = resolved.recovery_backup;
                self.recovery_phase = resolved.recovery_phase.clone();
                self.recovery_pending = resolved.recovery_pending;
                if let Some(dialog) = &mut self.conflict_dialog {
                    dialog.conflicts = resolved.conflicts;
                    dialog.session_id = resolved.session_id;
                    dialog.total_conflicts = resolved.total_conflicts;
                    dialog.page = resolved.page;
                    dialog.page_size = resolved.page_size.max(1);
                    dialog.selected_conflict = 0;
                    dialog.all_choices_made = resolved.all_choices_made;
                    dialog.recovery_backup = self.recovery_backup.clone();
                    dialog.error = None;
                }
                if self.recovery_pending {
                    self.sync_read_only = true;
                    self.conflict_dialog = None;
                    if !matches!(
                        self.recovery_phase.as_deref(),
                        Some("snapshot_ready" | "retry" | "applying" | "applied")
                    ) {
                        self.recovery_error =
                            Some("Unknown recovery phase; editing remains disabled.".into());
                    }
                }
            }
            Ok(outcome) => {
                let error = format!("Unexpected conflict page result: {}", outcome.render());
                self.recovery_error = Some(error.clone());
                self.sync_read_only = true;
                if let Some(dialog) = &mut self.conflict_dialog {
                    dialog.error = Some(error);
                }
            }
            Err(error) => {
                let error = format!("Cannot load conflict page {page}: {error}");
                self.recovery_error = Some(error.clone());
                self.sync_read_only = true;
                if let Some(dialog) = &mut self.conflict_dialog {
                    dialog.error = Some(error);
                }
            }
        }
        self.set_menu_enabled(self.worker.is_none());
    }

    pub(super) fn start_sync_recovery(&mut self) {
        if self.worker.is_some() || !self.recovery_pending {
            return;
        }
        let path = self.path.clone();
        let assets = Some(self.selection.capture_uids(&self.assets));
        let mut collections = None;
        if let FolderSelection::Collections(ids) = &self.folder_sel {
            let mut uids = Vec::new();
            for id in ids {
                match self.ctx.catalog().collection_uid_by_id(*id) {
                    Ok(Some(uid)) => uids.push(uid),
                    Ok(None) => {}
                    Err(error) => {
                        self.status = format!("Cannot preserve Collection selection: {error}");
                    }
                }
            }
            collections = Some(uids);
        }
        let placeholder = match Context::for_destination(&path) {
            Ok(ctx) => ctx,
            Err(error) => {
                self.recovery_error = Some(error.to_string());
                self.sync_read_only = true;
                self.status = format!("Cannot release catalogue for recovery: {error}");
                return;
            }
        };
        self.ctx = placeholder;
        self.recovery_selected_assets = assets;
        self.recovery_selected_collections = collections;
        let mut params = Params::new();
        params.set("action", "recover");
        match worker::Worker::start(path, "sync-resolve".into(), params) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.recovering_snapshot = true;
                self.sync_progress = None;
                self.set_menu_enabled(false);
            }
            Err(error) => {
                self.recovering_snapshot = false;
                self.reopen_after_sync();
                self.recovery_error = Some(error.to_string());
                self.sync_read_only = true;
                self.status = format!("Cannot resume interrupted sync recovery: {error}");
            }
        }
    }

    pub(super) fn finish_sync_recovery(
        &mut self,
        result: Result<Outcome, icatalog_engine::EngineError>,
    ) {
        self.recovering_snapshot = false;
        self.sync_progress = None;
        match result {
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(resolved))) => {
                self.recovery_conflicts = resolved.conflicts;
                self.recovery_backup = resolved.recovery_backup;
                self.recovery_session_id = resolved.session_id;
                self.recovery_total_conflicts = resolved.total_conflicts;
                self.recovery_page = resolved.page;
                self.recovery_page_size = resolved.page_size.max(1);
                self.recovery_all_choices_made = resolved.all_choices_made;
                self.recovery_phase = resolved.recovery_phase;
                self.recovery_pending = resolved.recovery_pending;
                if resolved.recovered {
                    self.recovery_error = None;
                }
                self.reopen_after_sync();
                if self.catalogue_error.is_none() {
                    self.textures.clear();
                    self.attempted_thumbnails.clear();
                    self.detail_for = None;
                    if let Some(assets) = self.recovery_selected_assets.take() {
                        self.selection = Default::default();
                        self.pending_asset_selection = Some(assets);
                    }
                    if let Some(uids) = self.recovery_selected_collections.take() {
                        let mut ids = Vec::new();
                        for uid in uids {
                            match self.ctx.catalog().collection_id_by_uid(&uid) {
                                Ok(Some(id)) => ids.push(id),
                                Ok(None) => {}
                                Err(error) => {
                                    self.status =
                                        format!("Cannot restore Collection selection: {error}")
                                }
                            }
                        }
                        self.folder_sel = if ids.is_empty() {
                            FolderSelection::All
                        } else {
                            FolderSelection::Collections(ids)
                        };
                    }
                    self.refresh_sync_latch();
                    if self.recovery_pending || !self.recovery_conflicts.is_empty() {
                        self.sync_read_only = true;
                    } else if resolved.recovered {
                        self.set_read_only(false);
                    }
                    self.status = if resolved.recovered {
                        "Interrupted sync recovery completed.".into()
                    } else {
                        "Recovery status refreshed.".into()
                    };
                }
            }
            Ok(outcome) => {
                self.reopen_after_sync();
                self.sync_read_only = true;
                self.recovery_phase = None;
                self.recovery_error =
                    Some(format!("Unexpected recovery result: {}", outcome.render()));
                self.status = "Interrupted sync recovery did not complete.".into();
            }
            Err(error) => {
                self.reopen_after_sync();
                self.sync_read_only = true;
                self.recovery_phase = None;
                self.recovery_error = Some(error.to_string());
                self.status = format!("Cannot resume interrupted sync recovery: {error}");
            }
        }
        self.set_menu_enabled(self.worker.is_none());
    }

    fn queue_conflict_choice(
        &mut self,
        key: &str,
        field: &str,
        choice: icatalog_engine::SyncChoice,
    ) {
        if self.worker.is_some() {
            return;
        }
        let mut params = Params::new();
        params
            .set("action", "choose")
            .set("key", key)
            .set("field", field)
            .set("choice", choice_key(choice));
        let Some(session_id) = self
            .conflict_dialog
            .as_ref()
            .and_then(|d| d.session_id.as_deref())
        else {
            if let Some(dialog) = &mut self.conflict_dialog {
                dialog.error = Some("Conflict session is still loading. Try again shortly.".into());
            }
            return;
        };
        params.set("session-id", session_id);
        match worker::Worker::start(self.path.clone(), "sync-resolve".into(), params) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.conflict_choice_pending = Some((key.to_owned(), field.to_owned(), choice));
                self.set_menu_enabled(false);
            }
            Err(error) => {
                if let Some(dialog) = &mut self.conflict_dialog {
                    dialog.error = Some(format!("Cannot save conflict choice: {error}"));
                }
            }
        }
    }

    fn start_resolve_apply(&mut self) {
        let Some(dialog) = &self.conflict_dialog else {
            return;
        };
        if self.worker.is_some() {
            return;
        }
        let mut params = Params::new();
        let Some(session_id) = dialog.session_id.as_deref() else {
            return;
        };
        params
            .set("action", "apply")
            .set("session-id", session_id)
            .set("url", dialog.url.trim());
        if let Some(path) = &dialog.token_file {
            let Some(path) = path.to_str() else {
                if let Some(d) = &mut self.conflict_dialog {
                    d.error = Some("Token file path must be valid UTF-8".into());
                }
                return;
            };
            params.set("token-file", path);
        }
        if let Some(path) = &dialog.ca_file {
            let Some(path) = path.to_str() else {
                if let Some(d) = &mut self.conflict_dialog {
                    d.error = Some("CA file path must be valid UTF-8".into());
                }
                return;
            };
            params.set("ca-file", path);
        }
        let mut prefs = self.sync_preferences.clone();
        prefs.url = dialog.url.trim().to_owned();
        prefs.remember_security_files = dialog.remember_security_files;
        prefs.token_file = match preference_file_path(
            dialog.token_file.as_deref(),
            dialog.remember_security_files,
        ) {
            Ok(path) => path,
            Err(error) => {
                if let Some(d) = &mut self.conflict_dialog {
                    d.error = Some(error);
                }
                return;
            }
        };
        prefs.ca_file =
            match preference_file_path(dialog.ca_file.as_deref(), dialog.remember_security_files) {
                Ok(path) => path,
                Err(error) => {
                    if let Some(d) = &mut self.conflict_dialog {
                        d.error = Some(error);
                    }
                    return;
                }
            };
        if let Err(error) = save_preferences(&self.path, &prefs) {
            if let Some(d) = &mut self.conflict_dialog {
                d.error = Some(format!("Cannot save settings: {error}"));
            }
            return;
        }
        self.sync_preferences = prefs;
        self.sync_url = dialog.url.trim().to_owned();
        let selected_assets = Some(self.selection.capture_uids(&self.assets));
        let path = self.path.clone();
        self.ctx = match Context::for_destination(&self.path) {
            Ok(ctx) => ctx,
            Err(error) => {
                if let Some(d) = &mut self.conflict_dialog {
                    d.error = Some(error.to_string());
                }
                return;
            }
        };
        match worker::Worker::start(path.clone(), "sync-resolve".into(), params) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.sync_progress = None;
                self.sync_job = Some(SyncJob {
                    path,
                    url: self.sync_url.clone(),
                    clone: false,
                    untracked: false,
                    selected_collections: None,
                    selected_assets,
                });
                self.set_menu_enabled(false);
            }
            Err(error) => {
                self.reopen_after_sync();
                if let Some(d) = &mut self.conflict_dialog {
                    d.error = Some(format!("Cannot start conflict resolution: {error}"));
                }
            }
        }
    }

    fn start_sync(&mut self) {
        let Some(dialog) = &self.sync_dialog else {
            return;
        };
        if self.worker.is_some() {
            return;
        }
        let clone = dialog.clone;
        let url = dialog.url.trim().to_owned();
        let path = if clone {
            let Some(path) = dialog.destination.clone() else {
                return;
            };
            path
        } else {
            if !self.replica && !dialog.acknowledge {
                return;
            }
            self.path.clone()
        };
        let mut params = Params::new();
        params.set("url", &url);
        if dialog.require_replica {
            params.set("require-replica", "true");
        }
        let mut selected_collections = None;
        let mut selected_assets = None;
        let prepared = (|| -> Result<(), String> {
            if url.len() > MAX_URL_BYTES {
                return Err("server address exceeds 8 KiB".into());
            }
            icatalog_sync::validate_server_url(&url).map_err(|e| e.to_string())?;
            for (key, label, path) in [
                ("token-file", "Token file", dialog.token_file.as_deref()),
                ("ca-file", "CA file", dialog.ca_file.as_deref()),
            ] {
                if let Some(path) = path {
                    let path = path
                        .to_str()
                        .ok_or_else(|| format!("{label} path must be valid UTF-8"))?;
                    if path.len() > MAX_URL_BYTES {
                        return Err(format!("{label} path exceeds 8 KiB"));
                    }
                    params.set(key, path);
                }
            }
            if !clone {
                selected_assets = Some(self.selection.capture_uids(&self.assets));
                if let FolderSelection::Collections(ids) = &self.folder_sel {
                    let mut uids = Vec::new();
                    for id in ids {
                        if let Some(uid) = self
                            .ctx
                            .catalog()
                            .collection_uid_by_id(*id)
                            .map_err(|e| e.to_string())?
                        {
                            uids.push(uid);
                        }
                    }
                    selected_collections = Some(uids);
                }
                // Release the desktop reader before the worker swaps the file.
                // This also makes replacement possible on Windows.
                let placeholder =
                    Context::for_destination(&self.path).map_err(|e| e.to_string())?;
                self.ctx = placeholder;
            }
            Ok(())
        })();
        if let Err(error) = prepared {
            if let Some(d) = &mut self.sync_dialog {
                d.error = Some(error);
            }
            return;
        }
        match worker::Worker::start(
            path.clone(),
            if clone { "clone" } else { "sync" }.into(),
            params,
        ) {
            Ok(worker) => {
                self.sync_progress = None;
                self.worker = Some(worker);
                self.sync_job = Some(SyncJob {
                    path,
                    url,
                    clone,
                    untracked: !self.replica,
                    selected_collections,
                    selected_assets,
                });
                if let Some(d) = &mut self.sync_dialog {
                    d.pending = true;
                    d.error = None;
                    d.result = None;
                }
                self.status = if clone {
                    "Cloning catalogue…"
                } else {
                    "Syncing catalogue…"
                }
                .into();
                self.set_menu_enabled(false);
            }
            Err(error) => {
                if !clone {
                    self.reopen_after_sync();
                }
                if let Some(d) = &mut self.sync_dialog {
                    d.error = Some(format!("Cannot start sync: {error}"));
                }
            }
        }
    }

    pub(super) fn reopen_after_sync(&mut self) {
        match worker::open_initialized_existing(&self.path) {
            Ok(ctx) => {
                self.ctx = ctx;
                self.catalogue_error = None;
                self.dirty = true;
            }
            Err(error) => {
                self.catalogue_error =
                    Some(format!("Cannot reopen {}: {error}", self.path.display()));
            }
        }
    }

    pub(super) fn finish_sync(&mut self, result: Result<Outcome, EngineError>) {
        let Some(job) = self.sync_job.take() else {
            return;
        };
        let conflicts = match &result {
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Run(run))) => run.conflicts.clone(),
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(resolved))) => {
                resolved.conflicts.clone()
            }
            _ => Vec::new(),
        };
        let resolver_metadata = match &result {
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(resolved))) => Some((
                resolved.session_id.clone(),
                resolved.total_conflicts,
                resolved.page,
                resolved.page_size,
                resolved.all_choices_made,
            )),
            _ => None,
        };
        let resolved_all = matches!(
            &result,
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(resolved)))
                if resolved.applied && resolved.conflicts.is_empty()
        );
        match &result {
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(resolved))) => {
                self.recovery_backup = resolved.recovery_backup.clone();
                self.recovery_phase = resolved.recovery_phase.clone();
                self.recovery_pending = resolved.recovery_pending;
            }
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Run(run))) => {
                self.recovery_backup = run.backup.as_deref().map(PathBuf::from);
            }
            _ => {}
        }
        if job.clone && result.is_ok() {
            self.path = job.path;
            self.folder_sel = FolderSelection::All;
            self.selection = Default::default();
            self.collection_anchor = None;
            self.search.clear();
            self.expanded.clear();
            self.collapsed_devices.clear();
        }
        if !job.clone || result.is_ok() {
            self.reopen_after_sync();
        }
        if result.is_ok() && self.catalogue_error.is_none() {
            if let Ok(Some(row)) = self.ctx.catalog().catalogue() {
                self.sync_preferences.catalogue_id = row.id;
            }
        }
        if resolved_all {
            self.recovery_conflicts.clear();
        }
        self.refresh_sync_latch();
        let completed_without_deferral = match &result {
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Run(run))) => {
                !run.text.to_ascii_lowercase().starts_with("sync deferred:")
            }
            Ok(Outcome::Sync(icatalog_engine::SyncOutcome::Resolve(resolved))) => {
                resolved.applied && resolved.conflicts.is_empty()
            }
            Ok(_) => true,
            Err(_) => false,
        };
        if completed_without_deferral {
            self.sync_check_error = None;
            if resolved_all {
                self.recovery_error = None;
            }
            self.recovery_pending = false;
            self.recovery_phase = None;
        }
        if completed_without_deferral
            && matches!(self.ctx.sync_required(), Ok(false))
            && conflicts.is_empty()
            && self.recovery_conflicts.is_empty()
            && !self.ctx.is_read_only()
        {
            self.set_read_only(false);
        }
        if job.untracked && !job.clone && matches!(self.ctx.catalog().is_replica(), Ok(true)) {
            if matches!(
                self.folder_sel,
                FolderSelection::Folder(_) | FolderSelection::Device(_)
            ) {
                self.folder_sel = FolderSelection::All;
            }
            self.expanded.clear();
            self.collapsed_devices.clear();
        }
        if let Some(selected) = job.selected_assets {
            // Row ids may now refer to different assets, including after a
            // backup-first sync of an unrelated local catalogue.
            self.selection = Default::default();
            self.pending_asset_selection = Some(selected);
        }
        let mut message = match &result {
            Ok(outcome) => outcome.render(),
            Err(error) => error.to_string(),
        };
        if let Some(uids) = job.selected_collections {
            // Local numeric Collection ids can change when concurrent creations
            // are reconciled. Keep the selected identity, never a reused row id.
            let restored = (|| -> icatalog_engine::Result<Vec<i64>> {
                let mut ids = Vec::new();
                for uid in uids {
                    if let Some(id) = self.ctx.catalog().collection_id_by_uid(&uid)? {
                        ids.push(id);
                    }
                }
                Ok(ids)
            })();
            self.collection_anchor = None;
            self.folder_sel = match restored {
                Ok(ids) if !ids.is_empty() => FolderSelection::Collections(ids),
                Ok(_) => FolderSelection::All,
                Err(error) => {
                    message.push_str(&format!("\nCannot restore Collection selection: {error}"));
                    FolderSelection::All
                }
            };
        }
        // Remember sync attempts too, including offline deferrals, for retry.
        // A failed clone never changes the current catalogue's connection.
        if !job.clone || result.is_ok() {
            self.sync_url = job.url;
            self.sync_preferences.url = self.sync_url.clone();
            if let Some(dialog) = &self.sync_dialog {
                self.sync_preferences.remember_security_files = dialog.remember_security_files;
                self.sync_preferences.token_file = dialog
                    .remember_security_files
                    .then(|| {
                        dialog
                            .token_file
                            .as_deref()
                            .and_then(Path::to_str)
                            .map(str::to_owned)
                    })
                    .flatten();
                self.sync_preferences.ca_file = dialog
                    .remember_security_files
                    .then(|| {
                        dialog
                            .ca_file
                            .as_deref()
                            .and_then(Path::to_str)
                            .map(str::to_owned)
                    })
                    .flatten();
            }
            if let Err(error) = save_preferences(&self.path, &self.sync_preferences) {
                message.push_str(&format!("\nCannot remember server address: {error}"));
            }
        }
        let reopen_error = self.catalogue_error.clone();
        if let Some(error) = &reopen_error {
            message.push_str(&format!("\n{error}"));
        }
        self.status = reopen_error
            .unwrap_or_else(|| message.lines().next().unwrap_or("Sync finished").to_owned());
        if let Some(dialog) = &mut self.sync_dialog {
            dialog.pending = false;
            if result.is_ok() && self.catalogue_error.is_none() {
                dialog.result = Some(message);
            } else {
                dialog.error = Some(message);
            }
        }
        if !conflicts.is_empty() {
            self.recovery_conflicts = conflicts.clone();
            self.sync_read_only = true;
            let (session_id, total_conflicts, page, page_size, all_choices_made) =
                resolver_metadata.unwrap_or((None, conflicts.len(), 0, 25, false));
            self.recovery_session_id = session_id.clone();
            self.recovery_total_conflicts = total_conflicts.max(conflicts.len());
            self.recovery_page = page;
            self.recovery_page_size = page_size.max(1);
            self.recovery_all_choices_made = all_choices_made;
            self.conflict_dialog = Some(ConflictDialog {
                conflicts,
                session_id,
                total_conflicts: total_conflicts.max(self.recovery_conflicts.len()),
                page,
                page_size: page_size.max(1),
                selected_conflict: 0,
                all_choices_made,
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
            self.sync_dialog = None;
            if self.recovery_session_id.is_none() {
                self.start_conflict_page(0);
            }
        } else if resolved_all {
            self.recovery_conflicts.clear();
            self.recovery_session_id = None;
            self.recovery_total_conflicts = 0;
            self.recovery_all_choices_made = false;
            self.conflict_dialog = None;
        } else if let Err(error) = &result {
            if let Some(dialog) = &mut self.conflict_dialog {
                dialog.error = Some(error.to_string());
            }
        }
        self.textures.clear();
        self.attempted_thumbnails.clear();
        self.detail_for = None;
        self.dirty = true;
    }
}

pub(super) fn progress_label(phase: icatalog_engine::SyncPhase) -> &'static str {
    match phase {
        icatalog_engine::SyncPhase::Preparing => "Preparing sync…",
        icatalog_engine::SyncPhase::Checking => "Checking server…",
        icatalog_engine::SyncPhase::Uploading => "Uploading edits…",
        icatalog_engine::SyncPhase::Downloading => "Downloading catalogue…",
        icatalog_engine::SyncPhase::Validating => "Validating catalogue…",
        icatalog_engine::SyncPhase::BackingUp => "Saving recovery backup…",
        icatalog_engine::SyncPhase::Replacing => "Replacing local catalogue…",
        icatalog_engine::SyncPhase::Complete => "Sync complete",
        icatalog_engine::SyncPhase::Deferred => "Sync deferred",
    }
}

pub(super) fn progress_widget(ui: &mut egui::Ui, progress: icatalog_engine::SyncProgress) {
    let label = progress_label(progress.phase);
    match (progress.completed, progress.total) {
        (Some(done), Some(total)) => {
            let fraction = if total == 0 {
                0.0
            } else {
                (done as f32 / total as f32).clamp(0.0, 1.0)
            };
            ui.add(
                egui::ProgressBar::new(fraction).text(format!("{label} {done} / {total} bytes")),
            );
        }
        (Some(done), None) if progress.phase == icatalog_engine::SyncPhase::Downloading => {
            ui.add(
                egui::ProgressBar::new(0.0)
                    .animate(true)
                    .text(format!("Downloading catalogue… {done} bytes")),
            );
        }
        _ => {
            ui.add(egui::ProgressBar::new(0.0).animate(true).text(label));
        }
    }
}

fn choice_key(choice: icatalog_engine::SyncChoice) -> &'static str {
    match choice {
        icatalog_engine::SyncChoice::Local => "local",
        icatalog_engine::SyncChoice::Server => "server",
        icatalog_engine::SyncChoice::Combined => "combined",
    }
}

fn conflict_kind(kind: icatalog_engine::SyncEntityKind) -> &'static str {
    match kind {
        icatalog_engine::SyncEntityKind::Asset => "Asset",
        icatalog_engine::SyncEntityKind::Collection => "Collection",
    }
}

const MAX_RENDERED_CONFLICT_VALUE_CHARS: usize = 512;
const MAX_RENDERED_SET_ITEMS: usize = 5;
const MAX_RENDERED_SET_ITEM_CHARS: usize = 80;

fn bounded_value_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut rendered: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        rendered.push('…');
    }
    rendered
}

fn value_label(value: &icatalog_engine::SyncValue) -> String {
    match value {
        icatalog_engine::SyncValue::Text(value) => {
            bounded_value_text(value, MAX_RENDERED_CONFLICT_VALUE_CHARS)
        }
        icatalog_engine::SyncValue::Integer(value) => value.to_string(),
        icatalog_engine::SyncValue::Set(values) => {
            let mut rendered = String::new();
            for value in values.iter().take(MAX_RENDERED_SET_ITEMS) {
                if !rendered.is_empty() {
                    rendered.push_str(", ");
                }
                rendered.push_str(&bounded_value_text(value, MAX_RENDERED_SET_ITEM_CHARS));
            }
            if values.len() > MAX_RENDERED_SET_ITEMS {
                rendered.push_str(&format!(
                    " … (+{} more)",
                    values.len() - MAX_RENDERED_SET_ITEMS
                ));
            }
            bounded_value_text(&rendered, MAX_RENDERED_CONFLICT_VALUE_CHARS)
        }
        icatalog_engine::SyncValue::Rules(rules) => {
            let mut fields = Vec::new();
            for (label, value) in [
                ("format", rules.format.as_deref()),
                ("captured from", rules.captured_from.as_deref()),
                ("captured to", rules.captured_to.as_deref()),
                ("camera", rules.camera.as_deref()),
                ("lens", rules.lens.as_deref()),
                ("keyword", rules.keyword.as_deref()),
                ("text", rules.text.as_deref()),
            ] {
                if let Some(value) = value {
                    fields.push(format!("{label}: {}", bounded_value_text(value, 48)));
                }
            }
            for (label, value) in [
                ("min rating", rules.min_rating),
                ("max rating", rules.max_rating),
                ("label", rules.label),
            ] {
                if let Some(value) = value {
                    fields.push(format!("{label}: {value}"));
                }
            }
            bounded_value_text(&fields.join(" · "), MAX_RENDERED_CONFLICT_VALUE_CHARS)
        }
        icatalog_engine::SyncValue::None => "(empty)".into(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const CATALOGUE_ID: &str = "synthetic-catalogue";

    #[test]
    fn conflict_value_rendering_bounds_large_text_and_sets() {
        let text = value_label(&icatalog_engine::SyncValue::Text("t".repeat(100_000)));
        assert!(text.chars().count() <= MAX_RENDERED_CONFLICT_VALUE_CHARS + 1);
        assert!(text.ends_with('…'));

        let values = (0..100_000)
            .map(|index| format!("tag-{index:05}"))
            .collect();
        let set = value_label(&icatalog_engine::SyncValue::Set(values));
        assert!(set.chars().count() <= MAX_RENDERED_CONFLICT_VALUE_CHARS + 1);
        assert!(set.contains("+99995 more"));
        assert!(!set.contains("tag-99999"));
    }

    #[test]
    fn connection_preferences_round_trip_replace_and_reject_corrupt_or_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalogue.sqlite");
        assert_eq!(load_url(&path, CATALOGUE_ID).unwrap(), "");
        save_url(&path, CATALOGUE_ID, "http://localhost:8080").unwrap();
        assert_eq!(
            load_url(&path, CATALOGUE_ID).unwrap(),
            "http://localhost:8080"
        );
        save_url(&path, CATALOGUE_ID, "https://localhost:8443").unwrap();
        assert_eq!(
            load_url(&path, CATALOGUE_ID).unwrap(),
            "https://localhost:8443"
        );
        assert!(!path.exists());
        for bytes in [
            vec![0xff],
            vec![b'x'; MAX_SETTINGS_BYTES + 1],
            b"ftp://unsupported".to_vec(),
            b"https://user:secret@nas:8443".to_vec(),
            b"https://nas:8443?token=secret".to_vec(),
        ] {
            std::fs::write(settings_path(&path), bytes).unwrap();
            assert!(load_url(&path, CATALOGUE_ID).is_err());
        }
        assert!(save_url(&path, CATALOGUE_ID, "ftp://unsupported").is_err());
        assert!(save_url(&path, CATALOGUE_ID, "https://user:secret@nas:8443").is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn preferences_are_versioned_and_legacy_url_migrates_with_check_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalogue.icatalog");
        let legacy = legacy_settings_path(&path);
        std::fs::write(&legacy, "https://nas:8443\n").unwrap();
        let migrated = load_preferences(&path, CATALOGUE_ID).unwrap();
        assert_eq!(migrated.url, "https://nas:8443");
        assert!(!migrated.check_on_open);
        assert_eq!(migrated.catalogue_id, CATALOGUE_ID);
        assert_eq!(load_preferences(&path, CATALOGUE_ID).unwrap(), migrated);
        let mut changed = migrated;
        changed.check_on_open = true;
        changed.remember_security_files = true;
        changed.token_file = Some("/tmp/token.file".into());
        changed.ca_file = Some("/tmp/ca.pem".into());
        save_preferences(&path, &changed).unwrap();
        assert_eq!(load_preferences(&path, CATALOGUE_ID).unwrap(), changed);
        assert!(load_preferences(&path, "another-catalogue").is_err());
        assert!(std::fs::read_to_string(settings_path(&path))
            .unwrap()
            .contains("version = 1"));
    }

    #[cfg(unix)]
    #[test]
    fn connection_preferences_refuse_symlink_reads_and_replace_links_without_touching_targets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalogue.sqlite");
        let target = dir.path().join("sentinel");
        std::fs::write(&target, b"preserve me").unwrap();
        std::os::unix::fs::symlink(&target, settings_path(&path)).unwrap();
        assert!(load_url(&path, CATALOGUE_ID).is_err());
        save_url(&path, CATALOGUE_ID, "http://localhost:8080").unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"preserve me");
        assert_eq!(
            load_url(&path, CATALOGUE_ID).unwrap(),
            "http://localhost:8080"
        );
    }
}
