//! Shared star widgets, annotation commands and optional Smart rules.
use super::*;

#[test]
fn five_star_editor_supports_pointer_keyboard_clear_and_disabled_state() {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for scale in [1.0, 1.5, 2.0] {
            let mut h = Harness::builder()
                .with_size([300.0, 100.0])
                .with_theme(theme)
                .with_pixels_per_point(scale)
                .build_ui_state(
                    |ui, state: &mut (Option<i64>, bool)| {
                        typography::configure(ui.ctx());
                        ui.add_enabled_ui(state.1, |ui| {
                            rating::editor(ui, &mut state.0, "Test Rating");
                        });
                    },
                    (None, true),
                );
            h.run();
            let group = h.get(
                By::new()
                    .label("Test Rating")
                    .role(egui::accesskit::Role::Group),
            );
            assert_eq!(
                group
                    .query_all(By::new().role(egui::accesskit::Role::Button))
                    .count(),
                5
            );
            let mut last = 0.0;
            for star in 1..=5 {
                let rect = h.get_by_label(&format!("Test Rating {star}")).rect();
                assert!(rect.width() >= 24.0 && rect.height() >= 24.0);
                assert!(rect.left() >= last && rect.right() < 300.0);
                last = rect.right();
            }
            h.get_by_label("Test Rating 4").click();
            h.run();
            assert_eq!(h.state().0, Some(4));
            assert_eq!(
                h.get_by_label("Test Rating").value().as_deref(),
                Some("4 out of 5")
            );
            h.get_by_label("Test Rating 4").click();
            h.run();
            assert_eq!(h.state().0, None);
            h.get_by_label("Test Rating 2").focus();
            h.run();
            for (key, expected) in [
                (egui::Key::Num3, Some(3)),
                (egui::Key::ArrowRight, Some(4)),
                (egui::Key::End, Some(5)),
                (egui::Key::ArrowUp, Some(5)),
                (egui::Key::Home, Some(0)),
                (egui::Key::ArrowLeft, Some(0)),
                (egui::Key::Num5, Some(5)),
                (egui::Key::Backspace, None),
                (egui::Key::Num0, Some(0)),
                (egui::Key::Delete, None),
            ] {
                h.key_press(key);
                h.run();
                assert_eq!(h.state().0, expected, "{theme:?} {scale}: {key:?}");
            }
            h.state_mut().1 = false;
            h.run();
            assert!(h
                .get_by_label("Test Rating 5")
                .accesskit_node()
                .is_disabled());
            h.get_by_label("Test Rating 5").click();
            h.key_press(egui::Key::Num5);
            h.run();
            assert_eq!(h.state().0, None);
        }
    }
}

#[test]
fn read_only_ratings_always_have_five_stars_and_no_edit_buttons() {
    for (value, expected) in [
        (i64::MIN, "☆☆☆☆☆"),
        (0, "☆☆☆☆☆"),
        (3, "★★★☆☆"),
        (5, "★★★★★"),
        (i64::MAX, "★★★★★"),
    ] {
        let mut h = Harness::builder().build_ui(|ui| {
            rating::display(ui, value, 14.0, ui.visuals().text_color());
        });
        h.run();
        assert!(h.query_by_label(&format!("Rating: {expected}")).is_some());
        assert_eq!(
            h.query_all(By::new().role(egui::accesskit::Role::Button))
                .count(),
            0
        );
    }
}

#[test]
fn inspector_stars_save_every_rating_and_clear_to_unrated() {
    let fixture = Fixture::new();
    let mut h = fixture.harness([800.0, 600.0]);
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    let ctx = open_catalogue(&fixture.path()).unwrap();
    for value in 1..=5 {
        h.get_by_label(&format!("Rating {value}")).click();
        settle(&mut h);
        assert_eq!(ctx.catalog().annotation(1).unwrap().unwrap().rating, value);
    }
    h.get_by_label("Rating 5").click();
    settle(&mut h);
    assert_eq!(ctx.catalog().annotation(1).unwrap().unwrap().rating, 0);
    h.get_by_label("List").click();
    settle(&mut h);
    assert_eq!(h.query_all(By::new().label("Rating: ☆☆☆☆☆")).count(), 3);
}

#[test]
fn smart_rating_stars_preserve_zero_and_clear_a_rule_without_writes_on_cancel() {
    let fixture = Fixture::new();
    let ctx = open_catalogue(&fixture.path()).unwrap();
    let id = ctx.catalog().insert_collection("All bitmaps").unwrap();
    ctx.catalog()
        .set_smart_rules(
            id,
            &icatalog_catalog::SmartRules {
                format: Some("BMP".into()),
                min_rating: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
    let mut h = fixture.harness([800.0, 600.0]);
    let open_rules = |h: &mut Harness<'_, App>| {
        h.get_by_label("All bitmaps (Smart)").click_secondary();
        settle(h);
        h.get_by_label("Edit Rules…").click();
        settle(h);
    };
    open_rules(&mut h);
    assert_eq!(
        h.get_by_label("Minimum Rating").value().as_deref(),
        Some("0 out of 5")
    );
    h.get_by_label("Save").click();
    settle(&mut h);
    assert_eq!(
        ctx.catalog().smart_rules(id).unwrap().unwrap().min_rating,
        Some(0)
    );
    open_rules(&mut h);
    h.get_by_label("Minimum Rating 3").click();
    settle(&mut h);
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert_eq!(
        ctx.catalog().smart_rules(id).unwrap().unwrap().min_rating,
        Some(0)
    );
    open_rules(&mut h);
    h.get_by_label("Minimum Rating 3").click();
    settle(&mut h);
    h.get_by_label("Minimum Rating 3").click();
    settle(&mut h);
    assert_eq!(
        h.get_by_label("Minimum Rating").value().as_deref(),
        Some("Not set")
    );
    assert!(h.query_by_label("3 matching items").is_some());
    h.get_by_label("Save").click();
    settle(&mut h);
    assert_eq!(
        ctx.catalog().smart_rules(id).unwrap().unwrap().min_rating,
        None
    );
    assert_eq!(ctx.catalog().collection_assets(id).unwrap(), vec![1, 2, 3]);
    // Zero is a real minimum rule, distinct from the cleared optional value.
    open_rules(&mut h);
    h.get_by_label("Minimum Rating 1").focus();
    h.run_steps(2);
    h.key_press(egui::Key::Num0);
    settle(&mut h);
    h.get_by_label("Save").click();
    settle(&mut h);
    assert_eq!(
        ctx.catalog().smart_rules(id).unwrap().unwrap().min_rating,
        Some(0)
    );
}
