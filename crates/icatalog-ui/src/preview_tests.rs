use super::*;

fn seed(fixture: &Fixture, external: bool) -> String {
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let jpeg =
        icatalog_media::thumbnail_jpeg(&support::bmp_with_size([80, 150, 220], 1280, 720), 1920)
            .unwrap();
    let hash = icatalog_engine::previews::hash(&jpeg.bytes);
    ctx.catalog()
        .store_preview_blob(&hash, Some(&jpeg.bytes))
        .unwrap();
    ctx.catalog()
        .store_preview(&icatalog_catalog::Preview {
            asset_id: 1,
            hash: hash.clone(),
            width: jpeg.width,
            height: jpeg.height,
            source_hash: "synthetic".into(),
        })
        .unwrap();
    if external {
        ctx.catalog().set_preview_mode("external").unwrap();
    }
    icatalog_engine::previews::cache(ctx.catalog(), &fixture.path(), &hash, &jpeg.bytes).unwrap();
    hash
}

fn open_from_menu(h: &mut Harness<'_, App>) {
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click_secondary();
    settle(h);
    h.get_by_label("View preview").click();
    settle(h);
    assert!(h.state().preview.is_open());
}

#[test]
fn stored_preview_opens_offline_in_both_views_and_themes_at_native_scales() {
    for external in [false, true] {
        let fixture = Fixture::new();
        seed(&fixture, external);
        let before = std::fs::read(fixture.path()).unwrap();
        for mode in ["Grid", "List"] {
            for theme in [egui::Theme::Dark, egui::Theme::Light] {
                for scale in [1.0, 1.5, 2.0] {
                    let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
                    h.get_by_label(mode).click();
                    settle(&mut h);
                    open_from_menu(&mut h);
                    assert!(h.query_by_label("Stored preview").is_some());
                    assert!(h.query_by_label("1280 × 720 pixels").is_some());
                    let close = h.get_by_label("Close").rect();
                    assert!(close.bottom() <= 600.0, "close: {close:?}");
                    let image = h.get_by_label("Stored preview").rect();
                    assert!(image.width() <= 736.0);
                    assert!(image.height() <= 410.0);
                    h.get_by_label("100%").click();
                    settle(&mut h);
                    let actual = h.get_by_label("Stored preview").rect();
                    assert!((actual.width() - 1280.0 / scale).abs() < 1.0);
                    h.get_by_label("Fit").click();
                    settle(&mut h);
                    h.key_press(egui::Key::Escape);
                    settle(&mut h);
                    assert!(!h.state().preview.is_open());
                    assert!(h.query_by_label("Stored preview").is_none());
                }
            }
        }
        assert_eq!(std::fs::read(fixture.path()).unwrap(), before);
    }
}

#[test]
fn preview_double_click_read_only_and_modal_guards() {
    let fixture = Fixture::new();
    seed(&fixture, false);
    let mut h = double_click_harness(&fixture);
    h.state_mut().sync_read_only = true;
    h.run_steps(40);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    h.run_steps(2);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    assert!(h.state().preview.is_open());
    assert!(h.query_by_label("Stored preview").is_some());
    h.state_mut().open_sync_dialog(false);
    assert!(h.state().sync_dialog.is_none());
    h.get_by_label("Close dialog").click();
    settle(&mut h);
    assert!(!h.state().preview.is_open());
    h.state_mut().sync_read_only = false;
    h.state_mut().create_collection = Some(CreateCollectionDialog::default());
    h.state_mut().open_preview(1);
    assert!(!h.state().preview.is_open());
    h.state_mut().create_collection = None;
    let (release, receiver) = std::sync::mpsc::channel();
    h.state_mut().worker = Some(worker::Worker::blocked_for_test(receiver));
    h.state_mut().open_preview(1);
    assert!(!h.state().preview.is_open());
    release.send(()).unwrap();
    settle(&mut h);
}

#[test]
fn dismissed_preview_load_blocks_sync_and_connect_until_its_reader_finishes() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([800.0, 600.0]);
    settle(&mut h);
    let (release, receiver) = std::sync::mpsc::channel();
    h.state_mut().preview = preview::Viewer::blocked_for_test(receiver);
    h.run_steps(3);
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(!h.state().preview.is_open());
    assert!(h.state().preview.is_loading());
    for clone in [false, true] {
        h.state_mut().open_sync_dialog(clone);
        assert!(h.state().sync_dialog.is_none());
    }
    release.send(()).unwrap();
    settle(&mut h);
    for clone in [false, true] {
        h.state_mut().open_sync_dialog(clone);
        assert!(h.state().sync_dialog.is_some());
        h.state_mut().sync_dialog = None;
    }
}

