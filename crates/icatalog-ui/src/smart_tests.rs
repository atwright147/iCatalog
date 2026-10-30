//! Real widgets using the same fixtures and worker as the desktop app.
use super::*;

fn replace_rule(h: &mut Harness<'_, App>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.key_press(egui::Key::Backspace);
    h.get_by_label(label).type_text(text);
    h.run_steps(2);
}

#[test]
fn selected_smart_double_click_keeps_selection_stable_between_clicks() {
    for list in [false, true] {
        let fixture = Fixture::new();
        let c = open_catalogue(&fixture.path()).unwrap();
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
        c.catalog()
            .connection()
            .execute_batch("INSERT INTO annotation(asset_id,rating) VALUES(1,5),(2,4)")
            .unwrap();
        let mut h = double_click_harness(&fixture);
        if list {
            h.get_by_label("List").click();
            settle(&mut h);
        }
        h.get_by_label("Picks (Smart)").click();
        settle(&mut h);
        h.get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
        settle(&mut h);
        h.run_steps(40);
        h.get_by_label("Picks (Smart)").click();
        h.run_steps(2);
        assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![id]));
        assert_eq!(h.state().assets.len(), 2);
        assert_eq!(h.state().selection.primary, Some(1));
        h.get_by_label("Picks (Smart)").click();
        settle(&mut h);
        assert_eq!(
            h.state().create_collection.as_ref().map(|d| d.editing_id),
            Some(Some(id))
        );
        assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![id]));
        assert_eq!(h.state().selection.primary, Some(1));
        h.get_by_label("Cancel").click();
        settle(&mut h);
        h.run_steps(40);
        assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![id]));
        // A genuine single click still returns to All after the click window.
        h.get_by_label("Picks (Smart)").click();
        h.run_steps(2);
        assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![id]));
        settle(&mut h);
        assert_eq!(h.state().folder_sel, FolderSelection::All);
        h.run_steps(40);
        h.get_by_label("Picks (Smart)").click();
        settle(&mut h);
        h.run_steps(40);
        h.get_by_label("Picks (Smart)").click();
        h.run_steps(2);
        h.get_by_label("All").click();
        settle(&mut h);
        h.run_steps(40);
        assert_eq!(h.state().folder_sel, FolderSelection::All);
        h.get_by_label("Picks (Smart)").click();
        settle(&mut h);
        h.run_steps(40);
        h.get_by_label("Picks (Smart)")
            .click_modifiers(egui::Modifiers {
                ctrl: true,
                ..Default::default()
            });
        h.run_steps(2);
        assert_eq!(h.state().folder_sel, FolderSelection::All);
        assert!(h.state().pending_collection_toggle.is_none());
        h.run_steps(40);
        h.get_by_label("Picks (Smart)").click();
        settle(&mut h);
        h.run_steps(40);
        h.get_by_label("Picks (Smart)").click();
        h.run_steps(2);
        h.key_press(egui::Key::Escape);
        settle(&mut h);
        assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![id]));
        assert!(h.state().pending_collection_toggle.is_none());
    }
}

#[test]
fn smart_double_click_honors_a_slower_native_interval() {
    let fixture = Fixture::new();
    let c = open_catalogue(&fixture.path()).unwrap();
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
    let mut h = double_click_harness(&fixture);
    interaction::set_double_click_interval(&h.ctx, 0.6);
    h.get_by_label("Picks (Smart)").click();
    h.run_steps(24); // 400 ms: outside egui's 300 ms default, within the native preference.
    assert!(h.state().create_collection.is_none());
    h.get_by_label("Picks (Smart)").click();
    settle(&mut h);
    assert_eq!(
        h.state().create_collection.as_ref().map(|d| d.editing_id),
        Some(Some(id))
    );
}

