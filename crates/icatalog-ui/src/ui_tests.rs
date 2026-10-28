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
        self.harness_with_display(size, 1.0, egui::Theme::Dark)
    }
    fn harness_with_display(
        &self,
        size: [f32; 2],
        pixels_per_point: f32,
        theme: egui::Theme,
    ) -> Harness<'static, App> {
        let path = self.path();
        Harness::builder()
            .with_size(size)
            .with_pixels_per_point(pixels_per_point)
            .with_theme(theme)
            .with_options(
                SnapshotOptions::default()
                    .output_path(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")),
            )
            .build_eframe(move |cc| {
                typography::configure(&cc.egui_ctx);
                let (_, receiver) = std::sync::mpsc::channel();
                App::with_menu(open_catalogue(&path).unwrap(), path, None, receiver)
            })
    }
}

#[test]
fn readable_text_fits_rows_at_native_display_scales_in_both_themes() {
    let fixture = Fixture::new();
    seed_list_metadata(&fixture);
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            let mut h = fixture.harness_with_display([1100.0, 720.0], scale, theme);
            h.get_by_label("List").click();
            settle(&mut h);
            assert_eq!(h.ctx.pixels_per_point(), scale);
            let row = asset_rect(&h, "Red.bmp");
            let mut previous_bottom = row.top();
            for label in [
                "Red.bmp",
                "Rating: ★★★★☆",
                "Captured: 2000-02-28 14:30:00",
                "Modified: 2000-02-29 00:00:00 UTC",
                "Size: 1.5 MiB",
            ] {
                let rect = h
                    .get(By::new().label(label).role(egui::accesskit::Role::Label))
                    .rect();
                assert!(rect.height() >= 14.0, "{theme:?} {scale}: {label}");
                assert!(rect.top() >= previous_bottom, "{label} overlaps");
                assert!(row.contains_rect(rect), "{label} clips outside row");
                previous_bottom = rect.bottom();
            }
            assert!(asset_rect(&h, "Blue.bmp").top() > row.bottom());
            h.get(
                By::new()
                    .label("Red.bmp")
                    .role(egui::accesskit::Role::Button),
            )
            .click();
            settle(&mut h);
            assert_eq!(h.state().selection.ordered(&h.state().assets), vec![1]);
            h.get_by_label("Grid").click();
            settle(&mut h);
            let tile = asset_rect(&h, "Red.bmp");
            assert!(
                tile.height() - tile.width()
                    >= h.ctx.style_of(theme).text_styles[&egui::TextStyle::Body].size
            );
        }
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

fn seed_list_metadata(fixture: &Fixture) {
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog()
        .connection()
        .execute_batch(
            "UPDATE asset SET file_size=1572864,mtime=951782400 WHERE id=1;
         INSERT INTO annotation(asset_id,rating) VALUES(1,4);
         INSERT INTO asset_meta(asset_id,captured_local,make,model,lens,focal_length_mm) VALUES
         (1,'2000-02-28T14:30:00','Example','Camera 1','24–70 mm f/2.8',50);
         INSERT INTO metadata_raw(asset_id,scheme,key,seq,value) VALUES
         (1,'image_info','color_model',0,CAST('RGB' AS BLOB)),
         (1,'image_info','icc_profile',0,CAST('Synthetic RGB profile' AS BLOB)),
         (2,'image_info','color_model',0,CAST('CMYK' AS BLOB)),
         (3,'image_info','color_model',0,CAST('Lab' AS BLOB));",
        )
        .unwrap();
}

