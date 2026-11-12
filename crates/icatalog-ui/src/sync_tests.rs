//! Real widgets and real HTTP commands over synthetic catalogues.
use super::*;

fn server(path: PathBuf) -> String {
    let server = icatalog_sync::server::bind("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    std::thread::spawn(move || {
        let mut ctx = Context::open(path).unwrap();
        let _ = icatalog_sync::server::serve_loop(server, &mut ctx);
    });
    format!("http://127.0.0.1:{port}")
}

fn menu(h: &mut Harness<'_, App>, label: &str) {
    h.get_by_label("File").click();
    settle(h);
    h.get_by_label(label).click();
    settle(h);
}

fn connect(h: &mut Harness<'_, App>, url: &str, path: PathBuf) {
    menu(h, "Connect to Server…");
    h.get_by_label("Server address").type_text(url);
    // The native save picker supplies a PathBuf; emulate that result headlessly.
    h.state_mut().sync_dialog.as_mut().unwrap().destination = Some(path);
    settle(h);
    h.get_by_label("Clone").click();
    settle(h);
}

fn sync_now(h: &mut Harness<'_, App>) {
    menu(h, "Sync…");
    h.get_by_label("Sync now").click();
    settle(h);
}

#[test]
fn reopen_after_sync_keeps_missing_and_empty_catalogues_as_errors() {
    let fixture = Fixture::new();
    let path = fixture.path();
    let mut h = fixture.harness([800.0, 600.0]);
    h.state_mut().ctx = Context::for_destination(&path).unwrap();

    std::fs::remove_file(&path).unwrap();
    h.state_mut().reopen_after_sync();
    assert!(!path.exists());
    assert!(h.state().catalogue_error.is_some());

    std::fs::write(&path, b"").unwrap();
    h.state_mut().reopen_after_sync();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    assert!(h.state().catalogue_error.is_some());

    std::fs::remove_file(&path).unwrap();
    drop(Context::open(&path).unwrap());
    h.state_mut().reopen_after_sync();
    assert!(h.state().catalogue_error.is_some());
}

