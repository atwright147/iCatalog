#![allow(clippy::unwrap_used, clippy::panic)]
use icatalog_engine::{dispatch, Context, Outcome, Params};

fn p(pairs: &[(&str, &str)]) -> Params {
    let mut p = Params::new();
    for (k, v) in pairs {
        p.set(*k, *v);
    }
    p
}
fn run(c: &mut Context, pairs: &[(&str, &str)]) -> Outcome {
    dispatch(c, "collection", &p(pairs)).unwrap()
}
fn rows(o: Outcome) -> Vec<Vec<String>> {
    match o {
        Outcome::Table { rows, .. } => rows,
        _ => panic!("table expected"),
    }
}
fn fixture() -> Context {
    let mut c = Context::open(":memory:").unwrap();
    dispatch(&mut c, "init", &Params::new()).unwrap();
    c.catalog().connection().execute_batch("INSERT INTO asset(id,uid,name,path,format) VALUES(1,'one','One.jpg','/one.jpg','JPEG'),(2,'two','Two.nef','/two.nef','NEF'); INSERT INTO annotation(asset_id,rating,label) VALUES(1,5,2);").unwrap();
    c
}

#[test]
fn unsaved_preview_counts_summarizes_and_never_changes_the_catalogue() {
    let mut c = fixture();
    let before = c.catalog().connection().total_changes();
    for (rules, expected) in [
        (
            vec![("min-rating", "5")],
            "1 matching item\nRating is at least 5 out of 5",
        ),
        (
            vec![("min-rating", "0")],
            "2 matching items\nRating is at least 0 out of 5",
        ),
        (
            vec![("camera", "Sony"), ("min-rating", "4")],
            "0 matching items\nCamera contains \"Sony\" AND Rating is at least 4 out of 5",
        ),
        (
            vec![("format", "NEF"), ("text", "Two")],
            "1 matching item\nFormat is \"NEF\" AND Filename or path contains \"Two\"",
        ),
    ] {
        let mut params = p(&rules);
        params.set("action", "preview-smart");
        assert_eq!(
            dispatch(&mut c, "collection", &params).unwrap().render(),
            expected
        );
    }
    for invalid in [
        vec![],
        vec![("min-rating", "6")],
        vec![("min-rating", "NaN")],
        vec![("captured-from", "2026-02-30")],
        vec![("text", "\0")],
        vec![("min-rating", "4"), ("name", "Not saved")],
    ] {
        let mut params = p(&invalid);
        params.set("action", "preview-smart");
        assert!(dispatch(&mut c, "collection", &params).is_err());
    }
    assert!(c.catalog().all_collections().unwrap().is_empty());
    assert_eq!(c.catalog().connection().total_changes(), before);
}

#[test]
fn smart_name_and_rules_save_atomically_and_keep_the_name_when_omitted() {
    let mut c = fixture();
    run(
        &mut c,
        &[
            ("action", "create-smart"),
            ("name", "Picks"),
            ("min-rating", "4"),
        ],
    );
    let original = c.catalog().collection(1).unwrap().unwrap();
    for name in ["", "  ", "bad\nname", &"x".repeat(1025)] {
        assert!(dispatch(
            &mut c,
            "collection",
            &p(&[
                ("action", "set-rules"),
                ("id", "1"),
                ("name", name),
                ("min-rating", "3")
            ])
        )
        .is_err());
        assert_eq!(c.catalog().collection(1).unwrap().unwrap(), original);
    }
    assert!(dispatch(
        &mut c,
        "collection",
        &p(&[
            ("action", "set-rules"),
            ("id", "1"),
            ("name", "Changed"),
            ("min-rating", "6")
        ])
    )
    .is_err());
    for trigger in [
        "CREATE TRIGGER reject_edit BEFORE UPDATE ON smart_collection BEGIN SELECT RAISE(ABORT,'synthetic rules failure'); END;",
        "CREATE TRIGGER reject_edit AFTER UPDATE OF name ON collection BEGIN SELECT RAISE(ABORT,'synthetic name failure'); END;",
    ] {
        c.catalog().connection().execute_batch(trigger).unwrap();
        assert!(dispatch(&mut c, "collection", &p(&[("action", "set-rules"), ("id", "1"), ("name", "Changed"), ("min-rating", "3")])).is_err());
        assert_eq!(c.catalog().collection(1).unwrap().unwrap(), original);
        c.catalog().connection().execute_batch("DROP TRIGGER reject_edit").unwrap();
    }
    run(
        &mut c,
        &[
            ("action", "set-rules"),
            ("id", "1"),
            ("name", "  Favourites  "),
            ("min-rating", "5"),
        ],
    );
    let updated = c.catalog().collection(1).unwrap().unwrap();
    assert_eq!(updated.id, original.id);
    assert_eq!(updated.name, "Favourites");
    assert_eq!(updated.rules.unwrap().min_rating, Some(5));
    assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![1]);
    run(
        &mut c,
        &[("action", "set-rules"), ("id", "1"), ("min-rating", "0")],
    );
    assert_eq!(
        c.catalog().collection(1).unwrap().unwrap().name,
        "Favourites"
    );
}

