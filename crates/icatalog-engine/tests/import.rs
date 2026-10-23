#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use icatalog_catalog::AssetQuery;
use icatalog_engine::*;

fn metric(o: Outcome, k: &str) -> u64 {
    match o {
        Outcome::Table { rows, .. } => rows.iter().find(|r| r[0] == k).unwrap()[1].parse().unwrap(),
        Outcome::Text(t) => panic!("{t}"),
    }
}

fn png(path: &std::path::Path, w: u32, h: u32, c: u8) {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([c, 10, 200]));
    img.save(path).unwrap();
}

fn setup(name: &str) -> (Context, std::path::PathBuf, std::path::PathBuf) {
    let base = std::env::temp_dir().join(format!("icat-imp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("sub")).unwrap();
    png(&base.join("a.png"), 40, 20, 1);
    png(&base.join("b.PNG"), 30, 50, 90);
    std::fs::write(base.join("c.txt"), b"hi").unwrap();
    png(&base.join("sub/d.png"), 17, 9, 200);
    let mut ctx = Context::open(base.join("cat.sqlite")).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    let dir = base.join("scan");
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["a.png", "b.PNG", "c.txt"] {
        std::fs::rename(base.join(f), dir.join(f)).unwrap();
    }
    std::fs::rename(base.join("sub"), dir.join("sub")).unwrap();
    (ctx, base, dir)
}

fn run(ctx: &mut Context, dir: &std::path::Path, rec: Option<&str>) -> Result<Outcome> {
    let mut p = Params::new();
    p.set("path", dir.to_str().unwrap());
    if let Some(r) = rec {
        p.set("recursive", r);
    }
    dispatch(ctx, "import", &p)
}

#[test]
fn import_dedupe_thumbs() {
    let (mut ctx, base, dir) = setup("a");
    let o = run(&mut ctx, &dir, None).unwrap();
    assert_eq!(metric(o, "imported"), 3);
    let assets = ctx.catalog().query_assets(&AssetQuery::default()).unwrap();
    assert_eq!(assets.len(), 3);
    assert!(assets.iter().all(|a| !a.name.ends_with(".txt")));
    let t = ctx
        .catalog()
        .thumbnail(assets[0].id, i64::from(icatalog_media::GRID_LONG_EDGE))
        .unwrap()
        .unwrap();
    assert_eq!(t.format, "jpeg");
    assert!(!t.orientation_applied);
    let o = run(&mut ctx, &dir, None).unwrap();
    assert_eq!(metric(o, "imported"), 0);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn non_recursive_and_dupes() {
    let (mut ctx, base, dir) = setup("b");
    let o = run(&mut ctx, &dir, Some("false")).unwrap();
    assert_eq!(metric(o, "imported"), 2);
    let o = run(&mut ctx, &dir, None).unwrap();
    assert_eq!(metric(o, "duplicates"), 2);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn skip_reasons_reported() {
    let (mut ctx, base, dir) = setup("d");
    let big = dir.join("huge.tif");
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(512 * 1024 * 1024 + 1).unwrap();
    drop(f);
    std::fs::write(dir.join("fake.tif"), b"not a tiff").unwrap();
    let o = run(&mut ctx, &dir, Some("false")).unwrap();
    assert_eq!(metric(o.clone(), "imported"), 2);
    assert_eq!(metric(o.clone(), "skipped"), 3);
    assert_eq!(metric(o.clone(), "skipped: unsupported file type"), 1);
    assert_eq!(metric(o.clone(), "skipped: larger than 512 MiB"), 1);
    assert_eq!(metric(o.clone(), "skipped: not a recognised image"), 1);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn import_builds_source_and_folder_tree() {
    let (mut ctx, base, dir) = setup("folders");
    run(&mut ctx, &dir, None).unwrap();
    let sources = ctx.catalog().all_sources().unwrap();
    assert_eq!(sources.len(), 1);
    let s = &sources[0];
    assert_eq!(s.kind, "local");
    assert_eq!(s.label, "scan");
    assert_eq!(
        s.identity,
        std::path::absolute(&dir).unwrap().to_string_lossy()
    );
    assert_eq!(s.last_mount_path.as_deref(), Some(s.identity.as_str()));
    #[cfg(not(windows))]
    assert_eq!(s.device_label.as_deref(), Some("This Mac"));

    let folders = ctx.catalog().all_folders().unwrap();
    assert_eq!(folders.len(), 2, "root + sub");
    let root = folders.iter().find(|f| f.parent_id.is_none()).unwrap();
    assert_eq!(root.source_id, Some(s.id));
    assert_eq!(root.path.as_deref(), Some(""));
    assert_eq!(root.name, "scan");
    let sub = ctx
        .catalog()
        .folder_by_source_path(s.id, "sub")
        .unwrap()
        .unwrap();
    assert_eq!(sub.parent_id, Some(root.id));
    assert_eq!(sub.name, "sub");

    let assets = ctx.catalog().query_assets(&AssetQuery::default()).unwrap();
    let in_folder = |fid: i64| assets.iter().filter(|a| a.folder_id == Some(fid)).count();
    assert_eq!(in_folder(root.id), 2, "a.png + b.PNG");
    assert_eq!(in_folder(sub.id), 1, "sub/d.png");

    // Re-importing the same root reuses the source and its folders.
    run(&mut ctx, &dir, None).unwrap();
    assert_eq!(ctx.catalog().all_sources().unwrap().len(), 1);
    assert_eq!(ctx.catalog().all_folders().unwrap().len(), 2);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn import_kind_is_declared_and_validated() {
    let (mut ctx, base, dir) = setup("kind");
    let mut p = Params::new();
    p.set("path", dir.to_str().unwrap());
    p.set("kind", "optical");
    dispatch(&mut ctx, "import", &p).unwrap();
    assert_eq!(ctx.catalog().all_sources().unwrap()[0].kind, "optical");

    let mut p = Params::new();
    p.set("path", dir.to_str().unwrap());
    p.set("kind", "floppy");
    assert!(matches!(
        dispatch(&mut ctx, "import", &p),
        Err(EngineError::BadParam(_))
    ));
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn bad_params() {
    let (mut ctx, base, dir) = setup("c");
    assert!(matches!(
        dispatch(&mut ctx, "import", &Params::new()),
        Err(EngineError::BadParam(_))
    ));
    assert!(run(&mut ctx, &dir.join("missing"), None).is_err());
    assert!(run(&mut ctx, &dir.join("a.png"), None).is_err());
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}
