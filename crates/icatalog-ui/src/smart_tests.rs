//! Real widgets using the same fixtures and worker as the desktop app.
use super::*;

#[test]
fn smart_rules_fit_a_narrow_window_and_all_fields_remain_reachable() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([800.0, 600.0]);
    h.get_by_label("New Collection…").click();
    settle(&mut h);
    h.get_by_label("Name").type_text("Narrow rules");
    h.run_steps(2);
    h.get_by_label("Smart Collection").click();
    settle(&mut h);
    h.run_steps(10);
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
        "Cancel",
        "Create",
    ] {
        let rect = h.get_by_label(label).rect();
        assert!(
            rect.left() >= 0.0
                && rect.right() <= 800.0
                && rect.top() >= 0.0
                && rect.bottom() <= 600.0,
            "{label}: {rect:?}"
        );
    }
    h.get_by_label("Filename or Path Contains").click();
    h.run_steps(2);
    h.get_by_label("Filename or Path Contains").type_text("Red");
    h.run_steps(2);
    h.get_by_label("Create").click();
    settle(&mut h);
    let c = open_catalogue(&fixture.path()).unwrap();
    assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![1]);
}

#[test]
fn smart_create_validate_edit_cancel_and_reopen() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("New Collection…").click();
    settle(&mut h);
    h.get_by_label("Name").type_text("JPEGs");
    h.run_steps(2);
    h.get_by_label("Smart Collection").click();
    settle(&mut h);
    h.run_steps(10);
    assert!(h.state().create_collection.as_ref().unwrap().smart);
    h.get_by_label("Create").click();
    settle(&mut h);
    assert!(
        h.state().create_collection.as_ref().is_some_and(|d| d
            .error
            .as_ref()
            .is_some_and(|e| e.contains("at least one rule"))),
        "status={}, dialog={:?}",
        h.state().status,
        h.state()
            .create_collection
            .as_ref()
            .map(|d| (&d.name, d.smart, &d.error))
    );
    assert!(h.state().collections.is_empty());
    h.get_by_label("Format").click();
    h.run_steps(2);
    h.get_by_label("Format").type_text("JPEG");
    h.run_steps(2);
    h.get_by_label("Create").click();
    settle(&mut h);
    assert!(h.state().create_collection.is_none());
    h.get_by_label("JPEGs (Smart)").click();
    settle(&mut h);
    assert!(h.state().assets.is_empty());
    assert_eq!(h.state().collection_counts.get(&1), Some(&0));
    h.get_by_label("JPEGs (Smart)").click_secondary();
    settle(&mut h);
    h.get_by_label("Edit Rules…").click();
    settle(&mut h);
    assert!(h.get_by_label("Name").accesskit_node().is_disabled());
    h.get_by_label("Captured From").click();
    h.run_steps(2);
    h.get_by_label("Captured From").type_text("2023-02-29");
    h.run_steps(2);
    h.get_by_label("Save Rules").click();
    settle(&mut h);
    assert!(h
        .state()
        .create_collection
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("real dates"));
    h.get_by_label("Cancel").click();
    settle(&mut h);
    let c = open_catalogue(&fixture.path()).unwrap();
    let rules = c.catalog().smart_rules(1).unwrap().unwrap();
    assert_eq!(rules.format.as_deref(), Some("JPEG"));
    assert!(rules.captured_from.is_none());
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("JPEGs (Smart)").click();
    settle(&mut h);
    assert!(h.state().assets.is_empty());
}