#[test]
fn completed_sync_reports_an_error_if_its_catalogue_disappears_before_reopen() {
    let fixture = Fixture::new();
    let url = server(fixture.path());
    let client = fixture.dir.path().join("reopen-race.sqlite");
    let mut h = fixture.harness([800.0, 600.0]);
    connect(&mut h, &url, client.clone());
    h.get_by_label("Close").click();
    settle(&mut h);

    let blocker = Context::open(&client).unwrap();
    blocker
        .catalog()
        .connection()
        .execute_batch("BEGIN IMMEDIATE")
        .unwrap();
    menu(&mut h, "Sync…");
    h.get_by_label("Sync now").click();
    h.run_steps(3);
    assert!(h.state().sync_job.is_some());
    blocker
        .catalog()
        .connection()
        .execute_batch("ROLLBACK")
        .unwrap();
    drop(blocker);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let result = loop {
        if let Some(result) = h.state_mut().worker.as_mut().unwrap().poll() {
            break result;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert!(result.is_ok());
    h.state_mut().worker = None;
    std::fs::remove_file(&client).unwrap();
    h.state_mut().finish_sync(result);

    assert!(!client.exists());
    assert!(h.state().catalogue_error.is_some());
    assert!(h.state().status.contains("Cannot reopen"));
    let dialog = h.state().sync_dialog.as_ref().unwrap();
    assert!(dialog.result.is_none());
    assert!(dialog.error.as_ref().unwrap().contains("Cannot reopen"));
}

#[test]
fn desktop_clone_first_edits_push_pull_and_idle_refresh_the_live_session() {
    let fixture = Fixture::new();
    let server_path = fixture.dir.path().join("server.sqlite");
    std::fs::copy(fixture.path(), &server_path).unwrap();
    let url = server(server_path.clone());
    let client_path = fixture.dir.path().join("client.sqlite");
    let original_bytes = std::fs::read(fixture.path()).unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    connect(&mut h, &url, client_path.clone());
    assert_eq!(h.state().path, client_path);
    assert!(h.state().replica);
    assert!(h
        .state()
        .sync_dialog
        .as_ref()
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .contains("local replica ready"));
    assert_eq!(std::fs::read(fixture.path()).unwrap(), original_bytes);
    h.get_by_label("Close").click();
    settle(&mut h);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    h.get_by_label("Rating 5").click();
    settle(&mut h);
    h.get_by_label("New Collection…").click();
    settle(&mut h);
    h.get_by_label("Name").type_text("From desktop");
    h.get_by_label("Create").click();
    settle(&mut h);
    sync_now(&mut h);
    assert!(
        h.state().status.contains("push: accepted"),
        "{}",
        h.state().status
    );
    assert_eq!(
        h.state()
            .ctx
            .catalog()
            .annotation(1)
            .unwrap()
            .unwrap()
            .rating,
        5
    );
    let canonical = Context::open(&server_path).unwrap();
    assert_eq!(
        canonical.catalog().annotation(1).unwrap().unwrap().rating,
        5
    );
    let collections = canonical.catalog().all_collections().unwrap();
    assert_eq!(collections.len(), 1);
    canonical
        .catalog()
        .rename_collection(collections[0].id, "Changed on server")
        .unwrap();
    assert!(!h.get_by_label("Sync now").accesskit_node().is_disabled());
    let rect_before = h.get_by_label("Sync now").rect();
    h.get_by_label("Sync now").click();
    settle(&mut h);
    assert_eq!(
        h.state().collections[0].name,
        "Changed on server",
        "status: {}; dialog: {:?}; rect: {:?} -> {:?}",
        h.state().status,
        h.state()
            .sync_dialog
            .as_ref()
            .and_then(|d| d.error.as_ref()),
        rect_before,
        h.get_by_label("Sync now").rect()
    );
    h.get_by_label("Sync now").click();
    settle(&mut h);
    assert!(h.state().status.contains("already up to date"));
    assert_eq!(canonical.catalog().all_collections().unwrap().len(), 1);
    h.get_by_label("Close").click();
    settle(&mut h);
    assert_eq!(sync_ui::load_url(&client_path).unwrap(), url);
    let (_, receiver) = std::sync::mpsc::channel();
    let reopened = App::with_menu(
        Context::open(&client_path).unwrap(),
        client_path,
        None,
        receiver,
    );
    assert_eq!(reopened.sync_url, url);
    assert!(reopened.sync_dialog.is_none()); // no automatic startup sync
}

#[test]
fn clone_failure_preserves_current_catalogue_and_inputs_for_retry() {
    let fixture = Fixture::new();
    let url = server(fixture.path());
    let mut h = fixture.harness([800.0, 600.0]);
    let sentinel = fixture.dir.path().join("existing.sqlite");
    std::fs::write(&sentinel, b"preserve me").unwrap();
    connect(&mut h, &url, sentinel.clone());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"preserve me");
    assert_eq!(h.state().path, fixture.path());
    let d = h.state().sync_dialog.as_ref().unwrap();
    assert_eq!(d.url, url);
    assert!(d.error.as_ref().unwrap().contains("already exists"));
    assert!(h.state().sync_url.is_empty());
    let new = fixture.dir.path().join("retry.sqlite");
    h.state_mut().sync_dialog.as_mut().unwrap().destination = Some(new.clone());
    settle(&mut h);
    h.get_by_label("Clone").click();
    settle(&mut h);
    assert_eq!(h.state().path, new);
    assert!(h.state().replica);
    h.get_by_label("Close").click();
    settle(&mut h);
}

#[test]
fn first_sync_requires_acknowledgement_and_keeps_previous_edits_in_backup() {
    let fixture = Fixture::new();
    let server_path = fixture.dir.path().join("server.sqlite");
    std::fs::copy(fixture.path(), &server_path).unwrap();
    let url = server(server_path);
    let mut h = fixture.harness([800.0, 600.0]);
    h.state_mut()
        .run_command("edit", &edit_params(1, "rating", "4"));
    settle(&mut h);
    menu(&mut h, "Sync…");
    h.get_by_label("Server address").type_text(&url);
    settle(&mut h);
    assert!(h.get_by_label("Sync now").accesskit_node().is_disabled());
    h.get_by_label("Use server state and preserve my current catalogue in a backup")
        .click();
    settle(&mut h);
    h.get_by_label("Sync now").click();
    settle(&mut h);
    assert!(h.state().replica);
    assert_eq!(
        h.state()
            .ctx
            .catalog()
            .annotation(1)
            .unwrap()
            .map(|a| a.rating)
            .unwrap_or(0),
        0
    );
    let result = h
        .state()
        .sync_dialog
        .as_ref()
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    assert!(result.contains("earlier local edits") && result.contains("backup:"));
    let backup = std::fs::read_dir(fixture.dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".rejected-")
        })
        .unwrap();
    assert_eq!(
        Context::open(backup)
            .unwrap()
            .catalog()
            .annotation(1)
            .unwrap()
            .unwrap()
            .rating,
        4
    );
    assert!(h.get_by_label("Close").rect().bottom() < 600.0);
}