#[test]
fn unavailable_and_corrupt_previews_show_errors_without_reading_originals() {
    for kind in ["absent", "uncached", "corrupt"] {
        let fixture = Fixture::new();
        if kind != "absent" {
            let hash = seed(&fixture, true);
            let path = fixture
                .dir
                .path()
                .join("previews")
                .join(format!("{hash}.jpg"));
            if kind == "corrupt" {
                std::fs::write(path, b"corrupt").unwrap();
            } else {
                std::fs::remove_file(path).unwrap();
            }
        }
        let before = std::fs::read(fixture.path()).unwrap();
        let mut h = fixture.harness([800.0, 600.0]);
        open_from_menu(&mut h);
        assert!(h.query_by_label("Preview unavailable").is_some());
        assert!(h.query_by_label("Stored preview").is_none());
        h.get_by_label("Close").click();
        settle(&mut h);
        assert!(!h.state().preview.is_open());
        assert_eq!(std::fs::read(fixture.path()).unwrap(), before);
    }
}

pub(super) fn visual_snapshots(results: &mut egui_kittest::SnapshotResults) {
    let fixture = Fixture::new();
    seed(&fixture, false);
    for (theme, name) in [
        (egui::Theme::Dark, "catalogue-preview"),
        (egui::Theme::Light, "catalogue-preview-light"),
    ] {
        let mut h = fixture.harness_with_display([800.0, 600.0], 1.0, theme);
        open_from_menu(&mut h);
        h.event(egui::Event::PointerGone);
        h.run();
        h.snapshot(name);
        results.extend(h.take_snapshot_results());
    }
    let absent = Fixture::new();
    let mut h = absent.harness([800.0, 600.0]);
    open_from_menu(&mut h);
    h.event(egui::Event::PointerGone);
    h.run();
    h.snapshot("catalogue-preview-unavailable");
    results.extend(h.take_snapshot_results());

    let remote = Fixture::new();
    make_uncached_replica(&remote);
    let mut h = remote.harness([800.0, 600.0]);
    h.state_mut().sync_url = "https://nas.example:8443".into();
    open_from_menu(&mut h);
    h.get_by_label("Security settings").click();
    settle(&mut h);
    h.event(egui::Event::PointerGone);
    h.run();
    h.snapshot("catalogue-preview-fetch");
    results.extend(h.take_snapshot_results());
}

fn make_uncached_replica(fixture: &Fixture) -> (String, Vec<u8>) {
    let hash = seed(fixture, true);
    let cache = fixture
        .dir
        .path()
        .join("previews")
        .join(format!("{hash}.jpg"));
    let bytes = std::fs::read(&cache).unwrap();
    std::fs::remove_file(cache).unwrap();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog()
        .set_replica_base(0, b"synthetic baseline")
        .unwrap();
    (hash, bytes)
}

/// A bounded synthetic HTTP responder records every request, so an accidental
/// catalogue sync or upload fails the same tests as a malformed preview fetch.
fn preview_server(
    responses: Vec<(u16, Vec<u8>)>,
) -> (
    String,
    std::sync::mpsc::Receiver<String>,
    std::thread::JoinHandle<()>,
) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        for (status, body) in responses {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "preview request timeout");
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("preview accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let len = stream.read(&mut buffer).unwrap();
                assert!(len > 0 && request.len() < 16 * 1024);
                request.extend_from_slice(&buffer[..len]);
            }
            tx.send(String::from_utf8(request).unwrap()).unwrap();
            write!(
                stream,
                "HTTP/1.1 {status} Synthetic\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    (url, rx, handle)
}

#[test]
fn explicit_preview_fetch_is_get_only_persists_cache_and_reopens_offline() {
    let fixture = Fixture::new();
    let (hash, bytes) = make_uncached_replica(&fixture);
    let before = std::fs::read(fixture.path()).unwrap();
    let (url, requests, server) = preview_server(vec![(200, bytes.clone())]);
    let mut h = fixture.harness([800.0, 600.0]);
    h.state_mut().sync_url = url;
    open_from_menu(&mut h);
    assert!(
        requests.try_recv().is_err(),
        "opening must never fetch automatically"
    );
    h.get_by_label("Fetch preview").click();
    settle(&mut h);
    assert!(h.query_by_label("Stored preview").is_some());
    let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(request.starts_with(&format!("GET /preview/{hash} HTTP/1.1\r\n")));
    server.join().unwrap();
    assert_eq!(
        std::fs::read(
            fixture
                .dir
                .path()
                .join("previews")
                .join(format!("{hash}.jpg"))
        )
        .unwrap(),
        bytes
    );
    assert_eq!(std::fs::read(fixture.path()).unwrap(), before);
    assert!(!fixture.dir.path().join("fixture.sqlite.sync.toml").exists());
    h.get_by_label("Close").click();
    settle(&mut h);
    open_from_menu(&mut h);
    assert!(h.query_by_label("Stored preview").is_some());
}

