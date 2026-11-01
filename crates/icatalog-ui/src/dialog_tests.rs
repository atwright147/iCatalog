//! Close controls dismiss drafts through the same paths as Cancel/Close.
use super::*;

fn assert_header_close(h: &Harness<'_, App>, title: &str) {
    let heading = h.get_by_label(title).rect();
    let close = h.get_by_label("Close dialog").rect();
    assert!(
        close.left() > heading.right(),
        "{title}: {heading:?} {close:?}"
    );
    assert!((close.center().y - heading.center().y).abs() < 1.0);
    assert!(close.width() >= 24.0 && close.height() >= 24.0);
    assert!(close.top() >= 0.0 && close.right() < 800.0);
}

#[test]
fn header_x_discards_collection_drafts_without_saving_and_allows_reopening() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    ctx.catalog().insert_collection("Album").unwrap();
    let smart = ctx.catalog().insert_collection("Picks").unwrap();
    let rules = icatalog_catalog::SmartRules {
        min_rating: Some(4),
        ..Default::default()
    };
    ctx.catalog().set_smart_rules(smart, &rules).unwrap();
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 2.0] {
            let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
            for kind in ["create", "create-smart", "rename", "rename-smart", "rules"] {
                let title = match kind {
                    "create" | "create-smart" => {
                        h.get_by_label("New Collection…").click();
                        settle(&mut h);
                        "New Collection"
                    }
                    _ => {
                        h.get_by_label(if kind == "rename" {
                            "Album"
                        } else {
                            "Picks (Smart)"
                        })
                        .click_secondary();
                        settle(&mut h);
                        h.get_by_label(if kind == "rules" {
                            "Edit Rules…"
                        } else {
                            "Rename…"
                        })
                        .click();
                        settle(&mut h);
                        if kind == "rules" {
                            "Edit Smart Collection"
                        } else {
                            "Rename Collection"
                        }
                    }
                };
                h.get_by_label("Name").click();
                h.run_steps(2);
                h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
                h.get_by_label("Name").type_text("Unsaved draft");
                h.run_steps(2);
                if kind == "create-smart" {
                    h.get_by_label("Smart Collection").click();
                    settle(&mut h);
                }
                if matches!(kind, "create-smart" | "rules") {
                    h.get_by_label("Format").click();
                    h.run_steps(2);
                    h.get_by_label("Format").type_text("PNG");
                    settle(&mut h);
                    assert_eq!(h.get_by_label("Format").value().as_deref(), Some("PNG"));
                }
                assert_header_close(&h, title);
                if theme == egui::Theme::Light {
                    h.get_by_label("Close dialog").click_accesskit();
                } else {
                    h.get_by_label("Close dialog").click();
                }
                settle(&mut h);
                assert!(h.state().create_collection.is_none());
                assert!(h.state().worker.is_none());
                assert!(!h
                    .get_by_label("New Collection…")
                    .accesskit_node()
                    .is_disabled());
                assert_eq!(ctx.catalog().all_collections().unwrap().len(), 2);
                assert_eq!(ctx.catalog().collection(1).unwrap().unwrap().name, "Album");
                assert_eq!(
                    ctx.catalog().collection(smart).unwrap().unwrap().name,
                    "Picks"
                );
                assert_eq!(
                    ctx.catalog().smart_rules(smart).unwrap(),
                    Some(rules.clone())
                );
            }
        }
    }
}

#[test]
fn header_x_closes_connect_and_sync_with_drafts_errors_and_results() {
    let fixture = Fixture::new();
    for (theme, scale) in [(egui::Theme::Dark, 1.0), (egui::Theme::Light, 2.0)] {
        let mut h = fixture.harness_with_display([800.0, 600.0], scale, theme);
        for clone in [true, false] {
            for state in ["draft", "error", "result"] {
                h.state_mut().open_sync_dialog(clone);
                {
                    let dialog = h.state_mut().sync_dialog.as_mut().unwrap();
                    dialog.url = "http://nas:8080".into();
                    if state == "error" {
                        dialog.error = Some("Synthetic error".into());
                    }
                    if state == "result" {
                        dialog.result = Some("Completed operation".into());
                    }
                }
                settle(&mut h);
                assert_header_close(
                    &h,
                    if clone {
                        "Connect to Server"
                    } else {
                        "Sync catalogue"
                    },
                );
                h.get_by_label("Close dialog").click();
                settle(&mut h);
                assert!(h.state().sync_dialog.is_none());
                assert!(h.state().worker.is_none());
                assert_eq!(h.state().path, fixture.path());
                assert_eq!(h.state().assets.len(), 3);
            }
        }
    }
    assert!(!fixture.dir.path().join("fixture.sqlite.sync-url").exists());
}
