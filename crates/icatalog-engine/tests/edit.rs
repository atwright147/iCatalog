#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use icatalog_catalog::Annotation;
use icatalog_engine::*;

fn tmp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("icat-edit-{name}-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn asset(name: &str) -> icatalog_catalog::Asset {
    icatalog_catalog::Asset {
        id: 0,
        uid: format!("u-{name}"),
        folder_id: None,
        path: format!("pics/{name}"),
        path_raw: None,
        name: name.into(),
        name_raw: None,
        format: Some("JPEG".into()),
        file_size: None,
        width: None,
        height: None,
        color_space: None,
        mtime: None,
        content_hash: None,
        missing: false,
    }
}

fn seed(name: &str) -> (Context, i64, i64, std::path::PathBuf) {
    let p = tmp(name);
    let mut ctx = Context::open(&p).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    let a = ctx.catalog().insert_asset(&asset("sunset.jpg")).unwrap();
    let b = ctx.catalog().insert_asset(&asset("beach.png")).unwrap();
    (ctx, a, b, p)
}

fn edit(ctx: &mut Context, id: i64, kv: &[(&str, &str)]) -> Outcome {
    let mut p = Params::new();
    p.set("id", id.to_string());
    for (k, v) in kv {
        p.set(*k, *v);
    }
    dispatch(ctx, "edit", &p).unwrap()
}

fn names(ctx: &Context, id: i64) -> Vec<String> {
    ctx.catalog()
        .asset_keywords(id)
        .unwrap()
        .into_iter()
        .map(|k| k.name)
        .collect()
}

fn keep(rows: Outcome) -> String {
    match rows {
        Outcome::Text(t) => t,
        Outcome::Table { .. } => panic!("expected text outcome"),
    }
}

#[test]
fn rating_and_label() {
    let (mut ctx, a, _, p) = seed("rl");
    let o = keep(edit(&mut ctx, a, &[("rating", "4"), ("label", "2")]));
    assert!(o.contains("rating 4"), "{o}");
    assert!(o.contains("label 2"), "{o}");
    let ann = ctx.catalog().annotation(a).unwrap().unwrap();
    assert_eq!(ann.rating, 4);
    assert_eq!(ann.label, 2);
    assert!(ann.updated_utc.is_some());
    keep(edit(&mut ctx, a, &[("rating", "4")]));
    let ann = ctx.catalog().annotation(a).unwrap().unwrap();
    assert_eq!(ann.rating, 4);
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn preserves_other_annotation_fields() {
    let (mut ctx, a, _, p) = seed("pres");
    let ann = Annotation {
        asset_id: a,
        rating: 0,
        label: 0,
        caption: Some("shot at dusk".into()),
        headline: None,
        creator: Some("andy".into()),
        copyright: None,
        updated_utc: None,
    };
    ctx.catalog().set_annotation(&ann).unwrap();
    let o = keep(edit(&mut ctx, a, &[("rating", "5")]));
    assert!(o.contains("rating 5"), "{o}");
    let ann = ctx.catalog().annotation(a).unwrap().unwrap();
    assert_eq!(ann.rating, 5);
    assert_eq!(ann.caption.as_deref(), Some("shot at dusk"));
    assert_eq!(ann.creator.as_deref(), Some("andy"));
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn keywords_add_remove_and_clear() {
    let (mut ctx, a, _, p) = seed("kw");
    let o = keep(edit(&mut ctx, a, &[("keyword", "sunset, beach")]));
    assert!(o.contains("+sunset"), "{o}");
    assert!(o.contains("+beach"), "{o}");
    assert_eq!(names(&ctx, a), vec!["beach", "sunset"]);

    let o = edit(&mut ctx, a, &[("keyword", "sunset")]);
    let o = keep(o);
    assert_eq!(
        o,
        "asset ".to_string()
            + &a.to_string()
            + " (sunset.jpg) already has these values; nothing changed"
    );
    assert_eq!(names(&ctx, a).len(), 2);

    keep(edit(&mut ctx, a, &[("remove-keyword", "sunset")]));
    assert_eq!(names(&ctx, a), vec!["beach"]);

    keep(edit(&mut ctx, a, &[("clear-keywords", "true")]));
    assert!(names(&ctx, a).is_empty());

    let mut other = Params::new();
    other.set("id", a.to_string());
    assert!(matches!(
        dispatch(&mut ctx, "edit", &other),
        Err(EngineError::BadParam(_))
    ));
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn other_asset_unaffected() {
    let (mut ctx, a, b, p) = seed("other");
    edit(&mut ctx, a, &[("keyword", "sunset"), ("rating", "5")]);
    assert_eq!(names(&ctx, b), Vec::<String>::new());
    assert!(ctx.catalog().annotation(b).unwrap().is_none());
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn validation() {
    let (mut ctx, a, _, p) = seed("val");
    assert!(matches!(
        dispatch(&mut ctx, "edit", &Params::new()),
        Err(EngineError::BadParam(_))
    ));
    let mut no_id = Params::new();
    no_id.set("rating", "3");
    assert!(matches!(
        dispatch(&mut ctx, "edit", &no_id),
        Err(EngineError::BadParam(_))
    ));
    for (k, v) in [
        ("rating", "6"),
        ("rating", "-1"),
        ("rating", "abc"),
        ("label", "8"),
        ("label", "-2"),
        ("id", "9999"),
        ("id", "abc"),
        ("clear-keywords", "banana"),
        ("keyword", ","),
    ] {
        let mut ps = Params::new();
        ps.set("id", a.to_string());
        ps.set(k, v);
        assert!(
            matches!(
                dispatch(&mut ctx, "edit", &ps),
                Err(EngineError::BadParam(_))
            ),
            "{k}={v} should be a BadParam"
        );
    }
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn uninitialised_is_unsupported() {
    let p = tmp("uninit");
    let mut ctx = Context::open(&p).unwrap();
    let mut ps = Params::new();
    ps.set("id", "1");
    ps.set("rating", "3");
    assert!(matches!(
        dispatch(&mut ctx, "edit", &ps),
        Err(EngineError::Unsupported(_))
    ));
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}