#[test]
fn commands_browse_edit_rules_and_follow_annotation_changes() {
    let mut c = fixture();
    run(
        &mut c,
        &[
            ("action", "create-smart"),
            ("name", "Picks"),
            ("min-rating", "4"),
            ("format", "jpeg"),
        ],
    );
    let list = rows(run(&mut c, &[("action", "list")]));
    assert_eq!(list[0], vec!["1", "Picks", "smart"]);
    assert_eq!(
        rows(run(&mut c, &[("action", "show"), ("id", "1")])).len(),
        1
    );
    assert_eq!(
        rows(dispatch(&mut c, "list", &p(&[("collection", "1")])).unwrap()).len(),
        1
    );
    assert!(
        rows(dispatch(&mut c, "find", &p(&[("collection", "1"), ("query", "Two")])).unwrap())
            .is_empty()
    );
    assert_eq!(
        rows(run(&mut c, &[("action", "rules"), ("id", "1")])).len(),
        2
    );
    dispatch(&mut c, "edit", &p(&[("id", "1"), ("rating", "2")])).unwrap();
    assert!(rows(run(&mut c, &[("action", "show"), ("id", "1")])).is_empty());
    run(
        &mut c,
        &[("action", "set-rules"), ("id", "1"), ("format", "NEF")],
    );
    assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![2]);
    assert_eq!(
        rows(run(&mut c, &[("action", "rules"), ("id", "1")])),
        vec![vec!["format", "NEF"]]
    );
    assert_eq!(c.catalog().asset(2).unwrap().unwrap().path, "/two.nef");
    assert_eq!(
        c.catalog()
            .connection()
            .query_row("SELECT COUNT(*) FROM collection_asset", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn bad_actions_rules_and_failed_creation_leave_no_partial_collection() {
    let mut c = fixture();
    run(
        &mut c,
        &[
            ("action", "create-smart"),
            ("name", "Picks"),
            ("text", "One"),
        ],
    );
    run(&mut c, &[("action", "create"), ("name", "Manual")]);
    for pairs in [
        vec![("action", "create-smart"), ("name", "Empty")],
        vec![("action", "set-rules"), ("id", "1")],
        vec![("action", "set-rules"), ("id", "2"), ("format", "JPEG")],
        vec![("action", "rules"), ("id", "2")],
        vec![("action", "rules"), ("id", "1"), ("format", "JPEG")],
        vec![("action", "add"), ("id", "1"), ("asset", "1")],
        vec![("action", "remove"), ("id", "1"), ("assets", "1,2")],
        vec![("action", "create"), ("name", "Oops"), ("format", "JPEG")],
    ] {
        assert!(
            dispatch(&mut c, "collection", &p(&pairs)).is_err(),
            "{pairs:?}"
        );
        assert_eq!(c.catalog().all_collections().unwrap().len(), 2);
        assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![1]);
    }
    for (key, bad) in [
        ("format", ""),
        ("text", "\0"),
        ("min-rating", "NaN"),
        ("min-rating", "6"),
        ("label", "8"),
        ("captured-from", "2023-02-29"),
        ("captured-to", "\u{202e}"),
    ] {
        for action in ["create-smart", "set-rules"] {
            let mut params = p(&[("action", action), (key, bad)]);
            if action == "create-smart" {
                params.set("name", "Bad");
            } else {
                params.set("id", "1");
            }
            assert!(dispatch(&mut c, "collection", &params).is_err());
        }
    }
    c.catalog().connection().execute_batch("CREATE TRIGGER fail_smart BEFORE INSERT ON smart_collection BEGIN SELECT RAISE(ABORT,'synthetic smart failure'); END;").unwrap();
    assert!(dispatch(
        &mut c,
        "collection",
        &p(&[
            ("action", "create-smart"),
            ("name", "Rolled back"),
            ("format", "JPEG")
        ])
    )
    .is_err());
    assert_eq!(c.catalog().all_collections().unwrap().len(), 2);
    assert!(c.catalog().connection().is_autocommit());
}

#[test]
fn hostile_smart_rule_parameters_never_unwind() {
    let mut c = fixture();
    for key in [
        "format",
        "camera",
        "lens",
        "keyword",
        "text",
        "captured-from",
        "captured-to",
        "min-rating",
        "label",
    ] {
        for value in [
            "\0".to_owned(),
            "🦀".to_owned(),
            "9".repeat(100_000),
            "x".repeat(1025),
            "-9223372036854775808".to_owned(),
            "' OR 1=1 --".to_owned(),
        ] {
            let params = p(&[
                ("action", "create-smart"),
                ("name", "Adversarial"),
                (key, &value),
            ]);
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(
                    &mut c,
                    "collection",
                    &params
                )))
                .is_ok()
            );
        }
    }
}
