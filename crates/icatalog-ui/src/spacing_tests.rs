//! Layout and editing checks for the text panels at native display scales.
use super::*;

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
                    assert!(h.get_by_label("Save").rect().bottom() < 600.0);
                } else {
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
                assert!(field.top() - previous_bottom >= 5.5, "{label}: {field:?}");
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
