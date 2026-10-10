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