#[test]
fn list_rows_present_metadata_columns_and_refresh_after_edit() {
    let fixture = Fixture::new();
    seed_list_metadata(&fixture);
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("List").click();
    settle(&mut h);
    let red = asset_rect(&h, "Red.bmp");
    let blue = asset_rect(&h, "Blue.bmp");
    assert!(red.height() >= 100.0);
    assert!(blue.top() > red.bottom());
    assert!(h.state().textures.contains_key(&1));
    let name = h
        .get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Label),
        )
        .rect();
    let camera = h.get_by_label("Camera: Example Camera 1").rect();
    let color = h.get_by_label("Colour: RGB").rect();
    assert!(name.left() > red.left() + 100.0);
    assert!(camera.left() > name.right());
    assert!(color.left() > camera.right());
    for label in [
        "Rating: ★★★★☆",
        "Captured: 2000-02-28 14:30:00",
        "Modified: 2000-02-29 00:00:00 UTC",
        "Size: 1.5 MiB",
        "Type: BMP",
        "Lens: 24–70 mm f/2.8",
        "Focal length: 50 mm",
        "64 × 32 px",
        "Profile: Synthetic RGB profile",
        "Colour: CMYK",
        "Colour: Lab",
        "Captured: —",
        "Profile: —",
    ] {
        assert!(
            h.query_all(By::new().label(label)).next().is_some(),
            "{label}"
        );
    }
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    h.get_by_label("Details").click();
    settle(&mut h);
    h.get_by_label("Rating 5").click();
    settle(&mut h);
    assert!(h.query_by_label("Rating: ★★★★★").is_some());
    assert_eq!(
        open_catalogue(&fixture.path())
            .unwrap()
            .catalog()
            .annotation(1)
            .unwrap()
            .unwrap()
            .rating,
        5
    );
    h.get_by_label("Details").click();
    settle(&mut h);
    assert!(h.get_by_label("Colour: RGB").rect().right() < 1100.0);
}

