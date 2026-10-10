#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use icatalog_engine::*;

fn tmp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("icat-eng-{name}-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn rows(o: Outcome) -> Vec<Vec<String>> {
    match o {
        Outcome::Table { rows, .. } => rows,
        Outcome::Text(t) => panic!("expected table, got {t}"),
    }
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
    }
}

fn inited(name: &str) -> (Context, std::path::PathBuf) {
    let p = tmp(name);
    let mut ctx = Context::open(&p).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    (ctx, p)
}

#[test]
fn ids() {
    assert_eq!(
        command_ids(),
        vec!["init", "open", "list", "find", "status", "import"]
    );
}

#[test]
fn init_twice() {
    let p = tmp("init");
    let mut ctx = Context::open(&p).unwrap();
    assert!(p.exists());
    let a = dispatch(&mut ctx, "init", &Params::new()).unwrap();
    assert!(a.render().contains("initialised catalogue"));
    let b = dispatch(&mut ctx, "init", &Params::new()).unwrap();
    assert!(b.render().contains("already initialised"));
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn status_and_open() {
    let (mut ctx, p) = inited("status");
    let r = rows(dispatch(&mut ctx, "status", &Params::new()).unwrap());
    for m in ["assets", "folders", "generation", "schema_version"] {
        assert!(r.iter().any(|row| row[0] == m), "{m}");
    }
    let o = dispatch(&mut ctx, "open", &Params::new()).unwrap();
    assert!(o.render().contains("generation"));
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn list_and_find() {
    let (mut ctx, p) = inited("list");
    ctx.catalog().insert_asset(&asset("sunset.jpg")).unwrap();
    let l = rows(dispatch(&mut ctx, "list", &Params::new()).unwrap());
    assert_eq!(l.len(), 1);
    assert_eq!(l[0][1], "sunset.jpg");
    let mut hit = Params::new();
    hit.set("query", "sunset");
    assert_eq!(rows(dispatch(&mut ctx, "find", &hit).unwrap()).len(), 1);
    let mut miss = Params::new();
    miss.set("query", "zzzz");
    assert!(rows(dispatch(&mut ctx, "find", &miss).unwrap()).is_empty());
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn errors() {
    let (mut ctx, p) = inited("errors");
    assert!(matches!(
        dispatch(&mut ctx, "nope", &Params::new()),
        Err(EngineError::UnknownCommand(_))
    ));
    assert!(matches!(
        dispatch(&mut ctx, "find", &Params::new()),
        Err(EngineError::BadParam(_))
    ));
    let mut bad = Params::new();
    bad.set("query", "x").set("rating", "abc");
    assert!(matches!(
        dispatch(&mut ctx, "find", &bad),
        Err(EngineError::BadParam(_))
    ));
    assert!(dispatch_guarded(&mut ctx, "status", &Params::new()).is_ok());
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn disabled_when_uninitialised() {
    let p = tmp("uninit");
    let mut ctx = Context::open(&p).unwrap();
    assert!(matches!(
        dispatch(&mut ctx, "list", &Params::new()),
        Err(EngineError::Unsupported(_))
    ));
    drop(ctx);
    let _ = std::fs::remove_file(&p);
}