#[test]
fn preview_fetch_failure_retains_dialog_and_ephemeral_security_for_retry() {
    let fixture = Fixture::new();
    let (hash, bytes) = make_uncached_replica(&fixture);
    let before = std::fs::read(fixture.path()).unwrap();
    let (url, requests, server) = preview_server(vec![(401, b"denied".to_vec()), (200, bytes)]);
    let token_path = fixture.dir.path().join("synthetic-token.txt");
    let token = "synthetic-preview-token-0123456789abcdef0123456789";
    std::fs::write(&token_path, token).unwrap();
    let mut h = fixture.harness([800.0, 600.0]);
    h.state_mut().sync_url = url;
    open_from_menu(&mut h);
    h.state_mut()
        .preview
        .set_fetch_files_for_test(Some(token_path), None);
    h.get_by_label("Fetch preview").click();
    settle(&mut h);
    assert!(h.query_by_label("Preview unavailable").is_some());
    assert!(!fixture
        .dir
        .path()
        .join("previews")
        .join(format!("{hash}.jpg"))
        .exists());
    assert_eq!(std::fs::read(fixture.path()).unwrap(), before);
    h.get_by_label("Fetch preview").click();
    settle(&mut h);
    assert!(h.query_by_label("Stored preview").is_some());
    for _ in 0..2 {
        let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(request.starts_with(&format!("GET /preview/{hash} HTTP/1.1\r\n")));
        assert!(request
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {token}")));
    }
    server.join().unwrap();
    assert!(!fixture.dir.path().join("fixture.sqlite.sync.toml").exists());
    assert!(h.state().sync_preferences.token_file.is_none());
}

#[test]
fn preview_fetch_is_disabled_in_read_only_and_recovery_states() {
    let fixture = Fixture::new();
    make_uncached_replica(&fixture);
    for state in ["required", "backup", "recovery", "check-error", "no-server"] {
        let mut h = fixture.harness([800.0, 600.0]);
        h.state_mut().sync_url = "http://127.0.0.1:1".into();
        match state {
            "required" => h.state_mut().sync_read_only = true,
            "backup" => h.state_mut().backup_return_path = Some(fixture.path()),
            "recovery" => {
                h.state_mut().recovery_pending = true;
                h.state_mut().recovery_phase = Some("snapshot_ready".into());
            }
            "check-error" => h.state_mut().sync_check_error = Some("synthetic".into()),
            _ => h.state_mut().sync_url.clear(),
        }
        open_from_menu(&mut h);
        assert!(
            h.get_by_label("Fetch preview")
                .accesskit_node()
                .is_disabled(),
            "{state}"
        );
        h.get_by_label("Fetch preview").click();
        h.run_steps(3);
        assert!(!h.state().preview.is_loading());
        assert!(h.query_by_label("Stored preview").is_none());
    }
}

#[test]
fn pending_preview_fetch_blocks_dismissal_quit_and_other_commands() {
    let fixture = Fixture::new();
    make_uncached_replica(&fixture);
    let mut h = fixture.harness([800.0, 600.0]);
    h.state_mut().sync_url = "http://127.0.0.1:1".into();
    open_from_menu(&mut h);
    let (release, receiver) = std::sync::mpsc::channel();
    h.state_mut().preview.block_fetch_for_test(receiver);
    h.run_steps(3);
    assert!(h.get_by_label("Close").accesskit_node().is_disabled());
    assert!(h
        .get_by_label("Close dialog")
        .accesskit_node()
        .is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().preview.is_open());
    assert!(!h.state_mut().close_allowed());
    h.state_mut()
        .run_command("edit", &edit_params(1, "rating", "5"));
    assert!(h.state().worker.is_none());
    h.state_mut().open_sync_dialog(false);
    assert!(h.state().sync_dialog.is_none());
    release.send(()).unwrap();
    settle(&mut h);
    assert!(!h.state().preview.is_fetching());
    h.get_by_label("Close").click();
    settle(&mut h);
    assert!(h.state_mut().close_allowed());
}