#[test]
fn first_sync_restores_asset_selection_by_uid_after_row_ids_change() {
    let fixture = Fixture::new();
    let server_path = fixture.dir.path().join("selection-server.sqlite");
    std::fs::copy(fixture.path(), &server_path).unwrap();
    let server_ctx = Context::open(&server_path).unwrap();
    let mut selected = server_ctx.catalog().asset(1).unwrap().unwrap();
    selected.folder_id = Some(2);
    server_ctx
        .catalog()
        .connection()
        .execute(
            "UPDATE asset SET uid='server-only', name='ServerOnly.bmp' WHERE id=1",
            [],
        )
        .unwrap();
    let new_id = server_ctx.catalog().insert_asset(&selected).unwrap();
    assert_ne!(new_id, 1);
    drop(server_ctx);

    let url = server(server_path);
    let mut h = fixture.harness([800.0, 600.0]);
    h.state_mut().folder_sel = FolderSelection::Folder(1);
    h.state_mut().dirty = true;
    settle(&mut h);
    h.state_mut().selection.focus(1);
    menu(&mut h, "Sync…");
    h.get_by_label("Server address").type_text(&url);
    h.get_by_label("Use server state and preserve my current catalogue in a backup")
        .click();
    settle(&mut h);
    h.get_by_label("Sync now").click();
    settle(&mut h);

    assert!(h.state().replica);
    assert_eq!(h.state().folder_sel, FolderSelection::All);
    assert_eq!(h.state().selection.primary, Some(new_id));
    assert_eq!(h.state().selection.ordered(&h.state().assets), vec![new_id]);
    assert_eq!(
        h.state().assets.iter().find(|a| a.id == 1).unwrap().uid,
        "server-only"
    );
}

#[test]
fn offline_first_sync_keeps_folder_navigation_without_replacement() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([800.0, 600.0]);
    h.state_mut().folder_sel = FolderSelection::Folder(1);
    h.state_mut().dirty = true;
    settle(&mut h);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let offline = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    menu(&mut h, "Sync…");
    h.get_by_label("Server address").type_text(&offline);
    h.get_by_label("Use server state and preserve my current catalogue in a backup")
        .click();
    settle(&mut h);
    h.get_by_label("Sync now").click();
    settle(&mut h);
    assert!(h.state().status.starts_with("sync deferred:"));
    assert_eq!(h.state().folder_sel, FolderSelection::Folder(1));
    assert!(!h.state().replica);
}

