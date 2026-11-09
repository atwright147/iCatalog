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

#[cfg(target_os = "linux")]
#[test]
fn invalid_utf8_file_does_not_mark_an_old_lossy_path_missing() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let (mut ctx, dir) = setup("lossy-legacy-path");
    let invalid = dir.join(OsString::from_vec(b"old-\xff.png".to_vec()));
    std::fs::copy(dir.join("a.png"), &invalid).unwrap();
    let mut legacy = all(&ctx).into_iter().find(|a| a.name == "a.png").unwrap();
    legacy.uid = "legacy-lossy-uid".into();
    legacy.path = invalid.to_string_lossy().into_owned();
    legacy.name = "old-�.png".into();
    legacy.content_hash = None;
    let id = ctx.catalog().insert_asset(&legacy).unwrap();

    let outcome = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&outcome, "skipped: path is not UTF-8"), 1);
    assert!(!ctx.catalog().asset(id).unwrap().unwrap().missing);
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
fn failed_thumbnail_write_rolls_back_asset_change_and_retry_repairs_it() {
    let (mut ctx, dir) = setup("thumbnail-write-failure");
    let before = all(&ctx).into_iter().find(|a| a.name == "a.png").unwrap();
    let thumbnail_before = ctx.catalog().thumbnail(before.id, 256).unwrap().unwrap();
    let generation_before = ctx.catalog().catalogue().unwrap().unwrap().generation;
    ctx.catalog()
        .connection()
        .execute_batch(
            "CREATE TRIGGER fail_thumbnail_write BEFORE INSERT ON thumbnail
             BEGIN SELECT RAISE(ABORT, 'synthetic thumbnail write failure'); END;",
        )
        .unwrap();

    png(&dir.join("a.png"), 100, 60, 150);
    let mut params = Params::new();
    params.set("path", dir.to_str().unwrap());
    let result = dispatch(&mut ctx, "update", &params);
    assert!(
        result.is_err(),
        "catalogue write failures must abort update"
    );
    assert_eq!(ctx.catalog().asset(before.id).unwrap().unwrap(), before);
    assert_eq!(
        ctx.catalog().thumbnail(before.id, 256).unwrap().unwrap(),
        thumbnail_before
    );
    assert_eq!(
        ctx.catalog().catalogue().unwrap().unwrap().generation,
        generation_before
    );
    assert!(ctx.catalog().connection().is_autocommit());

    ctx.catalog()
        .connection()
        .execute_batch("DROP TRIGGER fail_thumbnail_write")
        .unwrap();
    let outcome = dispatch(&mut ctx, "update", &params).unwrap();
    assert_eq!(metric(&outcome, "changed"), 1);
    let after = ctx.catalog().asset(before.id).unwrap().unwrap();
    let thumbnail_after = ctx.catalog().thumbnail(before.id, 256).unwrap().unwrap();
    assert_ne!(after.content_hash, before.content_hash);
    assert_eq!((after.width, after.height), (Some(100), Some(60)));
    assert_ne!(thumbnail_after.bytes, thumbnail_before.bytes);
    assert_eq!((thumbnail_after.width, thumbnail_after.height), (100, 60));
}

#[test]
fn undecodable_replacement_keeps_old_asset_and_thumbnail_until_retry() {
    let (mut ctx, dir) = setup("undecodable-replacement");
    let before = all(&ctx).into_iter().find(|a| a.name == "a.png").unwrap();
    let thumbnail_before = ctx.catalog().thumbnail(before.id, 256).unwrap().unwrap();
    let generation_before = ctx.catalog().catalogue().unwrap().unwrap().generation;
    let path = dir.join("a.png");
    png(&path, 100, 60, 150);
    let valid_bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &valid_bytes[..40]).unwrap();

    let outcome = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&outcome, "skipped: not a recognised image"), 1);
    assert_eq!(metric(&outcome, "changed"), 0);
    assert_eq!(ctx.catalog().asset(before.id).unwrap().unwrap(), before);
    assert_eq!(
        ctx.catalog().thumbnail(before.id, 256).unwrap().unwrap(),
        thumbnail_before
    );
    assert_eq!(
        ctx.catalog().catalogue().unwrap().unwrap().generation,
        generation_before
    );

    std::fs::write(&path, valid_bytes).unwrap();
    let outcome = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&outcome, "changed"), 1);
    let after = ctx.catalog().asset(before.id).unwrap().unwrap();
    let thumbnail_after = ctx.catalog().thumbnail(before.id, 256).unwrap().unwrap();
    assert_ne!(after.content_hash, before.content_hash);
    assert_eq!((after.width, after.height), (Some(100), Some(60)));
    assert_ne!(thumbnail_after.bytes, thumbnail_before.bytes);
}

