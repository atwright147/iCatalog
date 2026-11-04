//! Maximum/range rules use the same stars, command worker and live preview.
use super::*;

fn set_stars(h: &mut Harness<'_, App>, field: &str, key: egui::Key) {
    h.get_by_label(&format!("{field} 1")).focus();
    h.run_steps(2);
    h.key_press(key);
    settle(h);
}

fn open_rules(h: &mut Harness<'_, App>, name: &str) {
    h.get_by_label(&format!("{name} (Smart)")).click_secondary();
    settle(h);
    h.get_by_label("Edit Rules…").click();
    settle(h);
}

#[test]
fn maximum_stars_preview_save_validate_cancel_clear_and_reopen_at_native_scales() {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            let fixture = Fixture::new();
            let c = open_catalogue(&fixture.path()).unwrap();
            c.catalog()
                .connection()
                .execute_batch("INSERT INTO annotation(asset_id,rating) VALUES(1,5),(2,3)")
                .unwrap();
            let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
            h.get_by_label("New Collection…").click();
            settle(&mut h);
            h.get_by_label("Name").type_text("Range");
            h.get_by_label("Smart Collection").click();
            settle(&mut h);
            h.get_by_label("Maximum Rating 4").click();
            settle(&mut h);
            assert!(h.query_by_label("2 matching items").is_some());
            assert!(h.query_by_label("Rating is at most 4 out of 5").is_some());
            h.get_by_label("Minimum Rating 2").click();
            settle(&mut h);
            assert!(h.query_by_label("1 matching item").is_some());
            for caption in ["Maximum Rating", "Create"] {
                let bounds = h.get_by_label(caption).rect();
                assert!(
                    bounds.top() >= 0.0 && bounds.bottom() < 600.0,
                    "{caption}: {bounds:?}"
                );
            }
            h.get_by_label("Create").click();
            settle(&mut h);
            assert_eq!(
                c.catalog().smart_rules(1).unwrap().unwrap().max_rating,
                Some(4)
            );
            h.get_by_label("Range (Smart)").click();
            settle(&mut h);
            assert_eq!(
                h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
                vec![2]
            );

            open_rules(&mut h, "Range");
            assert_eq!(
                h.get_by_label("Maximum Rating").value().as_deref(),
                Some("4 out of 5")
            );
            h.get_by_label("Maximum Rating 1").click();
            settle(&mut h);
            assert!(h.query_by_label("Preview unavailable").is_some());
            assert!(h.query_by_label("1 matching item").is_none());
            h.get_by_label("Save").click();
            settle(&mut h);
            assert!(h
                .state()
                .create_collection
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("minimum rating must not exceed maximum rating"));
            assert_eq!(
                h.get_by_label("Maximum Rating").value().as_deref(),
                Some("1 out of 5")
            );
            assert_eq!(
                c.catalog().smart_rules(1).unwrap().unwrap().max_rating,
                Some(4)
            );
            h.get_by_label("Maximum Rating 4").click();
            settle(&mut h);
            h.get_by_label("Maximum Rating 4").click();
            settle(&mut h);
            assert_eq!(
                h.get_by_label("Maximum Rating").value().as_deref(),
                Some("Not set")
            );
            assert!(h.query_by_label("2 matching items").is_some());
            h.get_by_label("Cancel").click();
            settle(&mut h);
            assert_eq!(
                c.catalog().smart_rules(1).unwrap().unwrap().max_rating,
                Some(4)
            );

            open_rules(&mut h, "Range");
            set_stars(&mut h, "Maximum Rating", egui::Key::Delete);
            h.get_by_label("Save").click();
            settle(&mut h);
            assert_eq!(
                c.catalog().smart_rules(1).unwrap().unwrap().max_rating,
                None
            );
            assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![1, 2]);
            open_rules(&mut h, "Range");
            set_stars(&mut h, "Minimum Rating", egui::Key::Delete);
            set_stars(&mut h, "Maximum Rating", egui::Key::Num0);
            assert!(h.query_by_label("1 matching item").is_some());
            h.get_by_label("Save").click();
            settle(&mut h);
            let saved = c.catalog().smart_rules(1).unwrap().unwrap();
            assert_eq!(saved.min_rating, None);
            assert_eq!(saved.max_rating, Some(0));
            assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![3]);
        }
    }
}

#[test]
fn pending_rule_save_disables_maximum_stars_and_preserves_the_submitted_value() {
    let fixture = Fixture::new();
    let c = open_catalogue(&fixture.path()).unwrap();
    let id = c.catalog().insert_collection("Capped").unwrap();
    c.catalog()
        .set_smart_rules(
            id,
            &icatalog_catalog::SmartRules {
                max_rating: Some(4),
                ..Default::default()
            },
        )
        .unwrap();
    let mut h = fixture.harness([800.0, 600.0]);
    open_rules(&mut h, "Capped");
    h.get_by_label("Maximum Rating 3").click();
    settle(&mut h);
    c.catalog()
        .connection()
        .execute_batch("BEGIN EXCLUSIVE")
        .unwrap();
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert!(h.state().worker.is_some());
    assert!(h
        .get_by_label("Maximum Rating 2")
        .accesskit_node()
        .is_disabled());
    h.get_by_label("Maximum Rating 2").click();
    h.key_press(egui::Key::Num1);
    h.run_steps(2);
    c.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    assert_eq!(
        c.catalog().smart_rules(id).unwrap().unwrap().max_rating,
        Some(3)
    );
}
