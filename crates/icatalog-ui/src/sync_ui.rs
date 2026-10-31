//! Desktop sync presentation and local connection preferences. The registered
//! L3 commands own all transfer, validation, conflict and backup behaviour.
use super::{worker, App, Context, FolderSelection, Params};
use eframe::egui;
use icatalog_engine::{EngineError, Outcome};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_URL_BYTES: usize = 8192;

pub(super) struct SyncDialog {
    pub clone: bool,
    pub url: String,
    pub destination: Option<PathBuf>,
    pub acknowledge: bool,
    pub pending: bool,
    pub error: Option<String>,
    pub result: Option<String>,
    focus_url: bool,
    last_rect: Option<egui::Rect>,
}

pub(super) struct SyncJob {
    path: PathBuf,
    url: String,
    clone: bool,
    selected_collections: Option<Vec<String>>,
}

fn settings_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".sync-url");
    PathBuf::from(name)
}

pub(super) fn load_url(path: &Path) -> Result<String, String> {
    let path = settings_path(path);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_file() || metadata.len() > MAX_URL_BYTES as u64 {
        return Err("connection settings must be a regular file of at most 8 KiB".into());
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_URL_BYTES as u64 + 1).read_to_string(&mut text))
        .map_err(|e| e.to_string())?;
    if text.len() > MAX_URL_BYTES {
        return Err("connection settings exceed 8 KiB".into());
    }
    icatalog_sync::validate_server_url(text.trim()).map_err(|e| e.to_string())?;
    Ok(text.trim().into())
}

fn save_url(path: &Path, url: &str) -> Result<(), String> {
    if url.len() > MAX_URL_BYTES {
        return Err("server address exceeds 8 KiB".into());
    }
    icatalog_sync::validate_server_url(url).map_err(|e| e.to_string())?;
    let settings = settings_path(path);
    // TempPath reserves a unique sibling name and cleans it on every error.
    let temp =
        icatalog_sync::util::TempPath::reserve(&settings, "settings").map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(temp.path())
        .map_err(|e| e.to_string())?;
    file.write_all(url.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(temp.path(), settings).map_err(|e| e.to_string())?;
    Ok(())
}

impl App {
    pub(super) fn open_sync_dialog(&mut self, clone: bool) {
        if self.worker.is_some() || self.create_collection.is_some() || self.sync_dialog.is_some() {
            return;
        }
        self.pending_collection_toggle = None;
        self.sync_dialog = Some(SyncDialog {
            clone,
            url: self.sync_url.clone(),
            destination: None,
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
        let modal_id = egui::Id::new("client_sync");
        let area = egui::Modal::default_area(modal_id)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 32.0));
        let response = egui::Modal::new(modal_id).area(area).show(ctx, |ui| {
            ui.set_width(430.0);
            ui.heading(if dialog.clone { "Connect to Server" } else { "Sync catalogue" });
            ui.add_enabled_ui(!dialog.pending, |ui| {
                let label = ui.label("Server address");
                let address = ui.add(egui::TextEdit::singleline(&mut dialog.url)
                    .hint_text("http://nas:8080").desired_width(f32::INFINITY)
                    .char_limit(MAX_URL_BYTES)).labelled_by(label.id);
                if dialog.focus_url {
                    address.request_focus();
                    dialog.focus_url = false;
                }
                if address.changed() {
                    dialog.error = None;
                    dialog.result = None;
                }
                let enter = address.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.small("Use your trusted local network. The server currently supports HTTP.");
                if dialog.clone {
                    ui.label("Download a new local catalogue, ready for browsing and edits.");
                    ui.label("Local catalogue");
                    if let Some(path) = &dialog.destination {
                        ui.add(egui::Label::new(path.display().to_string()).wrap());
                    }
                    if ui.button("Choose local file…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("iCatalog catalogue", &["sqlite"])
                            .set_file_name("Library.sqlite").save_file() {
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
                    egui::ScrollArea::vertical().id_salt("sync_result")
                        .max_height(150.0).min_scrolled_height(150.0).auto_shrink([false, false]).show(ui, |ui| {
                            if let Some(error) = &dialog.error {
                                ui.add(egui::Label::new(egui::RichText::new(error)
                                    .color(ui.visuals().error_fg_color)).wrap().selectable(true));
                            }
                            if let Some(result) = &dialog.result {
                                ui.add(egui::Label::new(result).wrap().selectable(true));
                            }
                        });
                }
                ui.horizontal(|ui| {
                    close = ui.button(if dialog.result.is_some() { "Close" } else { "Cancel" }).clicked();
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
        if !dialog.pending && (close || response.should_close()) {
            self.sync_dialog = None;
            self.set_menu_enabled(self.worker.is_none());
        } else if submit && !dialog.pending {
            self.start_sync();
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
        let mut selected_collections = None;
        let prepared = (|| -> Result<(), String> {
            if url.len() > MAX_URL_BYTES {
                return Err("server address exceeds 8 KiB".into());
            }
            icatalog_sync::validate_server_url(&url).map_err(|e| e.to_string())?;
            if !clone {
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
        let mut params = Params::new();
        params.set("url", &url);
        match worker::Worker::start(
            path.clone(),
            if clone { "clone" } else { "sync" }.into(),
            params,
        ) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.sync_job = Some(SyncJob {
                    path,
                    url,
                    clone,
                    selected_collections,
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
        match Context::open(&self.path) {
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
            if let Err(error) = save_url(&self.path, &self.sync_url) {
                message.push_str(&format!("\nCannot remember server address: {error}"));
            }
        }
        if let Some(error) = &self.catalogue_error {
            message.push_str(&format!("\n{error}"));
        }
        self.status = message.lines().next().unwrap_or("Sync finished").to_owned();
        if let Some(dialog) = &mut self.sync_dialog {
            dialog.pending = false;
            if result.is_ok() {
                dialog.result = Some(message);
            } else {
                dialog.error = Some(message);
            }
        }
        self.textures.clear();
        self.attempted_thumbnails.clear();
        self.detail_for = None;
        self.dirty = true;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn connection_preferences_round_trip_replace_and_reject_corrupt_or_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalogue.sqlite");
        assert_eq!(load_url(&path).unwrap(), "");
        save_url(&path, "http://localhost:8080").unwrap();
        assert_eq!(load_url(&path).unwrap(), "http://localhost:8080");
        save_url(&path, "http://localhost:8081").unwrap();
        assert_eq!(load_url(&path).unwrap(), "http://localhost:8081");
        assert!(!path.exists());
        for bytes in [
            vec![0xff],
            vec![b'x'; MAX_URL_BYTES + 1],
            b"https://unsupported".to_vec(),
        ] {
            std::fs::write(settings_path(&path), bytes).unwrap();
            assert!(load_url(&path).is_err());
        }
        assert!(save_url(&path, "https://unsupported").is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn connection_preferences_refuse_symlink_reads_and_replace_links_without_touching_targets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalogue.sqlite");
        let target = dir.path().join("sentinel");
        std::fs::write(&target, b"preserve me").unwrap();
        std::os::unix::fs::symlink(&target, settings_path(&path)).unwrap();
        assert!(load_url(&path).is_err());
        save_url(&path, "http://localhost:8080").unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"preserve me");
        assert_eq!(load_url(&path).unwrap(), "http://localhost:8080");
    }
}