#[test]
fn offline_sync_keeps_edits_and_reports_deferral_without_claiming_success() {
    let fixture = Fixture::new();
    let url = server(fixture.path());
    let mut h = fixture.harness([1100.0, 720.0]);
    connect(&mut h, &url, fixture.dir.path().join("offline.sqlite"));
    h.get_by_label("Close").click();
    settle(&mut h);
    h.state_mut()
        .run_command("edit", &edit_params(1, "rating", "4"));
    settle(&mut h);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let offline = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    h.state_mut().sync_url = offline.clone();
    sync_now(&mut h);
    assert!(h.state().status.starts_with("sync deferred:"));
    assert_eq!(
        h.state()
            .ctx
            .catalog()
            .annotation(1)
            .unwrap()
            .unwrap()
            .rating,
        4
    );
    assert_eq!(sync_ui::load_url(&h.state().path).unwrap(), offline);
    assert!(h
        .state()
        .sync_dialog
        .as_ref()
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .starts_with("sync deferred:"));
    h.get_by_label("Close").click();
    settle(&mut h);
    h.state_mut().sync_url = url;
    sync_now(&mut h);
    assert!(h.state().status.contains("push: accepted"));
}

#[test]
fn conflicts_show_backup_and_keys_then_refresh_visible_annotations() {
    let fixture = Fixture::new();
    let url = server(fixture.path());
    let mut h = fixture.harness([800.0, 600.0]);
    connect(&mut h, &url, fixture.dir.path().join("conflict.sqlite"));
    h.get_by_label("Close").click();
    settle(&mut h);
    h.state_mut()
        .run_command("edit", &edit_params(1, "rating", "4"));
    settle(&mut h);
    let mut canonical = Context::open(fixture.path()).unwrap();
    dispatch_guarded(&mut canonical, "edit", &edit_params(1, "rating", "5")).unwrap();
    sync_now(&mut h);
    let result = h
        .state()
        .sync_dialog
        .as_ref()
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    assert!(
        result.contains("conflicting user edits")
            && result.contains("backup:")
            && result.contains("conflict: asset:red")
    );
    assert_eq!(
        h.state()
            .ctx
            .catalog()
            .annotation(1)
            .unwrap()
            .unwrap()
            .rating,
        5
    );
    assert!(h.get_by_label("Close").rect().bottom() < 600.0);
}

#[test]
fn sync_dialog_cancel_validation_and_modal_guards_preserve_the_session() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([800.0, 600.0]);
    menu(&mut h, "Connect to Server…");
    assert!(h.get_by_label("Clone").accesskit_node().is_disabled());
    h.state_mut().open_sync_dialog(false);
    assert!(h.state().sync_dialog.as_ref().unwrap().clone);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(h.state().sync_dialog.is_none());
    menu(&mut h, "Connect to Server…");
    h.get_by_label("Server address")
        .type_text("ftp://unsupported");
    h.state_mut().sync_dialog.as_mut().unwrap().destination =
        Some(fixture.dir.path().join("absent.sqlite"));
    settle(&mut h);
    h.get_by_label("Clone").click();
    settle(&mut h);
    assert!(!fixture.dir.path().join("absent.sqlite").exists());
    assert_eq!(h.state().path, fixture.path());
    assert!(h
        .state()
        .sync_dialog
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .to_ascii_lowercase()
        .contains("http"));
    h.get_by_label("Cancel").click();
    settle(&mut h);
    h.get_by_label("New Collection…").click();
    settle(&mut h);
    h.state_mut().open_sync_dialog(true);
    assert!(h.state().sync_dialog.is_none());
}