#[test]
fn smart_live_annotation_edits_mixed_selection_search_and_membership_gestures() {
    let fixture = Fixture::new();
    let c = open_catalogue(&fixture.path()).unwrap();
    c.catalog()
        .connection()
        .execute_batch("INSERT INTO annotation(asset_id,rating) VALUES(1,5),(2,4);")
        .unwrap();
    let id = c.catalog().insert_collection("Picks").unwrap();
    c.catalog()
        .set_smart_rules(
            id,
            &icatalog_catalog::SmartRules {
                min_rating: Some(4),
                ..Default::default()
            },
        )
        .unwrap();
    let album = c.catalog().insert_collection("Album").unwrap();
    c.catalog().add_to_collection(album, 2).unwrap();
    c.catalog().add_to_collection(album, 3).unwrap();
    for list in [false, true] {
        let mut h = fixture.harness([1100.0, 720.0]);
        if list {
            h.get_by_label("List").click();
            settle(&mut h);
        }
        h.get_by_label("Picks (Smart)").click();
        settle(&mut h);
        assert_eq!(h.state().assets.len(), 2);
        h.get_by_label("Album").click_modifiers(egui::Modifiers {
            ctrl: true,
            ..Default::default()
        });
        settle(&mut h);
        assert_eq!(h.state().assets.len(), 3);
        h.get_by_label("Search name or path").click();
        h.run_steps(2);
        h.get_by_label("Search name or path").type_text("Blue");
        h.key_press(egui::Key::Enter);
        settle(&mut h);
        assert_eq!(h.state().assets.len(), 1);
        h.get_by_label("x").click();
        settle(&mut h);
        h.get_by_label("All").click();
        settle(&mut h);
        let target = h.get_by_label("Picks (Smart)").rect().center();
        drag_to(&mut h, "Green.bmp", target);
        h.drop_at(target);
        settle(&mut h);
        assert_eq!(c.catalog().collection_assets(id).unwrap(), vec![1, 2]);
        open_asset_menu(&mut h, "Green.bmp");
        open_collection_submenu(&mut h);
        assert!(h
            .query(
                By::new()
                    .label("Picks")
                    .role(egui::accesskit::Role::CheckBox)
            )
            .is_none());
        h.key_press(egui::Key::Escape);
        h.key_press(egui::Key::Escape);
        settle(&mut h);
    }
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("Picks (Smart)").click();
    settle(&mut h);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    h.get_by_label("Rating 0").click();
    settle(&mut h);
    assert_eq!(
        h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![2]
    );
    assert_eq!(h.state().collection_counts.get(&id), Some(&1));
    assert!(h.state().selection.primary.is_none());
    assert_eq!(c.catalog().asset(1).unwrap().unwrap().folder_id, Some(1));
}

#[test]
fn smart_save_rules_failure_retry_and_busy_controls() {
    let fixture = Fixture::new();
    let c = open_catalogue(&fixture.path()).unwrap();
    let id = c.catalog().insert_collection("Picks").unwrap();
    c.catalog()
        .set_smart_rules(
            id,
            &icatalog_catalog::SmartRules {
                format: Some("BMP".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("Picks (Smart)").click_secondary();
    settle(&mut h);
    h.get_by_label("Edit Rules…").click();
    settle(&mut h);
    h.get_by_label("Camera Contains").click();
    h.run_steps(2);
    h.get_by_label("Camera Contains").type_text("Missing");
    h.run_steps(2);
    c.catalog().connection().execute_batch("CREATE TRIGGER reject_rules BEFORE UPDATE ON smart_collection BEGIN SELECT RAISE(ABORT,'synthetic rules failure'); END;").unwrap();
    h.get_by_label("Save Rules").click();
    settle(&mut h);
    assert!(h
        .state()
        .create_collection
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("synthetic rules failure"));
    assert!(c
        .catalog()
        .smart_rules(id)
        .unwrap()
        .unwrap()
        .camera
        .is_none());
    c.catalog()
        .connection()
        .execute_batch("DROP TRIGGER reject_rules; BEGIN EXCLUSIVE")
        .unwrap();
    h.get_by_label("Save Rules").click();
    h.run_steps(3);
    assert!(h.get_by_label("Cancel").accesskit_node().is_disabled());
    assert!(h.get_by_label("Save Rules").accesskit_node().is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().create_collection.is_some());
    c.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    assert!(h.state().create_collection.is_none());
    assert_eq!(h.state().collection_counts.get(&id), Some(&0));
    let c = open_catalogue(&fixture.path()).unwrap();
    assert_eq!(
        c.catalog()
            .smart_rules(id)
            .unwrap()
            .unwrap()
            .camera
            .as_deref(),
        Some("Missing")
    );
}
