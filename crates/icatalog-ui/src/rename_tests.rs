use super::*;

fn setup() -> Fixture {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let manual = ctx.catalog().insert_collection("Album").unwrap();
    ctx.catalog().add_to_collection(manual, 1).unwrap();
    ctx.catalog().add_to_collection(manual, 2).unwrap();
    let smart = ctx.catalog().insert_collection("Picks").unwrap();
    ctx.catalog()
        .set_smart_rules(
            smart,
            &icatalog_catalog::SmartRules {
                text: Some("Red".into()),
                ..Default::default()
            },
        )
        .unwrap();
    fixture
}

fn open_rename(h: &mut Harness<'_, App>, label: &str) {
    h.get_by_label(label).click_secondary();
    settle(h);
    h.get_by_label("Rename…").click();
    settle(h);
    assert!(h
        .state()
        .create_collection
        .as_ref()
        .unwrap()
        .rename_id
        .is_some());
    assert!(h.query_by_label("Smart Collection").is_none());
}

#[test]
fn double_click_manual_collection_renames_without_selection_flicker() {
    for list in [false, true] {
        let fixture = setup();
        let ctx = open_catalogue(&fixture.path()).unwrap();
        let mut h = double_click_harness(&fixture);
        if list {
            h.get_by_label("List").click();
            settle(&mut h);
        }
        h.get_by_label("Album").click();
        settle(&mut h);
        h.get(
            By::new()
                .label("Red.bmp")
                .role(egui::accesskit::Role::Button),
        )
        .click();
        settle(&mut h);
        h.run_steps(40);
        h.get_by_label("Album").click();
        h.run_steps(2);
        assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![1]));
        assert_eq!(h.state().selection.primary, Some(1));
        h.get_by_label("Album").click();
        settle(&mut h);
        assert_eq!(
            h.state().create_collection.as_ref().unwrap().rename_id,
            Some(1)
        );
        assert_eq!(h.get_by_label("Name").value().as_deref(), Some("Album"));
        assert!(h.query_by_label("Smart Collection").is_none());
        h.get_by_label("Name").type_text("Renamed album");
        h.run_steps(2);
        if list {
            h.key_press(egui::Key::Enter);
        } else {
            h.get_by_label("Rename").click();
        }
        settle(&mut h);
        assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![1]));
        assert_eq!(h.state().selection.primary, Some(1));
        assert_eq!(h.state().collection_counts.get(&1), Some(&2));
        let reopened = open_catalogue(&fixture.path()).unwrap();
        assert_eq!(
            reopened.catalog().collection(1).unwrap().unwrap().name,
            "Renamed album"
        );
        assert_eq!(reopened.catalog().collection_assets(1).unwrap(), vec![1, 2]);
        double_click_collection(&mut h, "Renamed album");
        h.get_by_label("Name").type_text("Discarded");
        h.run_steps(2);
        if list {
            h.key_press(egui::Key::Escape);
        } else {
            h.get_by_label("Cancel").click();
        }
        settle(&mut h);
        h.run_steps(40);
        assert_eq!(h.state().folder_sel, FolderSelection::Collections(vec![1]));
        assert_eq!(
            reopened.catalog().collection(1).unwrap().unwrap().name,
            "Renamed album"
        );
        ctx.catalog()
            .connection()
            .execute_batch("BEGIN EXCLUSIVE")
            .unwrap();
        let mut params = Params::new();
        params.set("id", "1").set("rating", "4");
        h.state_mut().run_command("edit", &params);
        h.run_steps(40);
        h.get_by_label("Renamed album").click();
        h.run_steps(2);
        h.get_by_label("Renamed album").click();
        h.run_steps(3);
        assert!(h.state().worker.is_some());
        assert!(h.state().create_collection.is_none());
        ctx.catalog().connection().execute_batch("COMMIT").unwrap();
        settle(&mut h);
    }
}

