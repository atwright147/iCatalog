#![allow(clippy::unwrap_used, clippy::panic)]
use icatalog_engine::{dispatch, Context, EngineError, Outcome, Params};

fn params(pairs: &[(&str, &str)]) -> Params {
    let mut params = Params::new();
    for (key, value) in pairs {
        params.set(*key, *value);
    }
    params
}

fn run(ctx: &mut Context, pairs: &[(&str, &str)]) -> Outcome {
    dispatch(ctx, "collection", &params(pairs)).unwrap()
}

fn rows(outcome: Outcome) -> Vec<Vec<String>> {
    match outcome {
        Outcome::Table { rows, .. } => rows,
        Outcome::Text(text) => panic!("expected table, got {text}"),
    }
}

fn init(ctx: &mut Context) {
    dispatch(ctx, "init", &Params::new()).unwrap();
    ctx.catalog()
        .connection()
        .execute_batch(
            "INSERT INTO folder(id,name) VALUES (1,'Original');
         INSERT INTO asset(id,uid,name,path,folder_id) VALUES
         (1,'one','One.jpg','/synthetic/One.jpg',1),
         (2,'two','Two.jpg','/synthetic/Two.jpg',1);",
        )
        .unwrap();
}

#[test]
fn collections_persist_without_moving_assets_and_membership_is_idempotent() {
    let path = std::env::temp_dir().join(format!("icat-collections-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut ctx = Context::open(&path).unwrap();
    init(&mut ctx);
    assert!(rows(run(&mut ctx, &[("action", "list")])).is_empty());
    let name = "  Trip '26 🦀  ";
    let created = rows(run(&mut ctx, &[("action", "create"), ("name", name)]));
    assert_eq!(
        created,
        vec![vec![
            "1".to_owned(),
            "Trip '26 🦀".to_owned(),
            "manual".to_owned()
        ]]
    );
    // Names are display text; ids identify collections even when names repeat.
    run(&mut ctx, &[("action", "create"), ("name", name)]);
    assert_eq!(rows(run(&mut ctx, &[("action", "list")])).len(), 2);
    assert!(rows(run(&mut ctx, &[("action", "show"), ("id", "1")])).is_empty());
    for (collection, asset) in [("1", "1"), ("1", "1"), ("1", "2"), ("2", "1")] {
        run(
            &mut ctx,
            &[("action", "add"), ("id", collection), ("asset", asset)],
        );
    }
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1, 2]);
    let listed = rows(
        dispatch(
            &mut ctx,
            "list",
            &params(&[("collection", "1"), ("limit", "1")]),
        )
        .unwrap(),
    );
    assert_eq!(listed.len(), 1);
    let found = rows(
        dispatch(
            &mut ctx,
            "find",
            &params(&[("query", "Two"), ("collection", "1")]),
        )
        .unwrap(),
    );
    assert_eq!(found[0][0], "2");
    drop(ctx);
    let mut ctx = Context::open(&path).unwrap();
    assert_eq!(
        rows(run(&mut ctx, &[("action", "show"), ("id", "1")])).len(),
        2
    );
    for _ in 0..2 {
        run(
            &mut ctx,
            &[("action", "remove"), ("id", "1"), ("asset", "1")],
        );
    }
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![2]);
    assert_eq!(ctx.catalog().collection_assets(2).unwrap(), vec![1]);
    let asset = ctx.catalog().asset(1).unwrap().unwrap();
    assert_eq!(asset.folder_id, Some(1));
    assert_eq!(asset.path, "/synthetic/One.jpg");
    assert_eq!(ctx.catalog().stats().unwrap().assets, 2);
    drop(ctx);
    let _ = std::fs::remove_file(path);
}

