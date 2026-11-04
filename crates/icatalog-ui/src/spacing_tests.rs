//! Layout and editing checks for the text panels at native display scales.
use super::*;

#[test]
fn inspector_exposure_is_concise_and_preserves_stored_precision() {
    let fixture = Fixture::new();
    seed_list_metadata(&fixture);
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog()
        .connection()
        .execute(
            "UPDATE asset_meta SET focal_length_mm=39.4 WHERE asset_id=1",
            [],
        )
        .unwrap();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
            h.get(
                By::new()
                    .label("Red.bmp")
                    .role(egui::accesskit::Role::Button),
            )
            .click();
            settle(&mut h);
            let lens = h.get_by_label("Lens: 24–70 mm f/2.8").rect();
            let exposure = h.get_by_label("39.4mm  f/2.8  0.16s  ISO12800").rect();
            assert!(exposure.top() - lens.bottom() >= 7.5);
            assert!(exposure.left() >= 480.0 && exposure.right() < 800.0);
        }
    }
    assert_eq!(
        ctx.catalog().asset_meta(1).unwrap().unwrap().exposure_s,
        Some(1.0 / 6.0)
    );
}

#[test]
fn inspector_path_truncates_in_the_middle_and_reveals_the_full_value_on_hover() {
    fn path_galleys(shape: &egui::Shape, paths: &mut Vec<std::sync::Arc<egui::Galley>>) {
        match shape {
            egui::Shape::Text(text) if text.galley.text().starts_with("Path: ") => {
                paths.push(text.galley.clone());
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    path_galleys(shape, paths);
                }
            }
            _ => {}
        }
    }

    let fixture = Fixture::new();
    let path = "/synthetic/Photos/a long folder name/旅行の写真/another long folder/Red.bmp";
    let full = format!("Path: {path}");
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog()
        .connection()
        .execute("UPDATE asset SET path=?1 WHERE id=1", [path])
        .unwrap();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            for list in [false, true] {
                let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
                h.get(
                    By::new()
                        .label("Red.bmp")
                        .role(egui::accesskit::Role::Button),
                )
                .click();
                settle(&mut h);
                if list {
                    h.get_by_label("List").click();
                    settle(&mut h);
                    h.get_by_label("Details").click();
                    settle(&mut h);
                }
                h.event(egui::Event::PointerGone);
                h.run();
                let bounds = h.get_by_label(&full).rect();
                assert!(bounds.left() >= 480.0 && bounds.right() < 800.0);
                let mut paths = Vec::new();
                for shape in &h.output().shapes {
                    path_galleys(&shape.shape, &mut paths);
                }
                assert_eq!(paths.len(), 1);
                let galley = &paths[0];
                assert_eq!(galley.rows.len(), 1);
                assert!(galley.text().starts_with("Path: /synthetic/"));
                assert!(galley.text().contains('…'));
                assert!(galley.text().ends_with("/Red.bmp"));
                assert!(bounds.height() < 25.0);

                h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
                h.get_by_label(&full).hover();
                h.run_steps(3);
                let mut hovered = Vec::new();
                for shape in &h.output().shapes {
                    path_galleys(&shape.shape, &mut hovered);
                }
                assert!(hovered.iter().any(|galley| galley.text() == full));
                assert_eq!(h.state().selection.primary, Some(1));
            }
        }
    }
    assert_eq!(ctx.catalog().asset(1).unwrap().unwrap().path, path);
}

#[test]
fn rename_and_edit_rules_put_name_beside_its_field_and_align_rule_columns() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let id = ctx.catalog().insert_collection("Picks").unwrap();
    ctx.catalog()
        .set_smart_rules(
            id,
            &icatalog_catalog::SmartRules {
                min_rating: Some(4),
                ..Default::default()
            },
        )
        .unwrap();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            for edit_rules in [false, true] {
                let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
                h.get_by_label("Picks (Smart)").click_secondary();
                settle(&mut h);
                h.get_by_label(if edit_rules {
                    "Edit Rules…"
                } else {
                    "Rename…"
                })
                .click();
                settle(&mut h);
                let label = h
                    .get(By::new().label("Name").role(egui::accesskit::Role::Label))
                    .rect();
                let field = h.get_by_label("Name").rect();
                assert!((label.center().y - field.center().y).abs() < 1.0);
                assert!(field.left() - label.right() >= 15.5);
                assert!(field.right() < 800.0 && field.top() >= 0.0);
                if edit_rules {
                    assert!((field.left() - h.get_by_label("Format").rect().left()).abs() < 0.5);
                    let rules = h.get(By::new().label("Rules").role(egui::accesskit::Role::Group));
                    let bounds = rules.rect();
                    assert!(field.bottom() < bounds.top());
                    for caption in [
                        "Match all filled rules. Blank fields match any value.",
                        "Format",
                        "Captured From",
                        "Captured To",
                        "Camera Contains",
                        "Lens Contains",
                        "Minimum Rating",
                        "Maximum Rating",
                        "Label Number",
                        "Keyword",
                        "Filename or Path Contains",
                    ] {
                        let child = rules.get_by_label(caption);
                        assert!(bounds.contains_rect(child.rect()), "{caption}");
                    }
                    assert!(rules.query_by_label("Name").is_none());
                    assert!(rules.query_by_label("Save").is_none());
                    assert!(rules.query_by_label("0 matching items").is_none());
                    assert!(h.get_by_label("Save").rect().top() > bounds.bottom());
                    assert!(h.get_by_label("Save").rect().bottom() < 600.0);
                } else {
                    assert!(h
                        .query(By::new().label("Rules").role(egui::accesskit::Role::Group))
                        .is_none());
                    // Opening Rename must still focus and select the old name.
                    h.get_by_label("Name").type_text("Replacement");
                    h.run_steps(2);
                    assert_eq!(
                        h.get_by_label("Name").value().as_deref(),
                        Some("Replacement")
                    );
                }
                h.get_by_label("Cancel").click();
                settle(&mut h);
                assert!(h.state().create_collection.is_none());
            }
        }
    }
    assert_eq!(ctx.catalog().collection(id).unwrap().unwrap().name, "Picks");
}

