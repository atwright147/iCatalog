//! Interaction and image tests drive the real eframe App through AccessKit.
#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;
#[path = "../tests/support/mod.rs"]
mod support;
use egui_kittest::{
    kittest::{By, NodeT, Queryable},
    Harness, SnapshotOptions,
};
use std::time::{Duration, Instant};
use support::bmp;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let ctx = open_catalogue(&dir.path().join("fixture.sqlite")).unwrap();
        ctx.catalog()
            .connection()
            .execute_batch(
                "INSERT INTO folder(id,name) VALUES (1,'Photos'),(2,'Other');
             INSERT INTO asset(id,uid,path,name,folder_id,width,height,format) VALUES
             (1,'red','/synthetic/Photos/Red.bmp','Red.bmp',1,64,32,'BMP'),
             (2,'blue','/synthetic/Photos/Blue.bmp','Blue.bmp',1,64,32,'BMP'),
             (3,'green','/synthetic/Other/Green.bmp','Green.bmp',2,64,32,'BMP');",
            )
            .unwrap();
        for (id, color) in [(1, [40, 50, 220]), (2, [220, 70, 40]), (3, [40, 170, 50])] {
            let thumbnail = icatalog_media::thumbnail_jpeg(&bmp(color), 256).unwrap();
            ctx.catalog()
                .insert_thumbnail(&icatalog_catalog::Thumbnail {
                    asset_id: id,
                    size_class: 256,
                    source: "synthetic".into(),
                    format: "jpeg".into(),
                    width: thumbnail.width,
                    height: thumbnail.height,
                    orientation_applied: false,
                    bytes: thumbnail.bytes,
                    derived_from: None,
                    generator_version: 1,
                })
                .unwrap();
        }
        Self { dir }
    }
    fn path(&self) -> PathBuf {
        self.dir.path().join("fixture.sqlite")
    }
    fn harness(&self, size: [f32; 2]) -> Harness<'static, App> {
        let path = self.path();
        Harness::builder()
            .with_size(size)
            .with_pixels_per_point(1.0)
            .with_theme(egui::Theme::Dark)
            .with_options(
                SnapshotOptions::default()
                    .output_path(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")),
            )
            .build_eframe(move |_| {
                let (_, receiver) = std::sync::mpsc::channel();
                App::with_menu(open_catalogue(&path).unwrap(), path, None, receiver)
            })
    }
}

fn settle(h: &mut Harness<'_, App>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    // Process queued pointer/key events before checking the worker.
    h.run_steps(3);
    while h.state().worker.is_some() {
        assert!(
            Instant::now() < deadline,
            "worker timeout: {}",
            h.state().status
        );
        std::thread::sleep(Duration::from_millis(2));
        h.step();
    }
    h.run();
}

fn drag_to(h: &mut Harness<'_, App>, name: &str, target: egui::Pos2) {
    let source = h
        .get(By::new().label(name).role(egui::accesskit::Role::Button))
        .rect()
        .center();
    h.hover_at(source);
    h.run_steps(1);
    h.drag_at(source);
    h.run_steps(1);
    h.hover_at(source + egui::vec2(12.0, 0.0));
    h.run_steps(2);
    h.hover_at(target);
    h.run_steps(2);
}

