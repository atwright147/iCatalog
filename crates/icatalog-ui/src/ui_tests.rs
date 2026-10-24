//! Interaction and image tests drive the real eframe App through AccessKit.
#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;
#[path = "../tests/support/mod.rs"]
mod support;
use egui_kittest::{
    kittest::{NodeT, Queryable},
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
    h.get_by_label("Red.bmp").click();
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
#[ignore = "GPU image comparisons: task ui:visual"]
fn visual_snapshots() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.run();
    h.snapshot("catalogue-grid");
    h.get_by_label("Red.bmp").click();
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
}

#[test]
fn interaction_busy_controls_and_failed_edit_recover() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("Red.bmp").click();
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
    h.get_by_label("Red.bmp").click();
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