#[test]
fn pending_sync_disables_controls_and_blocks_escape_and_other_commands() {
    let fixture = Fixture::new();
    let url = server(fixture.path());
    let client = fixture.dir.path().join("pending.sqlite");
    let mut h = fixture.harness([800.0, 600.0]);
    connect(&mut h, &url, client.clone());
    h.get_by_label("Close").click();
    settle(&mut h);
    let blocker = Context::open(&client).unwrap();
    blocker
        .catalog()
        .connection()
        .execute_batch("BEGIN IMMEDIATE")
        .unwrap();
    menu(&mut h, "Sync…");
    h.get_by_label("Security settings").click();
    settle(&mut h);
    h.get_by_label("Sync now").click();
    h.run_steps(3);
    assert!(h
        .get_by_label("Choose token file…")
        .accesskit_node()
        .is_disabled());
    assert!(h
        .get_by_label("Choose CA file…")
        .accesskit_node()
        .is_disabled());
    assert!(h.state().worker.is_some());
    assert!(h.state().sync_dialog.as_ref().unwrap().pending);
    assert!(h
        .get_by_label("Close dialog")
        .accesskit_node()
        .is_disabled());
    h.get_by_label("Close dialog").click();
    h.run_steps(2);
    assert!(h.state().sync_dialog.as_ref().unwrap().pending);
    assert!(h.get_by_label("Sync now").accesskit_node().is_disabled());
    assert!(h
        .get_by_label("Server address")
        .accesskit_node()
        .is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().sync_dialog.is_some());
    h.state_mut().open_sync_dialog(true);
    assert!(!h.state().sync_dialog.as_ref().unwrap().clone);
    blocker
        .catalog()
        .connection()
        .execute_batch("ROLLBACK")
        .unwrap();
    settle(&mut h);
    assert!(h.state().status.contains("already up to date"));
}

#[test]
fn concurrent_collection_ids_restore_the_selected_collection_by_uid() {
    let fixture = Fixture::new();
    let url = server(fixture.path());
    let mut h = fixture.harness([1100.0, 720.0]);
    connect(&mut h, &url, fixture.dir.path().join("ids.sqlite"));
    h.get_by_label("Close").click();
    settle(&mut h);
    let mut p = Params::new();
    p.set("action", "create").set("name", "Local");
    h.state_mut().run_command("collection", &p);
    settle(&mut h);
    let local_uid = h
        .state()
        .ctx
        .catalog()
        .collection_uid_by_id(1)
        .unwrap()
        .unwrap();
    assert_eq!(
        h.state().ctx.catalog().collection_uid_by_id(99).unwrap(),
        None
    );
    h.state_mut().folder_sel = FolderSelection::Collections(vec![1]);
    let mut canonical = Context::open(fixture.path()).unwrap();
    p.set("name", "Server");
    dispatch_guarded(&mut canonical, "collection", &p).unwrap();
    sync_now(&mut h);
    let id = h
        .state()
        .ctx
        .catalog()
        .collection_id_by_uid(&local_uid)
        .unwrap()
        .unwrap();
    assert_ne!(id, 1);
    assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![id]));
    assert_eq!(
        h.state()
            .ctx
            .catalog()
            .collection(id)
            .unwrap()
            .unwrap()
            .name,
        "Local"
    );
}

#[test]
fn settings_errors_are_visible_and_do_not_turn_a_completed_sync_into_failure() {
    let fixture = Fixture::new();
    let mut settings = fixture.path().as_os_str().to_os_string();
    settings.push(".sync-url");
    std::fs::write(PathBuf::from(settings), b"invalid settings").unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    assert!(h.state().status.contains("Cannot read saved sync server"));
    let url = server(fixture.path());
    let client = fixture.dir.path().join("settings.sqlite");
    connect(&mut h, &url, client.clone());
    h.get_by_label("Close").click();
    settle(&mut h);
    let mut settings = client.as_os_str().to_os_string();
    settings.push(".sync-url");
    let settings = PathBuf::from(settings);
    std::fs::remove_file(&settings).unwrap();
    std::fs::create_dir(&settings).unwrap();
    sync_now(&mut h);
    let d = h.state().sync_dialog.as_ref().unwrap();
    assert!(d.error.is_none());
    let result = d.result.as_ref().unwrap();
    assert!(
        result.contains("already up to date") && result.contains("Cannot remember server address")
    );
    assert!(h.state().ctx.catalog().is_replica().unwrap());
    assert_eq!(h.state().sync_url, url);
}