fn release_at(h: &mut Harness<'_, App>, target: egui::Pos2) {
    h.event(egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    settle(h);
}

fn open_asset_menu(h: &mut Harness<'_, App>, name: &str) {
    h.get(By::new().label(name).role(egui::accesskit::Role::Button))
        .click_secondary();
    h.run_steps(3);
}

#[test]
fn interaction_drag_assets_to_collections_in_grid_and_list() {
    for mode in ["Grid", "List"] {
        let fixture = Fixture::new();
        let ctx = open_catalogue(&fixture.path()).unwrap();
        ctx.catalog().insert_collection("Favourites").unwrap();
        let mut h = fixture.harness([1100.0, 720.0]);
        h.get_by_label(mode).click();
        settle(&mut h);
        h.get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
        settle(&mut h);
        for iteration in 0..2 {
            let target = h.get_by_label("Favourites").rect().center();
            drag_to(&mut h, "Blue.bmp", target);
            assert_eq!(h.state().selected, Some(2));
            assert!(egui::DragAndDrop::has_payload_of_type::<DraggedAsset>(
                &h.ctx
            ));
            assert_eq!(
                ctx.catalog().collection_assets(1).unwrap(),
                if iteration == 0 { vec![] } else { vec![2] },
                "dragging alone must not write"
            );
            release_at(&mut h, target);
            assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![2]);
        }
        // A drop elsewhere and an escaped drag must not add another member.
        let outside = h.get_by_label("All").rect().center();
        drag_to(&mut h, "Green.bmp", outside);
        release_at(&mut h, outside);
        let target = h.get_by_label("Favourites").rect().center();
        drag_to(&mut h, "Green.bmp", target);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        release_at(&mut h, target);
        let reopened = open_catalogue(&fixture.path()).unwrap();
        assert_eq!(reopened.catalog().collection_assets(1).unwrap(), vec![2]);
        assert_eq!(
            reopened.catalog().asset(2).unwrap().unwrap().folder_id,
            Some(1)
        );
    }
}

#[test]
fn interaction_collection_rows_fill_width_and_accept_clicks_and_drops_at_right_edge() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog().insert_collection("Favourites").unwrap();
    let long_name = "A very long collection name that should truncate within the sidebar";
    ctx.catalog().insert_collection(long_name).unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    let name = h.get_by_label("Favourites").rect();
    let long = h.get_by_label(long_name).rect();
    assert!(h
        .query_by_label("Add selected asset to Favourites")
        .is_none());
    assert!(h
        .query_by_label("Remove selected asset from Favourites")
        .is_none());
    assert!(name.width() > h.get_by_label("New Collection…").rect().width());
    assert!((name.width() - long.width()).abs() < 1.0);
    assert!((name.height() - long.height()).abs() < 1.0);
    assert!((name.right() - long.right()).abs() < 1.0);
    let target = egui::pos2(name.right() - 2.0, name.center().y);
    drag_to(&mut h, "Blue.bmp", target);
    release_at(&mut h, target);
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![2]);
    let name = h.get_by_label("Favourites").rect();
    let target = egui::pos2(name.right() - 2.0, name.center().y);
    h.hover_at(target);
    h.drag_at(target);
    h.event(egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    settle(&mut h);
    assert_eq!(h.state().folder_sel, FolderSelection::Collection(1));
    assert_eq!(
        h.state()
            .assets
            .iter()
            .map(|asset| asset.id)
            .collect::<Vec<_>>(),
        vec![2]
    );
}