#[test]
fn inspector_rows_have_room_and_scrolled_annotations_still_save() {
    let fixture = Fixture::new();
    seed_list_metadata(&fixture);
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            let mut h = fixture.harness_with_display([800.0, 480.0], scale, theme);
            h.get(
                By::new()
                    .label("Red.bmp")
                    .role(egui::accesskit::Role::Button),
            )
            .click();
            settle(&mut h);
            let path = h.get_by_label("Path: /synthetic/Photos/Red.bmp").rect();
            let format = h.get_by_label("Format: BMP  64x32").rect();
            assert!(format.top() - path.bottom() >= 7.5);
            let size = h.get_by_label("Size: 1.5 MiB").rect();
            assert!(size.top() - format.bottom() >= 7.5);
            let camera = h.get_by_label("Camera: Example Camera 1").rect();
            let lens = h.get_by_label("Lens: 24–70 mm f/2.8").rect();
            assert!(lens.top() - camera.bottom() >= 7.5);

            // A short window puts annotations below the viewport. Scrolling
            // must expose the input and its action together.
            assert!(h.get_by_label("Add").rect().bottom() > 450.0);
            h.get_by_label("Add").scroll_to_me();
            settle(&mut h);
            for label in ["Keyword", "Add"] {
                let rect = h.get_by_label(label).rect();
                assert!(
                    rect.top() >= 0.0 && rect.bottom() < 450.0,
                    "{label}: {rect:?}"
                );
                assert!(
                    rect.left() >= 480.0 && rect.right() < 800.0,
                    "{label}: {rect:?}"
                );
            }
            h.get_by_label("Keyword").click();
            let keyword = format!("Spacious {theme:?} {scale}");
            h.get_by_label("Keyword").type_text(&keyword);
            h.run_steps(2);
            h.get_by_label("Add").click();
            settle(&mut h);
            let reopened = open_catalogue(&fixture.path()).unwrap();
            assert!(reopened
                .catalog()
                .asset_keywords(1)
                .unwrap()
                .iter()
                .any(|k| k.name == keyword));
        }
    }
}

#[test]
fn padded_collection_dialogs_fit_and_keep_label_value_columns_separate() {
    let fixture = Fixture::new();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
            h.get_by_label("New Collection…").click();
            settle(&mut h);
            h.get_by_label("Smart Collection").click();
            settle(&mut h);
            let mut previous_bottom = 0.0;
            for label in [
                "Format",
                "Captured From",
                "Captured To",
                "Camera Contains",
                "Lens Contains",
                "Minimum Rating",
                "Maximum Rating",
                "Label Number",
                "Keyword",
                "Filename or Path Contains",
            ] {
                let field = h.get_by_label(label).rect();
                let caption = h
                    .get(By::new().label(label).role(egui::accesskit::Role::Label))
                    .rect();
                assert!(
                    field.left() - caption.right() >= 15.5,
                    "{label}: {caption:?} {field:?}"
                );
                assert!(field.top() - previous_bottom >= 3.5, "{label}: {field:?}");
                assert!(field.right() < 800.0 && field.bottom() < 600.0);
                previous_bottom = field.bottom();
            }
            for label in ["Name", "Cancel", "Create"] {
                let rect = h.get_by_label(label).rect();
                assert!(
                    rect.top() >= 0.0 && rect.bottom() < 600.0,
                    "{label}: {rect:?}"
                );
            }
            let cancel = h.get_by_label("Cancel").rect();
            assert!(h.get_by_label("Create").rect().left() - cancel.right() >= 11.5);
            h.get_by_label("Cancel").click();
            settle(&mut h);
            assert!(h.state().create_collection.is_none());
        }
    }
}

#[test]
fn padded_first_sync_dialog_keeps_results_and_actions_in_the_window() {
    let fixture = Fixture::new();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 2.0] {
            let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
            h.state_mut().open_sync_dialog(false);
            h.state_mut().sync_dialog.as_mut().unwrap().error =
                Some("A long diagnostic message. ".repeat(100));
            settle(&mut h);
            for label in ["Server address", "Cancel", "Sync now"] {
                let rect = h.get_by_label(label).rect();
                assert!(
                    rect.top() >= 0.0 && rect.bottom() < 600.0,
                    "{label}: {rect:?}"
                );
            }
            h.get_by_label("Cancel").click();
            settle(&mut h);
            assert!(h.state().sync_dialog.is_none());
        }
    }
}