#[test]
fn invalid_operations_report_errors_and_leave_membership_unchanged() {
    let mut ctx = Context::open(":memory:").unwrap();
    assert!(dispatch(&mut ctx, "collection", &params(&[("action", "list")])).is_err());
    init(&mut ctx);
    run(&mut ctx, &[("action", "create"), ("name", "Album")]);
    for pairs in [
        vec![],
        vec![("action", "unknown")],
        vec![("action", "create")],
        vec![("action", "create"), ("name", " \t ")],
        vec![("action", "create"), ("name", "bad\0name")],
        vec![("action", "show")],
        vec![("action", "show"), ("id", "999")],
        vec![("action", "show"), ("id", "-1")],
        vec![("action", "show"), ("id", "NaN")],
        vec![("action", "add"), ("id", "1")],
        vec![("action", "add"), ("id", "1"), ("asset", "999")],
        vec![("action", "remove"), ("id", "999"), ("asset", "1")],
        vec![("action", "remove"), ("id", "1"), ("asset", "0")],
        vec![("action", "list"), ("id", "1")],
        vec![("action", "create"), ("name", "Oops"), ("asset", "1")],
    ] {
        assert!(
            matches!(
                dispatch(&mut ctx, "collection", &params(&pairs)),
                Err(EngineError::BadParam(_))
            ),
            "{pairs:?}"
        );
        assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
        assert_eq!(ctx.catalog().all_collections().unwrap().len(), 1);
    }
    assert!(dispatch(
        &mut ctx,
        "collection",
        &params(&[("action", "create"), ("name", &"x".repeat(1025))])
    )
    .is_err());
    for id in ["0", "-1", "999", "1e100", "9223372036854775808"] {
        assert!(dispatch(&mut ctx, "list", &params(&[("collection", id)])).is_err());
        assert!(dispatch(
            &mut ctx,
            "find",
            &params(&[("query", ""), ("collection", id)])
        )
        .is_err());
    }
    ctx.catalog()
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_member AFTER INSERT ON collection_asset
         BEGIN SELECT RAISE(ABORT, 'synthetic collection failure'); END;",
        )
        .unwrap();
    assert!(dispatch(
        &mut ctx,
        "collection",
        &params(&[("action", "add"), ("id", "1"), ("asset", "1")])
    )
    .is_err());
    assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
    assert!(ctx.catalog().connection().is_autocommit());
}

#[test]
fn batch_membership_validates_every_id_and_rolls_back_failed_adds_and_removes() {
    let mut ctx = Context::open(":memory:").unwrap();
    init(&mut ctx);
    run(&mut ctx, &[("action", "create"), ("name", "Album")]);
    for assets in [
        "",
        "1,",
        "0,1",
        "1,-1",
        "1,NaN",
        "1,9223372036854775808",
        "1,999",
    ] {
        assert!(dispatch(
            &mut ctx,
            "collection",
            &params(&[("action", "add"), ("id", "1"), ("assets", assets)])
        )
        .is_err());
        assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
    }
    for extra in [
        params(&[
            ("action", "add"),
            ("id", "1"),
            ("asset", "1"),
            ("assets", "2"),
        ]),
        params(&[
            ("action", "add"),
            ("id", "1"),
            ("assets", &"1,".repeat(32_001)),
        ]),
        params(&[
            ("action", "add"),
            ("id", "1"),
            ("assets", &"9".repeat(704_001)),
        ]),
    ] {
        assert!(dispatch(&mut ctx, "collection", &extra).is_err());
    }
    ctx.catalog().connection().execute_batch("CREATE TRIGGER reject_second BEFORE INSERT ON collection_asset WHEN NEW.asset_id=2 BEGIN SELECT RAISE(ABORT, 'second member failed'); END;").unwrap();
    assert!(dispatch(
        &mut ctx,
        "collection",
        &params(&[("action", "add"), ("id", "1"), ("assets", "1,2")])
    )
    .is_err());
    assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
    ctx.catalog()
        .connection()
        .execute_batch("DROP TRIGGER reject_second")
        .unwrap();
    for _ in 0..2 {
        run(
            &mut ctx,
            &[("action", "add"), ("id", "1"), ("assets", "1, 2,1")],
        );
    }
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1, 2]);
    ctx.catalog().connection().execute_batch("CREATE TRIGGER reject_second BEFORE DELETE ON collection_asset WHEN OLD.asset_id=2 BEGIN SELECT RAISE(ABORT, 'second removal failed'); END;").unwrap();
    assert!(dispatch(
        &mut ctx,
        "collection",
        &params(&[("action", "remove"), ("id", "1"), ("assets", "1,2")])
    )
    .is_err());
    assert_eq!(ctx.catalog().collection_assets(1).unwrap(), vec![1, 2]);
    ctx.catalog()
        .connection()
        .execute_batch("DROP TRIGGER reject_second")
        .unwrap();
    for _ in 0..2 {
        run(
            &mut ctx,
            &[("action", "remove"), ("id", "1"), ("assets", "1,2")],
        );
    }
    assert!(ctx.catalog().collection_assets(1).unwrap().is_empty());
    assert_eq!(ctx.catalog().stats().unwrap().assets, 2);
    assert!(ctx.catalog().connection().is_autocommit());
}
