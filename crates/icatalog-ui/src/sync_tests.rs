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
        .type_text("https://unsupported");
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
        .contains("https"));
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
    h.get_by_label("Sync now").click();
    h.run_steps(3);
    assert!(h.state().worker.is_some());
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