#[test]
fn security_settings_fit_and_clear_selected_files_without_persisting_them() {
    let fixture = Fixture::new();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
            h.state_mut().open_sync_dialog(false);
            settle(&mut h);
            h.get_by_label("Security settings").click();
            settle(&mut h);
            let d = h.state_mut().sync_dialog.as_mut().unwrap();
            // Emulate native picker results, including long Unicode paths.
            d.token_file = Some("/synthetic/security/a long folder/旅行/token.txt".into());
            d.ca_file = Some("/synthetic/security/a long folder/旅行/ca.pem".into());
            d.error = Some("A long authentication error. ".repeat(100));
            settle(&mut h);
            assert!(h.get_by_label("Sync now").rect().bottom() < 600.0);
            h.get_by_label("Clear token file").click();
            settle(&mut h);
            assert!(h.state().sync_dialog.as_ref().unwrap().token_file.is_none());
            assert!(h.state().sync_dialog.as_ref().unwrap().ca_file.is_some());
            h.get_by_label("Clear CA file").click();
            settle(&mut h);
            assert!(h.state().sync_dialog.as_ref().unwrap().ca_file.is_none());
            h.state_mut().sync_dialog.as_mut().unwrap().token_file = Some("token.txt".into());
            settle(&mut h);
            h.get_by_label("Cancel").click();
            settle(&mut h);
            h.state_mut().open_sync_dialog(false);
            settle(&mut h);
            let d = h.state().sync_dialog.as_ref().unwrap();
            assert!(d.token_file.is_none() && d.ca_file.is_none());
            assert!(sync_ui::load_url(&fixture.path()).unwrap().is_empty());
        }
    }
}

#[cfg(unix)]
#[test]
fn security_paths_reject_non_utf8_and_oversized_values_before_releasing_catalogue() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let fixture = Fixture::new();
    let original = std::fs::read(fixture.path()).unwrap();
    for token in [true, false] {
        for path in [
            PathBuf::from(OsString::from_vec(b"/synthetic/bad-\xff".to_vec())),
            PathBuf::from("a".repeat(8193)),
        ] {
            let mut h = fixture.harness([800.0, 600.0]);
            menu(&mut h, "Sync…");
            let d = h.state_mut().sync_dialog.as_mut().unwrap();
            d.url = "http://127.0.0.1:1".into();
            d.acknowledge = true;
            if token {
                d.token_file = Some(path.clone());
            } else {
                d.ca_file = Some(path.clone());
            }
            settle(&mut h);
            h.get_by_label("Sync now").click();
            settle(&mut h);
            let d = h.state().sync_dialog.as_ref().unwrap();
            assert!(d.error.as_ref().unwrap().contains("path"));
            assert_eq!(
                if token {
                    d.token_file.as_ref()
                } else {
                    d.ca_file.as_ref()
                },
                Some(&path)
            );
            assert!(h.state().worker.is_none());
            assert!(h.state().ctx.catalog().catalogue().unwrap().is_some());
            assert_eq!(std::fs::read(fixture.path()).unwrap(), original);
        }
    }
}