#[test]
fn update_repairs_a_previously_committed_stale_thumbnail() {
    let (mut ctx, dir) = setup("repair-stale-thumbnail");
    let mut asset = all(&ctx).into_iter().find(|a| a.name == "a.png").unwrap();
    let thumbnail_before = ctx.catalog().thumbnail(asset.id, 256).unwrap().unwrap();
    let path = dir.join("a.png");
    png(&path, 100, 60, 150);
    let metadata = std::fs::metadata(&path).unwrap();
    asset.file_size = Some(i64::try_from(metadata.len()).unwrap());
    asset.mtime = Some(
        i64::try_from(
            metadata
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        .unwrap(),
    );
    asset.content_hash = Some(icatalog_media::content_hash(&std::fs::read(&path).unwrap()));
    asset.width = Some(100);
    asset.height = Some(60);
    // Simulate the former failure path: asset fields were committed, but the
    // previous thumbnail remained, so size/time alone appear unchanged.
    ctx.catalog().update_asset(&asset).unwrap();
    assert_ne!(
        ctx.catalog().thumbnail_derivation(asset.id, 256).unwrap(),
        Some(format!(
            "{}:{}",
            asset.file_size.unwrap(),
            asset.mtime.unwrap()
        ))
    );

    let outcome = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&outcome, "changed"), 1);
    let thumbnail_after = ctx.catalog().thumbnail(asset.id, 256).unwrap().unwrap();
    assert_ne!(thumbnail_after.bytes, thumbnail_before.bytes);
    assert_eq!((thumbnail_after.width, thumbnail_after.height), (100, 60));
    assert_eq!(
        thumbnail_after.derived_from,
        Some(format!(
            "{}:{}",
            asset.file_size.unwrap(),
            asset.mtime.unwrap()
        ))
    );
}

#[test]
fn touched_unchanged_file_does_not_regenerate_on_next_rescan() {
    let (mut ctx, dir) = setup("touched-unchanged-file");
    let asset = all(&ctx).into_iter().find(|a| a.name == "a.png").unwrap();
    let thumbnail_before = ctx.catalog().thumbnail(asset.id, 256).unwrap().unwrap();
    let touched_time = std::time::UNIX_EPOCH
        + std::time::Duration::from_secs(u64::try_from(asset.mtime.unwrap()).unwrap() + 5);
    std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("a.png"))
        .unwrap()
        .set_modified(touched_time)
        .unwrap();

    let first = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&first, "changed"), 0);
    let thumbnail_after = ctx.catalog().thumbnail(asset.id, 256).unwrap().unwrap();
    assert_eq!(thumbnail_after.bytes, thumbnail_before.bytes);
    assert_ne!(thumbnail_after.derived_from, thumbnail_before.derived_from);
    let generation_after_first = ctx.catalog().catalogue().unwrap().unwrap().generation;

    let second = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&second, "changed"), 0);
    assert_eq!(metric(&second, "unchanged"), 2);
    assert_eq!(
        ctx.catalog().catalogue().unwrap().unwrap().generation,
        generation_after_first
    );
    assert_eq!(
        ctx.catalog().thumbnail(asset.id, 256).unwrap().unwrap(),
        thumbnail_after
    );
}

#[test]
fn matching_hash_without_thumbnail_repairs_on_first_rescan() {
    let (mut ctx, dir) = setup("missing-thumbnail");
    let mut asset = all(&ctx).into_iter().find(|a| a.name == "a.png").unwrap();
    ctx.catalog()
        .connection()
        .execute("DELETE FROM thumbnail WHERE asset_id = ?1", [asset.id])
        .unwrap();
    asset.file_size = None;
    asset.mtime = None;
    ctx.catalog().update_asset(&asset).unwrap();

    let first = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&first, "changed"), 1);
    assert!(ctx.catalog().thumbnail(asset.id, 256).unwrap().is_some());
    let second = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&second, "changed"), 0);
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
fn offline_source_is_not_marked_missing() {
    let (mut ctx, dir) = setup("offline");
    let parked = dir.with_file_name("scan-offline");
    std::fs::rename(&dir, &parked).unwrap();
    let o = update(&mut ctx, None);
    assert_eq!(metric(&o, "offline"), 2);
    assert_eq!(metric(&o, "missing"), 0);
    assert!(all(&ctx).iter().all(|a| !a.missing));
    std::fs::rename(&parked, &dir).unwrap();
    let o = update(&mut ctx, None);
    assert_eq!(metric(&o, "offline"), 0);
    assert_eq!(metric(&o, "missing"), 0);
}

#[test]
fn update_files_new_assets_into_source_folders() {
    let (mut ctx, dir) = setup("newfolders");
    std::fs::create_dir_all(dir.join("newdir")).unwrap();
    png(&dir.join("newdir/c.png"), 12, 12, 3);
    update(&mut ctx, Some(&dir));
    let sources = ctx.catalog().all_sources().unwrap();
    let sub = ctx
        .catalog()
        .folder_by_source_path(sources[0].id, "newdir")
        .unwrap()
        .unwrap();
    let a = all(&ctx).into_iter().find(|a| a.name == "c.png").unwrap();
    assert_eq!(a.folder_id, Some(sub.id));
}

#[test]
fn skip_reasons_reported() {
    let (mut ctx, dir) = setup("skip");
    std::fs::write(dir.join("notes.txt"), b"hi").unwrap();
    let o = update(&mut ctx, Some(&dir));
    assert_eq!(metric(&o, "skipped"), 1);
    assert_eq!(metric(&o, "skipped: unsupported file type"), 1);
    assert_eq!(all(&ctx).len(), 2);
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