#[test]
fn double_click_opens_editor_when_press_and_release_share_a_native_frame() {
    let fixture = Fixture::new();
    let c = open_catalogue(&fixture.path()).unwrap();
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
    let mut h = double_click_harness(&fixture);
    let pos = h.get_by_label("Picks (Smart)").rect().center();
    for _ in 0..2 {
        h.input_mut().events.extend([
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        h.step();
        h.run_steps(2);
    }
    settle(&mut h);
    assert_eq!(
        h.state().create_collection.as_ref().map(|d| d.editing_id),
        Some(Some(id))
    );
}

#[test]
fn smart_double_click_edits_name_and_rules_in_grid_and_list() {
    for list in [false, true] {
        let fixture = Fixture::new();
        let c = open_catalogue(&fixture.path()).unwrap();
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
        c.catalog().insert_collection("Album").unwrap();
        c.catalog()
            .connection()
            .execute_batch("INSERT INTO annotation(asset_id,rating) VALUES(1,5),(2,4)")
            .unwrap();
        let mut h = double_click_harness(&fixture);
        if list {
            h.get_by_label("List").click();
            settle(&mut h);
        }
        double_click_collection(&mut h, "Album");
        assert_eq!(
            h.state().create_collection.as_ref().unwrap().rename_id,
            Some(2)
        );
        h.get_by_label("Cancel").click();
        settle(&mut h);
        double_click_collection(&mut h, "Picks (Smart)");
        assert_eq!(
            h.state().create_collection.as_ref().unwrap().editing_id,
            Some(id)
        );
        assert!(!h.get_by_label("Name").accesskit_node().is_disabled());
        assert_eq!(h.get_by_label("Name").value().as_deref(), Some("Picks"));
        assert!(matches!(&h.state().folder_sel, FolderSelection::Collections(ids) if ids == &[id]));
        replace_rule(&mut h, "Name", "Favourites");
        replace_rule(&mut h, "Minimum Rating", "5");
        settle(&mut h);
        assert!(h.query_by_label("1 matching item").is_some());
        if list {
            h.get_by_label("Name").click();
            h.run_steps(2);
            h.key_press(egui::Key::Enter);
        } else {
            h.get_by_label("Save").click();
        }
        settle(&mut h);
        assert!(h.state().create_collection.is_none());
        assert!(h.query_by_label("Favourites (Smart)").is_some());
        assert!(matches!(&h.state().folder_sel, FolderSelection::Collections(ids) if ids == &[id]));
        assert_eq!(h.state().collection_counts.get(&id), Some(&1));
        assert_eq!(
            h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![1]
        );
        let reopened = open_catalogue(&fixture.path()).unwrap();
        let saved = reopened.catalog().collection(id).unwrap().unwrap();
        assert_eq!(saved.name, "Favourites");
        assert_eq!(saved.rules.unwrap().min_rating, Some(5));
        // The existing context-menu path opens the same editable-name form.
        h.get_by_label("Favourites (Smart)").click_secondary();
        settle(&mut h);
        h.get_by_label("Edit Rules…").click();
        settle(&mut h);
        replace_rule(&mut h, "Name", "Discarded");
        h.get_by_label("Cancel").click();
        settle(&mut h);
        assert_eq!(
            reopened.catalog().collection(id).unwrap().unwrap().name,
            "Favourites"
        );
        c.catalog()
            .connection()
            .execute_batch("BEGIN EXCLUSIVE")
            .unwrap();
        let mut params = Params::new();
        params.set("id", "1").set("rating", "4");
        h.state_mut().run_command("edit", &params);
        h.run_steps(40);
        h.get_by_label("Favourites (Smart)").click();
        h.run_steps(2);
        h.get_by_label("Favourites (Smart)").click();
        h.run_steps(3);
        assert!(h.state().worker.is_some());
        assert!(h.state().create_collection.is_none());
        c.catalog().connection().execute_batch("COMMIT").unwrap();
        settle(&mut h);
    }
}

#[test]
fn unsaved_rules_preview_updates_counts_summary_and_validation_without_writes() {
    let fixture = Fixture::new();
    let c = open_catalogue(&fixture.path()).unwrap();
    c.catalog()
        .connection()
        .execute_batch("INSERT INTO annotation(asset_id,rating) VALUES(1,5),(2,4);")
        .unwrap();
    let mut h = fixture.harness([800.0, 600.0]);
    h.get_by_label("New Collection…").click();
    settle(&mut h);
    h.get_by_label("Name").type_text("Preview");
    h.run_steps(2);
    h.get_by_label("Smart Collection").click();
    settle(&mut h);
    assert!(h.query_by_label("Preview unavailable").is_some());
    replace_rule(&mut h, "Minimum Rating", "4");
    settle(&mut h);
    assert!(h.query_by_label("2 matching items").is_some());
    assert!(h.query_by_label("Rating is at least 4 out of 5").is_some());
    replace_rule(&mut h, "Camera Contains", "Sony");
    settle(&mut h);
    assert!(h.query_by_label("0 matching items").is_some());
    assert!(h
        .query_by_label("Camera contains \"Sony\" AND Rating is at least 4 out of 5")
        .is_some());
    replace_rule(&mut h, "Camera Contains", "");
    replace_rule(&mut h, "Minimum Rating", "6");
    settle(&mut h);
    assert!(h.query_by_label("Preview unavailable").is_some());
    assert!(h.query_by_label("0 matching items").is_none());
    replace_rule(&mut h, "Minimum Rating", "5");
    settle(&mut h);
    assert!(h.query_by_label("1 matching item").is_some());
    replace_rule(&mut h, "Camera Contains", &"Long camera name ".repeat(50));
    settle(&mut h);
    for label in [
        "Minimum Rating",
        "Filename or Path Contains",
        "Cancel",
        "Create",
    ] {
        let rect = h.get_by_label(label).rect();
        assert!(
            rect.top() >= 0.0 && rect.bottom() <= 600.0,
            "{label}: {rect:?}"
        );
    }
    assert!(c.catalog().all_collections().unwrap().is_empty());
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert!(c.catalog().all_collections().unwrap().is_empty());
}

#[test]
fn preview_discards_an_older_in_flight_query_when_rules_change() {
    let fixture = Fixture::new();
    let c = open_catalogue(&fixture.path()).unwrap();
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
    c.catalog()
        .connection()
        .execute_batch("INSERT INTO annotation(asset_id,rating) VALUES(1,5),(2,4)")
        .unwrap();
    let mut h = fixture.harness([800.0, 600.0]);
    h.get_by_label("Picks (Smart)").click_secondary();
    settle(&mut h);
    h.get_by_label("Edit Rules…").click();
    settle(&mut h);
    assert!(h.query_by_label("2 matching items").is_some());
    c.catalog()
        .connection()
        .execute_batch("BEGIN EXCLUSIVE")
        .unwrap();
    replace_rule(&mut h, "Minimum Rating", "5");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !h
        .state()
        .create_collection
        .as_ref()
        .unwrap()
        .preview
        .is_running()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
        h.step();
    }
    replace_rule(&mut h, "Minimum Rating", "4");
    assert!(h.query_by_label("Checking matches…").is_some());
    c.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    assert!(h.query_by_label("2 matching items").is_some());
    assert!(h.query_by_label("1 matching item").is_none());
    assert_eq!(
        c.catalog().smart_rules(id).unwrap().unwrap().min_rating,
        Some(4)
    );
}

#[test]
fn smart_minimum_rating_can_be_entered_and_saved() {
    let fixture = Fixture::new();
    let c = open_catalogue(&fixture.path()).unwrap();
    c.catalog()
        .connection()
        .execute_batch("INSERT INTO annotation(asset_id,rating) VALUES(1,5),(2,4);")
        .unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    h.get_by_label("New Collection…").click();
    settle(&mut h);
    h.get_by_label("Name").type_text("Picks");
    h.run_steps(2);
    h.get_by_label("Smart Collection").click();
    settle(&mut h);
    h.run_steps(10);
    h.get_by_label("Minimum Rating").click();
    h.run_steps(2);
    h.get_by_label("Minimum Rating").type_text("4");
    h.run_steps(2);
    h.get_by_label("Create").click();
    settle(&mut h);
    assert!(h.state().create_collection.is_none());
    assert_eq!(
        c.catalog().smart_rules(1).unwrap().unwrap().min_rating,
        Some(4)
    );
    assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![1, 2]);
    h.get_by_label("Picks (Smart)").click();
    settle(&mut h);
    assert_eq!(
        h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(h.state().collection_counts.get(&1), Some(&2));
    h.get_by_label("Picks (Smart)").click_secondary();
    settle(&mut h);
    h.get_by_label("Edit Rules…").click();
    settle(&mut h);
    assert!(h.query_by_label("2 matching items").is_some());
    h.get_by_label("Minimum Rating").click();
    h.run_steps(2);
    h.key_press(egui::Key::Home);
    h.key_press(egui::Key::Delete);
    h.get_by_label("Minimum Rating").type_text("5");
    h.run_steps(2);
    assert_eq!(
        h.get_by_label("Minimum Rating").value().as_deref(),
        Some("5")
    );
    h.get_by_label("Save").click();
    settle(&mut h);
    assert!(h.state().create_collection.is_none());
    assert_eq!(
        c.catalog().smart_rules(1).unwrap().unwrap().min_rating,
        Some(5)
    );
    assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![1]);
    assert_eq!(
        h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(h.state().collection_counts.get(&1), Some(&1));
}

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
    assert!(!h.get_by_label("Name").accesskit_node().is_disabled());
    h.get_by_label("Captured From").click();
    h.run_steps(2);
    h.get_by_label("Captured From").type_text("2023-02-29");
    h.run_steps(2);
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
    replace_rule(&mut h, "Name", "");
    assert!(h.get_by_label("Save").accesskit_node().is_disabled());
    replace_rule(&mut h, "Name", &"🦀".repeat(300));
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
        .contains("1024"));
    replace_rule(&mut h, "Name", "Renamed picks");
    h.get_by_label("Camera Contains").click();
    h.run_steps(2);
    h.get_by_label("Camera Contains").type_text("Missing");
    h.run_steps(2);
    c.catalog().connection().execute_batch("CREATE TRIGGER reject_rules BEFORE UPDATE ON smart_collection BEGIN SELECT RAISE(ABORT,'synthetic rules failure'); END;").unwrap();
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
        .contains("synthetic rules failure"));
    assert!(c
        .catalog()
        .smart_rules(id)
        .unwrap()
        .unwrap()
        .camera
        .is_none());
    assert_eq!(c.catalog().collection(id).unwrap().unwrap().name, "Picks");
    assert_eq!(
        h.state().create_collection.as_ref().unwrap().name,
        "Renamed picks"
    );
    c.catalog()
        .connection()
        .execute_batch("DROP TRIGGER reject_rules; BEGIN EXCLUSIVE")
        .unwrap();
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert!(h.get_by_label("Name").accesskit_node().is_disabled());
    assert!(h.get_by_label("Cancel").accesskit_node().is_disabled());
    assert!(h.get_by_label("Save").accesskit_node().is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().create_collection.is_some());
    c.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    assert!(h.state().create_collection.is_none());
    assert_eq!(
        c.catalog().collection(id).unwrap().unwrap().name,
        "Renamed picks"
    );
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
