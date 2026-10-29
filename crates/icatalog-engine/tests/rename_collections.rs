#![allow(clippy::unwrap_used, clippy::panic)]
use icatalog_engine::{dispatch, Context, Params};

fn rename(
    ctx: &mut Context,
    id: &str,
    name: &str,
) -> icatalog_engine::Result<icatalog_engine::Outcome> {
    let mut p = Params::new();
    p.set("action", "rename").set("id", id).set("name", name);
    dispatch(ctx, "collection", &p)
}

#[test]
fn rename_both_kinds_preserves_identity_members_rules_and_duplicate_names() {
    let path = std::env::temp_dir().join(format!("icat-rename-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut ctx = Context::open(&path).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    ctx.catalog()
        .connection()
        .execute_batch("INSERT INTO asset(id,uid,name,path) VALUES(1,'one','One.jpg','/one.jpg');")
        .unwrap();
    let manual = ctx.catalog().insert_collection("Manual").unwrap();
    ctx.catalog().add_to_collection(manual, 1).unwrap();
    let smart = ctx.catalog().insert_collection("Smart").unwrap();
    let rules = icatalog_catalog::SmartRules {
        text: Some("One".into()),
        ..Default::default()
    };
    ctx.catalog().set_smart_rules(smart, &rules).unwrap();
    for id in [manual, smart] {
        rename(&mut ctx, &id.to_string(), "  Renamed 🦀 '26  ").unwrap();
        rename(&mut ctx, &id.to_string(), "Renamed 🦀 '26").unwrap();
    }
    drop(ctx);
    let ctx = Context::open(&path).unwrap();
    for id in [manual, smart] {
        let c = ctx.catalog().collection(id).unwrap().unwrap();
        assert_eq!(c.id, id);
        assert_eq!(c.name, "Renamed 🦀 '26");
        assert_eq!(ctx.catalog().collection_assets(id).unwrap(), vec![1]);
    }
    assert_eq!(ctx.catalog().smart_rules(smart).unwrap(), Some(rules));
    assert!(ctx.catalog().smart_rules(manual).unwrap().is_none());
    assert_eq!(ctx.catalog().asset(1).unwrap().unwrap().uid, "one");
    drop(ctx);
    let _ = std::fs::remove_file(path);
}

#[test]
fn invalid_and_failed_renames_do_not_change_names_or_membership() {
    let mut ctx = Context::open(":memory:").unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    ctx.catalog().insert_collection("Original").unwrap();
    for (id, name) in [
        ("0", "Name"),
        ("-1", "Name"),
        ("999", "Name"),
        ("NaN", "Name"),
        ("1", ""),
        ("1", " \t "),
        ("1", "bad\0name"),
        ("1", "bad\nname"),
        ("1", &"x".repeat(1025)),
    ] {
        assert!(rename(&mut ctx, id, name).is_err());
        assert_eq!(
            ctx.catalog().collection(1).unwrap().unwrap().name,
            "Original"
        );
    }
    for pairs in [
        vec![("action", "rename"), ("id", "1")],
        vec![("action", "rename"), ("name", "Oops")],
        vec![
            ("action", "rename"),
            ("id", "1"),
            ("name", "Oops"),
            ("format", "JPEG"),
        ],
    ] {
        let mut p = Params::new();
        for (k, v) in pairs {
            p.set(k, v);
        }
        assert!(dispatch(&mut ctx, "collection", &p).is_err());
    }
    ctx.catalog().connection().execute_batch("CREATE TRIGGER fail_rename AFTER UPDATE OF name ON collection BEGIN SELECT RAISE(ABORT,'synthetic rename failure'); END;").unwrap();
    assert!(rename(&mut ctx, "1", "Fail").is_err());
    assert_eq!(
        ctx.catalog().collection(1).unwrap().unwrap().name,
        "Original"
    );
    assert!(ctx.catalog().connection().is_autocommit());
    assert!(ctx.catalog().rename_collection(999, "Unknown").is_err());
}
