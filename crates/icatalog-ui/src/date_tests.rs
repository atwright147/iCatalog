//! Regional date display through the real inspector and List widgets.
use super::*;

#[test]
fn inspector_and_list_share_regional_dates_and_keep_camera_time() {
    let fixture = Fixture::new();
    seed_list_metadata(&fixture);
    for (locale, offset, captured, modified) in [
        (
            chrono::Locale::en_GB,
            0,
            "28/02/2000 at 14:30:00",
            "29/02/2000 at 00:00:00",
        ),
        (
            chrono::Locale::en_US,
            -18_000,
            "02/28/2000 at 02:30:00 PM",
            "02/28/2000 at 07:00:00 PM",
        ),
        (
            chrono::Locale::de_DE,
            3600,
            "28.02.2000 at 14:30:00",
            "29.02.2000 at 01:00:00",
        ),
    ] {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            for scale in [1.0, 2.0] {
                let mut h = fixture.harness_with_display([1100.0, 720.0], scale, theme);
                h.state_mut().dates = dates::DateFormatter::fixed(locale, offset);
                h.get(
                    By::new()
                        .label("Red.bmp")
                        .role(egui::accesskit::Role::Button),
                )
                .click();
                settle(&mut h);
                for list in [false, true] {
                    if list {
                        h.get_by_label("List").click();
                        settle(&mut h);
                    }
                    for text in [
                        format!("Captured: {captured}"),
                        format!("Modified: {modified}"),
                    ] {
                        let rect = h.get_by_label(&text).rect();
                        assert!(rect.height() >= 14.0);
                        assert!(rect.top() >= 0.0 && rect.bottom() < 720.0);
                    }
                }
            }
        }
    }
    let ctx = open_catalogue(&fixture.path()).unwrap();
    assert_eq!(
        ctx.catalog().asset(1).unwrap().unwrap().mtime,
        Some(951_782_400)
    );
    assert_eq!(
        ctx.catalog()
            .asset_meta(1)
            .unwrap()
            .unwrap()
            .captured_local
            .as_deref(),
        Some("2000-02-28T14:30:00")
    );
}

#[test]
fn real_inspector_uses_system_preferences_when_no_test_settings_are_injected() {
    let fixture = Fixture::new();
    seed_list_metadata(&fixture);
    let mut h = fixture.harness([1100.0, 720.0]);
    h.state_mut().dates = dates::DateFormatter::system();
    h.get(
        By::new()
            .label("Red.bmp")
            .role(egui::accesskit::Role::Button),
    )
    .click();
    settle(&mut h);
    let dates = dates::DateFormatter::system();
    for label in [
        format!("Modified: {}", dates.modified(Some(951_782_400))),
        format!("Captured: {}", dates.captured(Some("2000-02-28T14:30:00"))),
    ] {
        assert!(h.query_by_label(&label).is_some(), "{label}");
    }
}