#[test]
fn interaction_selected_collection_toggles_off_and_preserves_search() {
    for mode in ["Grid", "List"] {
        let fixture = Fixture::new();
        let ctx = open_catalogue(&fixture.path()).unwrap();
        let favourites = ctx.catalog().insert_collection("Favourites").unwrap();
        let album = ctx.catalog().insert_collection("Album").unwrap();
        ctx.catalog().add_to_collection(favourites, 1).unwrap();
        ctx.catalog().add_to_collection(album, 2).unwrap();
        let mut h = fixture.harness([1100.0, 720.0]);
        h.get_by_label(mode).click();
        settle(&mut h);
        h.get_by_label("Photos").click();
        settle(&mut h);
        h.get_by_label("Favourites").click();
        settle(&mut h);
        assert_eq!(h.state().assets.len(), 1);
        h.get_by_label("Favourites").click();
        settle(&mut h);
        assert_eq!(h.state().folder_sel, FolderSelection::All);
        assert_eq!(h.state().assets.len(), 3);
        h.get_by_label("Album").click();
        settle(&mut h);
        h.get_by_label("Favourites").click();
        settle(&mut h);
        assert_eq!(
            h.state().folder_sel,
            FolderSelection::Collection(favourites)
        );
        assert_eq!(h.state().assets[0].id, 1);
        h.get_by_label("Search name or path").click();
        h.get_by_label("Search name or path").type_text("Blue");
        h.key_press(egui::Key::Enter);
        settle(&mut h);
        assert!(h.state().assets.is_empty());
        h.get_by_label("Favourites").click();
        settle(&mut h);
        assert_eq!(h.state().folder_sel, FolderSelection::All);
        assert_eq!(h.state().search, "Blue");
        assert_eq!(
            h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(
            ctx.catalog().collection_assets(favourites).unwrap(),
            vec![1]
        );
        assert_eq!(ctx.catalog().collection_assets(album).unwrap(), vec![2]);
    }
}

#[test]
fn interaction_context_menu_targets_clicked_asset_and_removes_from_active_collection() {
    for mode in ["Grid", "List"] {
        let fixture = Fixture::new();
        let ctx = open_catalogue(&fixture.path()).unwrap();
        ctx.catalog().insert_collection("Favourites").unwrap();
        ctx.catalog().insert_collection("Album").unwrap();
        let mut h = fixture.harness([1100.0, 720.0]);
        h.get_by_label(mode).click();
        settle(&mut h);
        h.get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
        settle(&mut h);
        h.get(
            By::new()
                .label("Blue.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_secondary();
        settle(&mut h);
        assert_eq!(h.state().selected, Some(2));
        h.get_by_label("Add to Favourites").click();
        settle(&mut h);
        assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![2]);
        h.get(
            By::new()
                .label("Blue.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_secondary();
        settle(&mut h);
        h.get_by_label("Add to Album").click();
        settle(&mut h);
        h.get_by_label("Favourites").click();
        settle(&mut h);
        h.get(
            By::new()
                .label("Blue.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_secondary();
        settle(&mut h);
        h.get_by_label("Remove from Favourites").click();
        settle(&mut h);
        assert!(h.state().assets.is_empty());
        assert!(h.state().selected.is_none());
        assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
        assert_eq!(ctx.catalog().collection_assets(2).unwrap(), vec![2]);
        assert_eq!(ctx.catalog().stats().unwrap().assets, 3);
    }
}

#[test]
fn interaction_collection_drag_and_context_menu_block_busy_writes_and_report_failure() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog().insert_collection("Album").unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    ctx.catalog()
        .connection()
        .execute_batch("BEGIN EXCLUSIVE")
        .unwrap();
    open_asset_menu(&mut h, "Red.bmp");
    h.get_by_label("Add to Album").click();
    h.run_steps(3);
    let target = h.get_by_label("Album").rect().center();
    drag_to(&mut h, "Blue.bmp", target);
    assert!(!egui::DragAndDrop::has_payload_of_type::<DraggedAsset>(
        &h.ctx
    ));
    // Release without waiting for the deliberately blocked worker.
    h.event(egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
    h.get(
        By::new()
            .label("Blue.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click_secondary();
    h.run_steps(3);
    assert!(h
        .get_by_label("Add to Album")
        .accesskit_node()
        .is_disabled());
    h.get_by_label("Add to Album").click();
    h.run_steps(2);
    ctx.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1]);
    ctx.catalog()
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_drop BEFORE INSERT ON collection_asset
         BEGIN SELECT RAISE(ABORT, 'synthetic drop failure'); END;",
        )
        .unwrap();
    let target = h.get_by_label("Album").rect().center();
    drag_to(&mut h, "Blue.bmp", target);
    release_at(&mut h, target);
    assert!(h.state().status.contains("synthetic drop failure"));
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1]);
}

#[test]
fn interaction_edit_search_and_reopen() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("Photos").click();
    settle(&mut h);
    assert_eq!(h.state().assets.len(), 2);
    h.get_by_label("Search name or path").click();
    h.get_by_label("Search name or path").type_text("Red");
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(h.state().assets.len(), 1);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    assert_eq!(h.state().selected, Some(1));
    h.get_by_label("Rating 5").click();
    settle(&mut h);
    h.get_by_label("Label 3").click();
    settle(&mut h);
    h.get_by_label("Keyword").click();
    h.get_by_label("Keyword").type_text("Favourite");
    h.run_steps(2);
    h.get_by_label("Add").click();
    settle(&mut h);
    let reopened = open_catalogue(&fixture.path()).unwrap();
    let annotation = reopened.catalog().annotation(1).unwrap().unwrap();
    assert_eq!(annotation.rating, 5);
    assert_eq!(annotation.label, 3);
    assert_eq!(
        reopened.catalog().asset_keywords(1).unwrap()[0].name,
        "Favourite"
    );
    h.get_by_label("x").click();
    settle(&mut h);
    assert_eq!(h.state().assets.len(), 2);
    h.get_by_label("List").click();
    settle(&mut h);
    assert_eq!(h.state().view_mode, ViewMode::List);
}

#[test]
fn interaction_collections_create_add_search_remove_and_reopen() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("New Collection…").click();
    settle(&mut h);
    assert!(h.get_by_label("Create").accesskit_node().is_disabled());
    h.get_by_label("Name").type_text("Favourites");
    h.run_steps(2);
    h.get_by_label("Create").click();
    settle(&mut h);
    assert!(h.state().create_collection.is_none());
    assert_eq!(h.state().collections.len(), 1);
    h.get_by_label("Favourites").click();
    settle(&mut h);
    assert!(h.state().assets.is_empty());
    assert!(h
        .query_by_label("Add selected asset to Favourites")
        .is_none());
    h.get_by_label("All").click();
    settle(&mut h);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    open_asset_menu(&mut h, "Red.bmp");
    h.get_by_label("Add to Favourites").click();
    settle(&mut h);
    assert_eq!(h.state().selected, Some(1));
    h.get_by_label("Favourites").click();
    settle(&mut h);
    assert_eq!(
        h.state()
            .assets
            .iter()
            .map(|asset| asset.id)
            .collect::<Vec<_>>(),
        vec![1]
    );
    h.get_by_label("Search name or path").click();
    h.get_by_label("Search name or path").type_text("Blue");
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert!(h.state().assets.is_empty());
    h.get_by_label("x").click();
    settle(&mut h);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    let reopened = open_catalogue(&fixture.path()).unwrap();
    assert_eq!(reopened.catalog().collection_assets(1).unwrap(), vec![1]);
    assert_eq!(
        reopened.catalog().asset(1).unwrap().unwrap().folder_id,
        Some(1)
    );
    open_asset_menu(&mut h, "Red.bmp");
    h.get_by_label("Remove from Favourites").click();
    settle(&mut h);
    assert!(h.state().assets.is_empty());
    assert!(h.state().selected.is_none());
    assert!(reopened.catalog().collection_assets(1).unwrap().is_empty());
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("Favourites").click();
    settle(&mut h);
    assert!(h.state().assets.is_empty());
    assert_eq!(h.state().collections.len(), 1);
}

#[test]
fn interaction_collection_write_failure_preserves_membership_and_recovers() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog().insert_collection("Album").unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    ctx.catalog()
        .connection()
        .execute_batch("BEGIN EXCLUSIVE")
        .unwrap();
    open_asset_menu(&mut h, "Red.bmp");
    h.get_by_label("Add to Album").click();
    h.run_steps(3);
    assert!(h.state().worker.is_some());
    assert!(h
        .get_by_label("New Collection…")
        .accesskit_node()
        .is_disabled());
    assert!(h.query_by_label("Add selected asset to Album").is_none());
    ctx.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    ctx.catalog()
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_removal BEFORE DELETE ON collection_asset
         BEGIN SELECT RAISE(ABORT, 'synthetic membership failure'); END;",
        )
        .unwrap();
    open_asset_menu(&mut h, "Red.bmp");
    h.get_by_label("Remove from Album").click();
    settle(&mut h);
    assert!(h.state().status.contains("synthetic membership failure"));
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1]);
    open_asset_menu(&mut h, "Red.bmp");
    assert!(!h
        .get_by_label("Remove from Album")
        .accesskit_node()
        .is_disabled());
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    ctx.catalog()
        .connection()
        .execute_batch("DROP TRIGGER reject_removal")
        .unwrap();
    open_asset_menu(&mut h, "Red.bmp");
    h.get_by_label("Remove from Album").click();
    settle(&mut h);
    assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
}

