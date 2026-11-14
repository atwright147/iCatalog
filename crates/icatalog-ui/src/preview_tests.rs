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
}