#[test]
fn stored_thumbnail_orientation_supports_old_catalogues_without_double_rotation() {
    for (orientation, applied, expected) in [
        (Some(6), false, [32, 64]),
        (Some(8), false, [32, 64]),
        (Some(3), false, [64, 32]),
        (None, false, [64, 32]),
        (Some(262), false, [64, 32]),
        (Some(6), true, [32, 64]),
    ] {
        let fixture = Fixture::new();
        let ctx = open_catalogue(&fixture.path()).unwrap();
        if let Some(orientation) = orientation {
            ctx.catalog()
                .connection()
                .execute(
                    "INSERT INTO asset_meta(asset_id,orientation) VALUES (1,?1)",
                    [orientation],
                )
                .unwrap();
        }
        if applied {
            let t = icatalog_media::thumbnail_jpeg_with_orientation(
                &bmp([40, 50, 220]),
                256,
                orientation,
            )
            .unwrap();
            let mut stored = ctx.catalog().thumbnail(1, 256).unwrap().unwrap();
            stored.bytes = t.bytes;
            stored.width = t.width;
            stored.height = t.height;
            stored.orientation_applied = t.orientation_applied;
            stored.generator_version = icatalog_media::GENERATOR_VERSION;
            ctx.catalog().insert_thumbnail(&stored).unwrap();
        }
        let before = ctx.catalog().thumbnail(1, 256).unwrap().unwrap();
        // Fixture media paths do not exist: fallback only needs SQLite data.
        let mut h = fixture.harness([1100.0, 800.0]);
        assert_eq!(h.state().textures.get(&1).unwrap().size(), expected);
        h.get(By::new().label("List")).click();
        h.run();
        h.get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
        h.run();
        assert_eq!(h.state().selection.primary, Some(1));
        assert_eq!(h.state().textures.get(&1).unwrap().size(), expected);
        assert_eq!(ctx.catalog().thumbnail(1, 256).unwrap().unwrap(), before);
    }
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

fn open_collection_submenu(h: &mut Harness<'_, App>) {
    h.get(
        By::new()
            .label("Collections ⏵")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    h.run_steps(2);
}

fn click_open_collection_checkbox(h: &mut Harness<'_, App>, name: &str) {
    h.get(By::new().label(name).role(egui::accesskit::Role::CheckBox))
        .click();
}

fn click_collection_checkbox(h: &mut Harness<'_, App>, name: &str) {
    open_collection_submenu(h);
    click_open_collection_checkbox(h, name);
    settle(h);
}

fn asset_rect(h: &Harness<'_, App>, name: &str) -> egui::Rect {
    h.get(By::new().label(name).role(egui::accesskit::Role::Button))
        .rect()
}

fn begin_rectangle(
    h: &mut Harness<'_, App>,
    start: egui::Pos2,
    end: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.hover_at(start);
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    h.run_steps(1);
    h.hover_at(end);
    h.run_steps(3);
}

#[test]
fn interaction_asset_modifier_selection_and_filter_pruning() {
    let ctrl = egui::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    for mode in ["Grid", "List"] {
        let fixture = Fixture::new();
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
                .label("Green.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_modifiers(ctrl);
        settle(&mut h);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![1, 3]);
        assert!(h.query_by_label("Rating 5").is_none());
        h.get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_modifiers(ctrl);
        settle(&mut h);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![3]);
        h.get(
            By::new()
                .label("Blue.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_modifiers(egui::Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        });
        settle(&mut h);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![2, 3]);
        h.get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
        settle(&mut h);
        h.get(
            By::new()
                .label("Green.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_modifiers(shift);
        settle(&mut h);
        assert_eq!(
            h.state().selection.ordered(&h.state().assets),
            vec![1, 2, 3]
        );
        h.get(
            By::new()
                .label("Blue.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_modifiers(shift);
        settle(&mut h);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![1, 2]);
        h.get(
            By::new()
                .label("Green.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_modifiers(egui::Modifiers {
            shift: true,
            ctrl: true,
            ..Default::default()
        });
        settle(&mut h);
        assert_eq!(
            h.state().selection.ordered(&h.state().assets),
            vec![1, 2, 3]
        );
        open_asset_menu(&mut h, "Blue.bmp");
        assert_eq!(h.state().selection.ids.len(), 3);
        h.key_press(egui::Key::Escape);
        settle(&mut h);
        h.get_by_label("Search name or path").click();
        h.get_by_label("Search name or path").type_text("Blue");
        h.key_press(egui::Key::Enter);
        settle(&mut h);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![2]);
        h.get_by_label("x").click();
        settle(&mut h);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![2]);
    }
}

#[test]
fn interaction_rectangle_selection_replace_add_toggle_and_escape() {
    for mode in ["Grid", "List"] {
        let fixture = Fixture::new();
        let mut h = fixture.harness([1100.0, 720.0]);
        h.get_by_label(mode).click();
        settle(&mut h);
        let red = asset_rect(&h, "Red.bmp");
        let blue = asset_rect(&h, "Blue.bmp");
        let start = egui::pos2(blue.right() + 5.0, blue.bottom() + 1.0);
        let end = red.min + egui::vec2(3.0, 3.0);
        begin_rectangle(&mut h, start, end, egui::Modifiers::NONE);
        assert!(h.state().marquee.is_some(), "{mode}");
        assert_eq!(
            h.state().selection.ordered(&h.state().assets),
            vec![1, 2],
            "{mode}"
        );
        assert!(!egui::DragAndDrop::has_payload_of_type::<DraggedAsset>(
            &h.ctx
        ));
        release_at(&mut h, end);
        h.get(
            By::new()
                .label("Green.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
        settle(&mut h);
        begin_rectangle(
            &mut h,
            start,
            end,
            egui::Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        release_at(&mut h, end);
        assert_eq!(
            h.state().selection.ordered(&h.state().assets),
            vec![1, 2, 3]
        );
        begin_rectangle(
            &mut h,
            start,
            end,
            egui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
        );
        release_at(&mut h, end);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![3]);
        begin_rectangle(&mut h, start, end, egui::Modifiers::NONE);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![1, 2]);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        assert_eq!(h.state().selection.ordered(&h.state().assets), vec![3]);
        release_at(&mut h, end);
        // A click in empty space clears the selection without changing the view.
        begin_rectangle(&mut h, start, start, egui::Modifiers::NONE);
        release_at(&mut h, start);
        assert!(h.state().selection.ids.is_empty());
        assert_eq!(h.state().assets.len(), 3);
    }
}

#[test]
fn interaction_rectangle_selection_tracks_scroll_in_content_coordinates() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog().connection().execute_batch(
        "WITH RECURSIVE n(x) AS (VALUES(4) UNION ALL SELECT x+1 FROM n WHERE x<10000)
         INSERT INTO asset(id,uid,path,name,folder_id) SELECT x,'generated-'||x,'/synthetic/'||x,'Generated '||x,1 FROM n;"
    ).unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("List").click();
    settle(&mut h);
    let red = asset_rect(&h, "Red.bmp");
    let blue = asset_rect(&h, "Blue.bmp");
    let start = egui::pos2(red.right() + 5.0, red.top() + 2.0);
    let end = egui::pos2(red.left() + 2.0, blue.bottom() - 2.0);
    begin_rectangle(&mut h, start, end, egui::Modifiers::NONE);
    assert_eq!(h.state().selection.ordered(&h.state().assets), vec![1, 2]);
    assert!(
        h.ctx.dragged_id().is_none(),
        "rectangle selection must not leave an asset drag active: {:?}",
        h.ctx.dragged_id()
    );
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        phase: egui::TouchPhase::Move,
        delta: egui::vec2(0.0, -1600.0),
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert!(
        h.state().selection.ids.len() > 10,
        "selected {}",
        h.state().selection.ids.len()
    );
    assert!(h.state().selection.ids.contains(&1));
    release_at(&mut h, end);
    assert!(h.state().textures.len() < 100);
    h.get_by_label("Search name or path").click();
    h.get_by_label("Search name or path")
        .type_text("Generated 9999");
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(h.state().assets.len(), 1);
    assert!(h.state().selection.ids.is_empty());
}

#[test]
fn interaction_group_collection_drop_menu_and_atomic_failure() {
    for mode in ["Grid", "List"] {
        let fixture = Fixture::new();
        let ctx = open_catalogue(&fixture.path()).unwrap();
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
        .click_modifiers(egui::Modifiers {
            ctrl: true,
            ..Default::default()
        });
        settle(&mut h);
        let target = h.get_by_label("Album").rect().center();
        drag_to(&mut h, "Blue.bmp", target);
        assert_eq!(
            egui::DragAndDrop::payload::<DraggedAsset>(&h.ctx)
                .unwrap()
                .ids,
            vec![1, 2]
        );
        release_at(&mut h, target);
        assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1, 2]);
        open_asset_menu(&mut h, "Red.bmp");
        click_collection_checkbox(&mut h, "Album");
        assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
        ctx.catalog().connection().execute_batch("CREATE TRIGGER reject_second BEFORE INSERT ON collection_asset WHEN NEW.asset_id=2 BEGIN SELECT RAISE(ABORT, 'batch second asset failure'); END;").unwrap();
        open_asset_menu(&mut h, "Red.bmp");
        click_collection_checkbox(&mut h, "Album");
        assert!(h.state().status.contains("batch second asset failure"));
        assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
        assert_eq!(h.state().selection.ids.len(), 2);
        ctx.catalog()
            .connection()
            .execute_batch("DROP TRIGGER reject_second")
            .unwrap();
        open_asset_menu(&mut h, "Blue.bmp");
        click_collection_checkbox(&mut h, "Album");
        h.get_by_label("Album").click();
        settle(&mut h);
        open_asset_menu(&mut h, "Red.bmp");
        click_collection_checkbox(&mut h, "Album");
        assert!(h.state().selection.ids.is_empty());
        assert!(h.state().assets.is_empty());
        assert_eq!(ctx.catalog().stats().unwrap().assets, 3);
        assert!(open_catalogue(&fixture.path())
            .unwrap()
            .catalog()
            .collection_assets(1)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn interaction_collection_checkboxes_add_remove_and_normalize_mixed_selection() {
    for mode in ["Grid", "List"] {
        let fixture = Fixture::new();
        let ctx = open_catalogue(&fixture.path()).unwrap();
        let album = ctx.catalog().insert_collection("Album").unwrap();
        let favourites = ctx.catalog().insert_collection("Favourites").unwrap();
        ctx.catalog().add_to_collection(album, 1).unwrap();
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
        .click_modifiers(egui::Modifiers {
            ctrl: true,
            ..Default::default()
        });
        settle(&mut h);

        // Right-clicking a selected asset preserves the two-asset selection.
        open_asset_menu(&mut h, "Blue.bmp");
        assert_eq!(h.state().selection.ids.len(), 2);
        click_collection_checkbox(&mut h, "Album");
        assert_eq!(ctx.catalog().collection_assets(album).unwrap(), vec![1, 2]);

        // A checked collection removes every selected asset; an unchecked one
        // adds every selected asset.
        open_asset_menu(&mut h, "Red.bmp");
        click_collection_checkbox(&mut h, "Album");
        assert!(ctx.catalog().collection_assets(album).unwrap().is_empty());
        open_asset_menu(&mut h, "Red.bmp");
        click_collection_checkbox(&mut h, "Favourites");
        assert_eq!(
            ctx.catalog().collection_assets(favourites).unwrap(),
            vec![1, 2]
        );
    }
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
            assert_eq!(h.state().selection.primary, Some(2));
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
fn collection_count_space_keeps_long_names_separate_from_counts() {
    for width in [160.0, 220.0] {
        for selected in [false, true] {
            for count in [0, 12_345] {
                let name = "A very long collection name that must truncate beside the count";
                let mut h = Harness::builder().with_size([width, 80.0]).build_ui_state(
                    |ui, rects| {
                        let row = collection_row(ui, name, count, selected);
                        *rects = Some((
                            row.rect(egui::Id::new("collection_name")).unwrap(),
                            row.rect(egui::Id::new("collection_count")).unwrap(),
                            row.response.rect,
                        ));
                    },
                    None::<(egui::Rect, egui::Rect, egui::Rect)>,
                );
                h.run();
                let (name_rect, count_rect, row_rect) = h.state().unwrap();
                assert!(name_rect.right() < count_rect.left());
                assert!(row_rect.contains_rect(count_rect));
                assert!(row_rect.width() <= width);
                assert!(count_rect.width() > 0.0);
                // The count is in the same row without altering its accessible name.
                assert_eq!(h.get_by_label(name).rect(), row_rect);
            }
        }
    }
}

#[test]
fn interaction_collection_membership_cache_survives_redraws_and_skips_busy_reads() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let album = ctx.catalog().insert_collection("Album").unwrap();
    ctx.catalog().add_to_collection(album, 1).unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
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
    .click_modifiers(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
    settle(&mut h);
    open_asset_menu(&mut h, "Red.bmp");
    open_collection_submenu(&mut h);
    assert_eq!(
        h.state()
            .selection_collection_members
            .get(&album)
            .unwrap()
            .len(),
        1
    );

    ctx.catalog()
        .connection()
        .execute_batch("BEGIN EXCLUSIVE")
        .unwrap();
    h.run_steps(3);
    assert!(h.state().selection_membership_loaded);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    open_asset_menu(&mut h, "Blue.bmp");
    open_collection_submenu(&mut h);
    assert!(h.state().selection_membership_loaded);

    let params = collection_params("add", album, &[2]);
    h.state_mut().run_command("collection", &params);
    h.run_steps(3);
    assert!(h.state().worker.is_some());
    assert!(!h.state().selection_membership_loaded);
    assert!(h
        .get_by_label("Collections ⏵")
        .accesskit_node()
        .is_disabled());
    ctx.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    open_asset_menu(&mut h, "Red.bmp");
    open_collection_submenu(&mut h);
    assert_eq!(
        h.state()
            .selection_collection_members
            .get(&album)
            .unwrap()
            .len(),
        2
    );
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    h.get(
        By::new()
            .label("Green.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    open_asset_menu(&mut h, "Green.bmp");
    open_collection_submenu(&mut h);
    assert!(h.state().selection_membership_loaded);
    assert!(!h.state().selection_collection_members.contains_key(&album));
}

#[test]
fn interaction_collection_membership_read_error_retries_when_menu_reopens() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let album = ctx.catalog().insert_collection("Album").unwrap();
    ctx.catalog().add_to_collection(album, 1).unwrap();
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
        .execute_batch("ALTER TABLE collection_asset RENAME TO unavailable_memberships")
        .unwrap();
    open_asset_menu(&mut h, "Red.bmp");
    assert!(!h.state().selection_membership_loaded);
    assert!(h
        .state()
        .status
        .contains("Cannot load collection membership"));
    ctx.catalog()
        .connection()
        .execute_batch("ALTER TABLE unavailable_memberships RENAME TO collection_asset")
        .unwrap();
    h.run_steps(3);
    assert!(
        !h.state().selection_membership_loaded,
        "an open menu must not retry reads every frame"
    );
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    open_asset_menu(&mut h, "Red.bmp");
    open_collection_submenu(&mut h);
    assert!(h.state().selection_membership_loaded);
    assert_eq!(
        h.state()
            .selection_collection_members
            .get(&album)
            .unwrap()
            .len(),
        1
    );
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
    assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![1]));
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
            FolderSelection::Collections(vec![favourites])
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
fn interaction_collections_shift_ranges_ctrl_command_toggles_and_search() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let ctrl = egui::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    let command = egui::Modifiers {
        mac_cmd: true,
        command: true,
        ..Default::default()
    };
    let ctrl_shift = egui::Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    for mode in ["Grid", "List"] {
        let fixture = Fixture::new();
        let ctx = open_catalogue(&fixture.path()).unwrap();
        // Insertion order differs from displayed name order.
        let delta = ctx.catalog().insert_collection("Delta").unwrap();
        let bravo = ctx.catalog().insert_collection("Bravo").unwrap();
        let alpha = ctx.catalog().insert_collection("Alpha").unwrap();
        let charlie = ctx.catalog().insert_collection("Charlie").unwrap();
        for (collection, asset) in [(alpha, 1), (bravo, 2), (charlie, 1), (charlie, 3)] {
            ctx.catalog().add_to_collection(collection, asset).unwrap();
        }
        let mut h = fixture.harness([1100.0, 720.0]);
        h.get_by_label(mode).click();
        settle(&mut h);
        let assert_view = |h: &Harness<'_, App>, collections: &[i64], assets: &[i64]| {
            assert_eq!(
                h.state().folder_sel,
                if collections.is_empty() {
                    FolderSelection::All
                } else {
                    FolderSelection::Collections(collections.to_vec())
                }
            );
            assert_eq!(
                h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
                assets
            );
        };
        h.get_by_label("Bravo").click_modifiers(shift);
        settle(&mut h);
        assert_view(&h, &[bravo], &[2]);
        h.get_by_label("Alpha").click();
        settle(&mut h);
        h.get_by_label("Charlie").click_modifiers(shift);
        settle(&mut h);
        assert_view(&h, &[alpha, bravo, charlie], &[1, 2, 3]);
        h.get_by_label("Bravo").click_modifiers(shift);
        settle(&mut h);
        assert_view(&h, &[alpha, bravo], &[1, 2]);
        h.get_by_label("Delta").click_modifiers(ctrl);
        settle(&mut h);
        assert_view(&h, &[alpha, bravo, delta], &[1, 2]);
        h.get_by_label("Bravo").click_modifiers(ctrl);
        settle(&mut h);
        assert_view(&h, &[alpha, delta], &[1]);
        h.get_by_label("Charlie").click_modifiers(command);
        settle(&mut h);
        assert_view(&h, &[alpha, charlie, delta], &[1, 3]);
        h.get_by_label("Bravo").click_modifiers(ctrl_shift);
        settle(&mut h);
        assert_view(&h, &[alpha, bravo, charlie, delta], &[1, 2, 3]);
        h.get_by_label("Search name or path").click();
        h.get_by_label("Search name or path").type_text("Blue");
        h.key_press(egui::Key::Enter);
        settle(&mut h);
        assert_view(&h, &[alpha, bravo, charlie, delta], &[2]);
        h.get_by_label("Bravo").click_modifiers(ctrl);
        settle(&mut h);
        assert_view(&h, &[alpha, charlie, delta], &[]);
        assert_eq!(h.state().search, "Blue");
        h.get_by_label("x").click();
        settle(&mut h);
        h.get_by_label("Bravo").click();
        settle(&mut h);
        assert_view(&h, &[bravo], &[2]);
        h.get_by_label("Bravo").click();
        settle(&mut h);
        assert_view(&h, &[], &[1, 2, 3]);
        h.get_by_label("Delta").click_modifiers(shift);
        settle(&mut h);
        assert_view(&h, &[delta], &[]);
        h.get_by_label("Delta").click_modifiers(ctrl);
        settle(&mut h);
        assert_view(&h, &[], &[1, 2, 3]);
        // Folder browsing resets the previous collection range anchor.
        h.get_by_label("Alpha").click();
        settle(&mut h);
        h.get_by_label("Photos").click();
        settle(&mut h);
        h.get_by_label("Charlie").click_modifiers(shift);
        settle(&mut h);
        assert_view(&h, &[charlie], &[1, 3]);
        h.get_by_label("Alpha").click_modifiers(shift);
        settle(&mut h);
        assert_view(&h, &[alpha, bravo, charlie], &[1, 2, 3]);
        let target = h.get_by_label("Alpha").rect().center();
        drag_to(&mut h, "Blue.bmp", target);
        release_at(&mut h, target);
        assert_view(&h, &[alpha, bravo, charlie], &[1, 2, 3]);
        assert_eq!(ctx.catalog().collection_assets(alpha).unwrap(), vec![1, 2]);
        assert_eq!(ctx.catalog().collection_assets(bravo).unwrap(), vec![2]);
        assert_eq!(
            ctx.catalog().collection_assets(charlie).unwrap(),
            vec![1, 3]
        );
        assert!(ctx.catalog().collection_assets(delta).unwrap().is_empty());
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
        assert_eq!(h.state().selection.primary, Some(2));
        click_collection_checkbox(&mut h, "Favourites");
        assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![2]);
        h.get(
            By::new()
                .label("Blue.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_secondary();
        settle(&mut h);
        click_collection_checkbox(&mut h, "Album");
        h.get_by_label("Favourites").click();
        settle(&mut h);
        h.get_by_label("Album").click_modifiers(egui::Modifiers {
            ctrl: true,
            ..Default::default()
        });
        settle(&mut h);
        h.get(
            By::new()
                .label("Blue.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_secondary();
        settle(&mut h);
        click_collection_checkbox(&mut h, "Favourites");
        assert_eq!(
            h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(h.state().selection.primary, Some(2));
        h.get_by_label("Album").click_modifiers(egui::Modifiers {
            ctrl: true,
            ..Default::default()
        });
        settle(&mut h);
        assert!(h.state().assets.is_empty());
        assert!(h.state().selection.primary.is_none());
        assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
        assert_eq!(ctx.catalog().collection_assets(2).unwrap(), vec![2]);
        assert_eq!(ctx.catalog().stats().unwrap().assets, 3);
    }
}

#[test]
fn interaction_collection_drag_and_context_menu_report_failure() {
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
    open_asset_menu(&mut h, "Red.bmp");
    click_collection_checkbox(&mut h, "Album");
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
    assert_eq!(h.state().selection.primary, Some(1));
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
    assert_eq!(h.state().collection_counts.get(&1), Some(&0));
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
    click_collection_checkbox(&mut h, "Favourites");
    assert_eq!(h.state().selection.primary, Some(1));
    assert_eq!(h.state().collection_counts.get(&1), Some(&1));
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
    click_collection_checkbox(&mut h, "Favourites");
    assert!(h.state().assets.is_empty());
    assert!(h.state().selection.primary.is_none());
    assert!(reopened.catalog().collection_assets(1).unwrap().is_empty());
    assert_eq!(h.state().collection_counts.get(&1), Some(&0));
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
    let album = ctx.catalog().insert_collection("Album").unwrap();
    ctx.catalog().add_to_collection(album, 1).unwrap();
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
        .execute_batch(
            "CREATE TRIGGER reject_removal BEFORE DELETE ON collection_asset
         BEGIN SELECT RAISE(ABORT, 'synthetic membership failure'); END;",
        )
        .unwrap();
    open_asset_menu(&mut h, "Red.bmp");
    click_collection_checkbox(&mut h, "Album");
    assert!(h.state().status.contains("synthetic membership failure"));
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1]);
    open_asset_menu(&mut h, "Red.bmp");
    h.get(
        By::new()
            .label("Collections ⏵")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    h.run_steps(2);
    assert!(!h
        .get(
            By::new()
                .label("Album")
                .role(egui::accesskit::Role::CheckBox)
        )
        .accesskit_node()
        .is_disabled());
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    ctx.catalog()
        .connection()
        .execute_batch("DROP TRIGGER reject_removal")
        .unwrap();
    open_asset_menu(&mut h, "Red.bmp");
    click_collection_checkbox(&mut h, "Album");
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
            h.state().selection.primary,
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
    // Mesa and Metal differ by two red-channel levels at two pixels of the
    // clipped green thumbnail. CI's measured YIQ distance is below 1.0.
    // Keep a zero failed-pixel allowance and the normal tolerance elsewhere.
    let clipped_thumbnail_options = h
        .options()
        .clone()
        .threshold(egui_kittest::OsThreshold::new(0.6).linux(1.0));
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
    narrow.snapshot_options("catalogue-narrow", &clipped_thumbnail_options);
    let mut results = h.take_snapshot_results();
    results.extend(narrow.take_snapshot_results());
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let id = ctx.catalog().insert_collection("Favourites").unwrap();
    ctx.catalog().add_to_collection(id, 1).unwrap();
    ctx.catalog().add_to_collection(id, 2).unwrap();
    let album = ctx.catalog().insert_collection("Album").unwrap();
    ctx.catalog().add_to_collection(album, 1).unwrap();
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
                .label("Blue.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_modifiers(egui::Modifiers {
            ctrl: true,
            ..Default::default()
        });
    settle(&mut collections);
    collections
        .get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_secondary();
    settle(&mut collections);
    collections
        .get(
            By::new()
                .label("Collections ⏵")
                .role(egui::accesskit::Role::Button),
        )
        .click();
    collections.run_steps(2);
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
    let portfolio = ctx.catalog().insert_collection("Portfolio").unwrap();
    ctx.catalog().add_to_collection(portfolio, 1).unwrap();
    ctx.catalog().add_to_collection(portfolio, 3).unwrap();
    let mut multiple = fixture.harness([1100.0, 720.0]);
    multiple.get_by_label("Favourites").click();
    settle(&mut multiple);
    multiple
        .get_by_label("Portfolio")
        .click_modifiers(egui::Modifiers {
            ctrl: true,
            ..Default::default()
        });
    settle(&mut multiple);
    multiple.event(egui::Event::PointerGone);
    multiple.run();
    multiple.snapshot("catalogue-collections-multiple");
    results.extend(multiple.take_snapshot_results());
    let mut assets = fixture.harness([1100.0, 720.0]);
    assets
        .get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
    settle(&mut assets);
    assets
        .get(
            By::new()
                .label("Green.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click_modifiers(egui::Modifiers {
            shift: true,
            ..Default::default()
        });
    settle(&mut assets);
    assets.event(egui::Event::PointerGone);
    assets.run();
    assets.snapshot("catalogue-assets-multiple");
    results.extend(assets.take_snapshot_results());
    assets.get_by_label("List").click();
    settle(&mut assets);
    assets.event(egui::Event::PointerGone);
    assets.run();
    assets.snapshot("catalogue-assets-list");
    results.extend(assets.take_snapshot_results());
    assets.get_by_label("Grid").click();
    settle(&mut assets);
    let red = asset_rect(&assets, "Red.bmp");
    let blue = asset_rect(&assets, "Blue.bmp");
    let start = egui::pos2(blue.right() + 5.0, blue.bottom() + 1.0);
    let end = red.min + egui::vec2(3.0, 3.0);
    begin_rectangle(&mut assets, start, end, egui::Modifiers::NONE);
    assets.snapshot("catalogue-assets-rectangle");
    results.extend(assets.take_snapshot_results());
    release_at(&mut assets, end);

    let long_name = "A very long collection name that must leave room for its count";
    let long = ctx.catalog().insert_collection(long_name).unwrap();
    for asset in [1, 2, 3] {
        ctx.catalog().add_to_collection(long, asset).unwrap();
    }
    ctx.catalog()
        .insert_collection("An empty collection with another long name")
        .unwrap();
    let mut counts = fixture.harness([800.0, 600.0]);
    counts.get_by_label(long_name).click();
    settle(&mut counts);
    counts.event(egui::Event::PointerGone);
    counts.run();
    counts.snapshot_options("catalogue-collection-counts", &clipped_thumbnail_options);
    results.extend(counts.take_snapshot_results());

    let rich_fixture = Fixture::new();
    seed_list_metadata(&rich_fixture);
    let mut rich = rich_fixture.harness([1100.0, 720.0]);
    rich.get_by_label("List").click();
    settle(&mut rich);
    rich.event(egui::Event::PointerGone);
    rich.run();
    rich.snapshot("catalogue-list-metadata");
    results.extend(rich.take_snapshot_results());
    let mut rich_narrow = rich_fixture.harness([800.0, 600.0]);
    rich_narrow.get_by_label("List").click();
    settle(&mut rich_narrow);
    rich_narrow.event(egui::Event::PointerGone);
    rich_narrow.run();
    rich_narrow.snapshot("catalogue-list-narrow");
    results.extend(rich_narrow.take_snapshot_results());
    for (name, scale, theme) in [
        ("catalogue-list-light", 1.0, egui::Theme::Light),
        ("catalogue-list-retina", 2.0, egui::Theme::Dark),
    ] {
        let mut display = rich_fixture.harness_with_display([1100.0, 720.0], scale, theme);
        display.get_by_label("List").click();
        settle(&mut display);
        display.event(egui::Event::PointerGone);
        display.run();
        display.snapshot(name);
        results.extend(display.take_snapshot_results());
    }
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
    let mut visible: Vec<i64> = h
        .state()
        .visible_thumbnails
        .iter()
        .copied()
        .filter(|id| *id > 3)
        .collect();
    visible.sort_unstable();
    let target = *visible.get(visible.len() / 2).unwrap();
    let name = h
        .state()
        .assets
        .iter()
        .find(|a| a.id == target)
        .unwrap()
        .name
        .clone();
    h.get(By::new().label(&name).role(egui::accesskit::Role::Button))
        .click_modifiers(egui::Modifiers {
            shift: true,
            ..Default::default()
        });
    settle(&mut h);
    assert_eq!(
        h.state().selection.ids.len(),
        usize::try_from(target).unwrap()
    );
    assert!(
        h.state().textures.len() < 100,
        "offscreen range selection must not decode all selected thumbnails"
    );
}