#[test]
fn interaction_create_collection_modal_cancel_escape_and_backdrop_do_not_write() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    for dismissal in ["cancel", "escape", "backdrop"] {
        h.get_by_label("New Collection…").click();
        settle(&mut h);
        assert!(h
            .state()
            .create_collection
            .as_ref()
            .unwrap()
            .name
            .is_empty());
        // The input takes focus automatically when the modal opens.
        h.get_by_label("Name").type_text("Discard me");
        h.run_steps(2);
        assert!(h
            .get_by_label("New Collection…")
            .accesskit_node()
            .is_disabled());
        match dismissal {
            "cancel" => h.get_by_label("Cancel").click(),
            "escape" => h.key_press(egui::Key::Escape),
            _ => h
                .get(
                    By::new()
                        .label("Green.bmp")
                        .role(egui::accesskit::Role::Button),
                )
                .click(),
        }
        settle(&mut h);
        assert!(h.state().create_collection.is_none());
        assert!(h.state().worker.is_none());
        assert!(h.state().collections.is_empty());
        assert_eq!(
            h.state().selected,
            Some(1),
            "backdrop must block asset selection"
        );
    }
    assert!(open_catalogue(&fixture.path())
        .unwrap()
        .catalog()
        .all_collections()
        .unwrap()
        .is_empty());
}