#[test]
fn security_files_reach_clone_and_sync_workers_without_saving_credentials() {
    let fixture = Fixture::new();
    let token = "synthetic-ui-token-0123456789abcdef0123456789abcdef";
    let token_path = fixture.dir.path().join("token.txt");
    let wrong_path = fixture.dir.path().join("wrong-token.txt");
    let ca_path = fixture.dir.path().join("invalid-ca.pem");
    std::fs::write(&token_path, token).unwrap();
    std::fs::write(
        &wrong_path,
        "wrong-synthetic-token-0123456789abcdef0123456789",
    )
    .unwrap();
    std::fs::write(&ca_path, b"not a certificate").unwrap();
    let mut params = Params::new();
    params.set("token-file", token_path.to_str().unwrap());
    let mut security =
        icatalog_sync::security::ServerSecurity::from_params(&params, "127.0.0.1").unwrap();
    let server = icatalog_sync::server::bind_with_security("127.0.0.1:0", &mut security).unwrap();
    let url = format!(
        "http://127.0.0.1:{}",
        server.server_addr().to_ip().unwrap().port()
    );
    let server_path = fixture.path();
    std::thread::spawn(move || {
        let mut ctx = Context::open(server_path).unwrap();
        let _ = icatalog_sync::server::serve_loop_with_security(server, &mut ctx, security);
    });
    let client_path = fixture.dir.path().join("authenticated.sqlite");
    let original = std::fs::read(fixture.path()).unwrap();
    let mut h = fixture.harness([800.0, 600.0]);
    menu(&mut h, "Connect to Server…");
    let d = h.state_mut().sync_dialog.as_mut().unwrap();
    d.url = url.clone();
    d.destination = Some(client_path.clone());
    d.token_file = Some(wrong_path.clone());
    settle(&mut h);
    h.get_by_label("Clone").click();
    settle(&mut h);
    assert!(!client_path.exists());
    assert_eq!(h.state().path, fixture.path());
    let d = h.state().sync_dialog.as_ref().unwrap();
    assert!(d.error.as_ref().unwrap().contains("401"));
    assert_eq!(d.token_file.as_ref(), Some(&wrong_path));
    assert_eq!(std::fs::read(fixture.path()).unwrap(), original);

    // A selected CA path must reach the backend rather than being ignored.
    let d = h.state_mut().sync_dialog.as_mut().unwrap();
    d.url = "https://127.0.0.1:1".into();
    d.token_file = Some(token_path.clone());
    d.ca_file = Some(ca_path.clone());
    settle(&mut h);
    h.get_by_label("Clone").click();
    settle(&mut h);
    assert!(!client_path.exists());
    let d = h.state().sync_dialog.as_ref().unwrap();
    assert!(d.error.as_ref().unwrap().contains("CA"));
    assert_eq!(d.ca_file.as_ref(), Some(&ca_path));

    let d = h.state_mut().sync_dialog.as_mut().unwrap();
    d.url = url.clone();
    d.ca_file = None;
    settle(&mut h);
    h.get_by_label("Clone").click();
    settle(&mut h);
    assert_eq!(h.state().path, client_path);
    assert!(h.state().replica);
    let mut settings = client_path.as_os_str().to_os_string();
    settings.push(".sync-url");
    assert_eq!(
        std::fs::read_to_string(PathBuf::from(settings)).unwrap(),
        url
    );
    h.get_by_label("Close").click();
    settle(&mut h);
    h.state_mut()
        .run_command("edit", &edit_params(1, "rating", "4"));
    settle(&mut h);
    menu(&mut h, "Sync…");
    let d = h.state_mut().sync_dialog.as_mut().unwrap();
    assert!(d.token_file.is_none() && d.ca_file.is_none());
    d.token_file = Some(wrong_path.clone());
    settle(&mut h);
    h.get_by_label("Sync now").click();
    settle(&mut h);
    assert!(h
        .state()
        .sync_dialog
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("401"));
    assert_eq!(
        h.state()
            .ctx
            .catalog()
            .annotation(1)
            .unwrap()
            .unwrap()
            .rating,
        4
    );
    h.state_mut().sync_dialog.as_mut().unwrap().token_file = Some(token_path);
    settle(&mut h);
    h.get_by_label("Sync now").click();
    settle(&mut h);
    assert!(
        h.state().status.contains("push: accepted"),
        "{}",
        h.state().status
    );
    assert!(!h.state().status.contains(token));
    let canonical = Context::open(fixture.path()).unwrap();
    assert_eq!(
        canonical.catalog().annotation(1).unwrap().unwrap().rating,
        4
    );
}