#[test]
fn rename_manual_and_smart_preserves_selection_counts_memberships_rules_and_reopen() {
    for list in [false, true] {
        let fixture = setup();
        let mut h = fixture.harness([1100.0, 720.0]);
        if list {
            h.get_by_label("List").click();
            settle(&mut h);
        }
        h.get_by_label("Album").click();
        settle(&mut h);
        h.get_by_label("Picks (Smart)")
            .click_modifiers(egui::Modifiers {
                ctrl: true,
                ..Default::default()
            });
        settle(&mut h);
        let original_selection = match &h.state().folder_sel {
            FolderSelection::Collections(ids) => ids.clone(),
            _ => panic!("collection selection expected"),
        };
        for (id, old, new, enter) in [
            (1, "Album", "Z Renamed", false),
            (2, "Picks (Smart)", "A Renamed", true),
        ] {
            open_rename(&mut h, old);
            h.get_by_label("Name").type_text(new);
            h.run_steps(2);
            if enter {
                h.key_press(egui::Key::Enter);
            } else {
                h.get_by_label("Rename").click();
            }
            settle(&mut h);
            assert!(h.state().create_collection.is_none());
            assert!(
                matches!(&h.state().folder_sel,FolderSelection::Collections(ids) if *ids==original_selection)
            );
            assert_eq!(
                h.state().assets.iter().map(|a| a.id).collect::<Vec<_>>(),
                vec![1, 2]
            );
            assert_eq!(
                h.state().collection_counts.get(&id),
                Some(&if id == 1 { 2 } else { 1 })
            );
        }
        let ctx = open_catalogue(&fixture.path()).unwrap();
        assert_eq!(
            ctx.catalog().collection(1).unwrap().unwrap().name,
            "Z Renamed"
        );
        assert_eq!(
            ctx.catalog().collection(2).unwrap().unwrap().name,
            "A Renamed"
        );
        assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1, 2]);
        assert_eq!(
            ctx.catalog()
                .smart_rules(2)
                .unwrap()
                .unwrap()
                .text
                .as_deref(),
            Some("Red")
        );
        assert_eq!(ctx.catalog().collection_assets(2).unwrap(), vec![1]);
        assert_eq!(
            ctx.catalog()
                .all_collections()
                .unwrap()
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        open_asset_menu(&mut h, "Red.bmp");
        open_collection_submenu(&mut h);
        assert!(h
            .query(
                By::new()
                    .label("Z Renamed")
                    .role(egui::accesskit::Role::CheckBox)
            )
            .is_some());
        assert!(h.query_by_label("Album").is_none());
    }
}

#[test]
fn rename_cancel_escape_and_blank_input_do_not_write() {
    let fixture = setup();
    let mut h = fixture.harness([800.0, 600.0]);
    for (row, dismissal) in [
        ("Album", "cancel"),
        ("Picks (Smart)", "escape"),
        ("Album", "backdrop"),
    ] {
        open_rename(&mut h, row);
        h.key_press(egui::Key::Backspace);
        h.run_steps(2);
        assert!(h.get_by_label("Rename").accesskit_node().is_disabled());
        h.get_by_label("Name").type_text("Discard me");
        h.run_steps(2);
        match dismissal {
            "cancel" => h.get_by_label("Cancel").click(),
            "escape" => h.key_press(egui::Key::Escape),
            _ => h.get_by_label("All").click(),
        };
        settle(&mut h);
        assert!(h.state().create_collection.is_none());
        let c = open_catalogue(&fixture.path()).unwrap();
        assert_eq!(c.catalog().collection(1).unwrap().unwrap().name, "Album");
        assert_eq!(c.catalog().collection(2).unwrap().unwrap().name, "Picks");
    }
}

#[test]
fn rename_invalid_text_failed_save_retry_and_pending_controls() {
    let fixture = setup();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let mut h = fixture.harness([1100.0, 720.0]);
    open_rename(&mut h, "Picks (Smart)");
    h.get_by_label("Name").type_text(&"🦀".repeat(300));
    h.run_steps(2);
    h.get_by_label("Rename").click();
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
    h.get_by_label("Name").type_text("Retry name");
    h.run_steps(2);
    ctx.catalog().connection().execute_batch("CREATE TRIGGER reject_rename AFTER UPDATE OF name ON collection BEGIN SELECT RAISE(ABORT,'synthetic rename failure'); END;").unwrap();
    h.get_by_label("Rename").click();
    settle(&mut h);
    let dialog = h.state().create_collection.as_ref().unwrap();
    assert_eq!(dialog.name, "Retry name");
    assert!(dialog
        .error
        .as_ref()
        .unwrap()
        .contains("synthetic rename failure"));
    assert_eq!(ctx.catalog().collection(2).unwrap().unwrap().name, "Picks");
    ctx.catalog()
        .connection()
        .execute_batch("DROP TRIGGER reject_rename; BEGIN EXCLUSIVE")
        .unwrap();
    h.get_by_label("Rename").click();
    h.run_steps(3);
    for label in ["Name", "Cancel", "Rename"] {
        assert!(h.get_by_label(label).accesskit_node().is_disabled());
    }
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().create_collection.is_some());
    ctx.catalog().connection().execute_batch("COMMIT").unwrap();
    settle(&mut h);
    assert!(h.state().create_collection.is_none());
    assert_eq!(
        ctx.catalog().collection(2).unwrap().unwrap().name,
        "Retry name"
    );
    assert_eq!(ctx.catalog().collection_assets(2).unwrap(), vec![1]);
}