#[test]
fn interaction_create_collection_modal_failure_retry_and_busy_controls() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog()
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_collection BEFORE INSERT ON collection
         BEGIN SELECT RAISE(ABORT, 'synthetic creation failure'); END;",
        )
        .unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("New Collection…").click();
    settle(&mut h);
    h.get_by_label("Name").type_text("Retry album");
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    let dialog = h.state().create_collection.as_ref().unwrap();
    assert_eq!(dialog.name, "Retry album");
    assert!(dialog
        .error
        .as_ref()
        .unwrap()
        .contains("synthetic creation failure"));
    assert!(!dialog.pending);
    assert!(ctx.catalog().all_collections().unwrap().is_empty());
    ctx.catalog()
        .connection()
        .execute_batch("DROP TRIGGER reject_collection; BEGIN EXCLUSIVE")
        .unwrap();
    h.get_by_label("Create").click();
    h.run_steps(3);
    assert!(h.state().create_collection.as_ref().unwrap().pending);
    assert!(h.get_by_label("Create").accesskit_node().is_disabled());
    assert!(h.get_by_label("Cancel").accesskit_node().is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().create_collection.is_some());
    ctx.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    assert!(h.state().create_collection.is_none());
    let collections = ctx.catalog().all_collections().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].name, "Retry album");
}

