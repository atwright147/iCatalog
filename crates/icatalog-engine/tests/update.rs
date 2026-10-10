#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use icatalog_catalog::AssetQuery;
use icatalog_engine::*;

fn metric(o: &Outcome, k: &str) -> u64 {
    match o {
        Outcome::Table { rows, .. } => rows.iter().find(|r| r[0] == k).unwrap()[1].parse().unwrap(),
        Outcome::Text(t) => panic!("{t}"),
    }
}

fn png(path: &std::path::Path, w: u32, h: u32, c: u8) {
    image::RgbImage::from_pixel(w, h, image::Rgb([c, 10, 200]))
        .save(path)
        .unwrap();
}

fn setup(name: &str) -> (Context, std::path::PathBuf) {
    let base = std::env::temp_dir().join(format!("icat-upd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("scan");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    png(&dir.join("a.png"), 40, 20, 1);
    png(&dir.join("sub/b.png"), 30, 50, 90);
    let mut ctx = Context::open(base.join("cat.sqlite")).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    let mut p = Params::new();
    p.set("path", dir.to_str().unwrap());
    dispatch(&mut ctx, "import", &p).unwrap();
    (ctx, dir)
}

fn update(ctx: &mut Context, dir: Option<&std::path::Path>) -> Outcome {
    let mut p = Params::new();
    if let Some(d) = dir {
        p.set("path", d.to_str().unwrap());
    }
    dispatch(ctx, "update", &p).unwrap()
}

fn all(ctx: &Context) -> Vec<icatalog_catalog::Asset> {
    ctx.catalog().query_assets(&AssetQuery::default()).unwrap()
}

#[test]
fn added_and_unchanged() {
    let (mut ctx, dir) = setup("add");
    let o = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&o, "unchanged"), 2);
    assert_eq!(metric(&o, "added"), 0);
    png(&dir.join("new.png"), 11, 12, 77);
    let o = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&o, "added"), 1);
    assert_eq!(all(&ctx).len(), 3);
}

#[test]
fn changed_keeps_uid_and_refreshes() {
    let (mut ctx, dir) = setup("chg");
    let before = all(&ctx).into_iter().find(|a| a.name == "a.png").unwrap();
    let t0 = ctx.catalog().thumbnail(before.id, 256).unwrap().unwrap();
    png(&dir.join("a.png"), 100, 60, 150);
    let o = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&o, "changed"), 1);
    let after = ctx.catalog().asset(before.id).unwrap().unwrap();
    assert_eq!(after.uid, before.uid);
    assert_ne!(after.content_hash, before.content_hash);
    assert_eq!(after.width, Some(100));
    let t1 = ctx.catalog().thumbnail(before.id, 256).unwrap().unwrap();
    assert_ne!(t0.bytes, t1.bytes);
    assert_eq!(all(&ctx).len(), 2);
}

#[test]
fn missing_then_restored() {
    let (mut ctx, dir) = setup("miss");
    let saved = std::fs::read(dir.join("sub/b.png")).unwrap();
    std::fs::remove_file(dir.join("sub/b.png")).unwrap();
    let o = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&o, "missing"), 1);
    assert_eq!(all(&ctx).iter().filter(|a| a.missing).count(), 1);
    std::fs::write(dir.join("sub/b.png"), saved).unwrap();
    let o = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&o, "missing"), 0);
    assert!(all(&ctx).iter().all(|a| !a.missing));
    assert_eq!(all(&ctx).len(), 2);
}

#[test]
fn no_path_mode() {
    let (mut ctx, dir) = setup("nopath");
    png(&dir.join("new.png"), 9, 9, 5);
    std::fs::remove_file(dir.join("a.png")).unwrap();
    let o = update(&mut ctx, None);
    assert_eq!(metric(&o, "added"), 1);
    assert_eq!(metric(&o, "missing"), 1);
    assert_eq!(all(&ctx).len(), 3);
}

#[test]
fn adversarial_params() {
    let (mut ctx, dir) = setup("adv");
    for (k, v) in [
        ("path", "/nonexistent/zzz"),
        ("path", dir.join("a.png").to_str().unwrap()),
    ] {
        let mut p = Params::new();
        p.set(k, v);
        assert!(dispatch(&mut ctx, "update", &p).is_err());
    }
    let mut p = Params::new();
    p.set("recursive", "banana");
    assert!(dispatch(&mut ctx, "update", &p).is_err());
}