#[test]
#[ignore = "GPU image comparisons: task ui:visual"]
fn visual_snapshots() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.run();
    h.snapshot("catalogue-grid");
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    h.event(egui::Event::PointerGone);
    h.run();
    h.snapshot("catalogue-details");
    h.get_by_label("List").click();
    settle(&mut h);
    h.event(egui::Event::PointerGone);
    h.run();
    h.snapshot("catalogue-list");
    let mut narrow = fixture.harness([800.0, 600.0]);
    narrow.run();
    narrow.snapshot("catalogue-narrow");
    let mut results = h.take_snapshot_results();
    results.extend(narrow.take_snapshot_results());
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let id = ctx.catalog().insert_collection("Favourites").unwrap();
    ctx.catalog().add_to_collection(id, 1).unwrap();
    ctx.catalog().add_to_collection(id, 2).unwrap();
    let mut collections = fixture.harness([1100.0, 720.0]);
    collections.get_by_label("Favourites").click();
    settle(&mut collections);
    collections
        .get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
    settle(&mut collections);
    collections.event(egui::Event::PointerGone);
    collections.run();
    collections.snapshot("catalogue-collections");
    results.extend(collections.take_snapshot_results());
    collections.get_by_label("New Collection…").click();
    settle(&mut collections);
    collections.get_by_label("Name").type_text("Summer holiday");
    collections.event(egui::Event::PointerGone);
    collections.run();
    collections.snapshot("catalogue-create-collection");
    results.extend(collections.take_snapshot_results());
    collections.get_by_label("Cancel").click();
    settle(&mut collections);
    collections
        .get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_secondary();
    settle(&mut collections);
    collections.event(egui::Event::PointerGone);
    collections.run();
    collections.snapshot("catalogue-collection-menu");
    results.extend(collections.take_snapshot_results());
    collections.key_press(egui::Key::Escape);
    settle(&mut collections);
    let target = collections.get_by_label("Favourites").rect().center();
    drag_to(&mut collections, "Blue.bmp", target);
    collections.snapshot("catalogue-collection-drop");
    results.extend(collections.take_snapshot_results());
    collections.key_press(egui::Key::Escape);
    release_at(&mut collections, target);
}

#[test]
fn interaction_busy_controls_and_failed_edit_recover() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    let lock = open_catalogue(&fixture.path()).unwrap();
    lock.catalog()
        .connection()
        .execute_batch("BEGIN EXCLUSIVE")
        .unwrap();
    h.get_by_label("Rating 5").click();
    h.run_steps(3);
    assert!(h.state().worker.is_some());
    assert!(h.get_by_label("Rating 4").accesskit_node().is_disabled());
    // Disabled clicks must not enqueue a second edit.
    h.get_by_label("Rating 4").click();
    h.run_steps(2);
    lock.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    assert_eq!(lock.catalog().annotation(1).unwrap().unwrap().rating, 5);
    assert!(!h.get_by_label("Rating 4").accesskit_node().is_disabled());
    lock.catalog().connection().execute_batch(
        "CREATE TRIGGER reject_annotation BEFORE INSERT ON annotation BEGIN SELECT RAISE(ABORT, 'test write failure'); END;"
    ).unwrap();
    h.get_by_label("Rating 4").click();
    settle(&mut h);
    assert!(h.state().status.contains("test write failure"));
    assert_eq!(h.state().annotation.as_ref().unwrap().rating, 5);
    assert!(!h.get_by_label("Rating 4").accesskit_node().is_disabled());
}

#[test]
fn interaction_scroll_keeps_thumbnail_cache_bounded() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog().connection().execute_batch(
        "WITH RECURSIVE n(x) AS (VALUES(4) UNION ALL SELECT x+1 FROM n WHERE x<10000)
         INSERT INTO asset(id,uid,path,name,folder_id) SELECT x,'generated-'||x,'/synthetic/'||x,'Generated '||x,1 FROM n;
         INSERT INTO thumbnail(asset_id,size_class,source,format,width,height,orientation_applied,bytes,generator_version)
         SELECT asset.id,256,'synthetic','jpeg',64,32,0,t.bytes,1 FROM asset CROSS JOIN thumbnail t WHERE asset.id>3 AND t.asset_id=1;"
    ).unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    h.event(egui::Event::PointerMoved(egui::pos2(500.0, 400.0)));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        phase: egui::TouchPhase::Move,
        delta: egui::vec2(0.0, -2400.0),
        modifiers: egui::Modifiers::NONE,
    });
    settle(&mut h);
    assert!(h.state().visible_thumbnails.iter().any(|id| *id > 30));
    assert!(h.state().textures.len() < 100);
    assert!(
        h.state().textures.contains_key(&1),
        "selection stays cached after scrolling away"
    );
}
