use icatalog_engine::{Context, Params};
use std::path::PathBuf;

fn view(c: &mut Context, id: &str) -> icatalog_engine::Result<icatalog_engine::Outcome> {
    let mut params = Params::new();
    params.set("id", id);
    icatalog_engine::dispatch_guarded(c, "preview", &params)
}

#[test]
fn viewer_reads_oriented_pixels_without_originals_or_writes_even_when_sync_required() {
    let (root, mut c) = fixture("viewer-read-only");
    c.catalog().connection().execute("INSERT INTO asset_meta(asset_id,orientation) VALUES(1,6) ON CONFLICT(asset_id) DO UPDATE SET orientation=6", []).unwrap();
    command(&mut c, "generate").unwrap();
    std::fs::remove_dir_all(root.join("media")).unwrap();
    c.set_sync_required(true).unwrap();
    for external in [false, true] {
        if external {
            command(&mut c, "external").unwrap();
        }
        let before = std::fs::read(c.path()).unwrap();
        let mut reader = Context::open_read_only(c.path()).unwrap();
        let icatalog_engine::Outcome::Preview(image) = view(&mut reader, "1").unwrap() else {
            panic!("expected preview pixels");
        };
        assert_eq!(image.asset_id, 1);
        assert_eq!(image.filename, "generated.png");
        assert_eq!((image.width, image.height), (960, 1920));
        assert_eq!(image.rgba.len(), 960 * 1920 * 4);
        assert_eq!(image.hash, c.catalog().preview(1).unwrap().unwrap().hash);
        drop(reader);
        assert_eq!(std::fs::read(c.path()).unwrap(), before);
    }
}

#[test]
fn viewer_reports_missing_and_corrupt_previews_without_repairing_or_generating() {
    let (root, mut c) = fixture("viewer-errors");
    let before = std::fs::read(c.path()).unwrap();
    assert!(view(&mut c, "1")
        .unwrap_err()
        .to_string()
        .contains("generate"));
    for id in ["", "0", "-1", "9223372036854775808", "NaN", "999"] {
        assert!(view(&mut c, id).is_err());
    }
    assert_eq!(std::fs::read(c.path()).unwrap(), before);
    command(&mut c, "external").unwrap();
    command(&mut c, "generate").unwrap();
    let reference = c.catalog().preview(1).unwrap().unwrap();
    let path = root
        .join("previews")
        .join(format!("{}.jpg", reference.hash));
    std::fs::remove_file(&path).unwrap();
    let before = std::fs::read(c.path()).unwrap();
    let error = view(&mut c, "1").unwrap_err().to_string();
    assert!(error.contains(&reference.hash) && error.contains("--preview"));
    assert!(!path.exists());
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(view(&mut c, "1").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    assert_eq!(std::fs::read(c.path()).unwrap(), before);
}

#[test]
fn viewer_rejects_hash_valid_non_images_oversized_pixels_and_mismatched_dimensions() {
    let (_, mut c) = fixture("viewer-hostile");
    // Model a foreign/corrupt database that bypassed the writer's CHECK constraints.
    c.catalog()
        .connection()
        .execute_batch("PRAGMA ignore_check_constraints=ON")
        .unwrap();
    for (width, height, bytes) in [
        (1, 1, b"not a jpeg".to_vec()),
        (1, 1, jpeg(1921, 1)),
        (1, 1, jpeg(2, 1)),
        (i64::MAX, 1, jpeg(2, 1)),
    ] {
        let hash = icatalog_engine::previews::hash(&bytes);
        c.catalog().store_preview_blob(&hash, Some(&bytes)).unwrap();
        c.catalog()
            .store_preview(&icatalog_catalog::Preview {
                asset_id: 1,
                hash,
                width,
                height,
                source_hash: c.catalog().asset(1).unwrap().unwrap().content_hash.unwrap(),
            })
            .unwrap();
        assert!(view(&mut c, "1").is_err());
    }
}

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(width, height))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    bytes
}

#[cfg(unix)]
#[test]
fn viewer_rejects_special_cache_entries_without_blocking() {
    const CHILD_PATH: &str = "ICATALOG_PREVIEW_SPECIAL_FILE_TEST";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let mut context = Context::open_read_only(path).unwrap();
        let error = view(&mut context, "1").unwrap_err().to_string();
        assert!(error.contains("regular file"), "{error}");
        return;
    }
    let (root, mut context) = fixture("viewer-special-file");
    command(&mut context, "generate").unwrap();
    command(&mut context, "external").unwrap();
    let reference = context.catalog().preview(1).unwrap().unwrap();
    let cache = root
        .join("previews")
        .join(format!("{}.jpg", reference.hash));
    std::fs::remove_file(&cache).unwrap();
    for pipe in [false, true] {
        if pipe {
            assert!(std::process::Command::new("mkfifo")
                .arg(&cache)
                .status()
                .unwrap()
                .success());
        } else {
            std::os::unix::fs::symlink(root.join("missing-target"), &cache).unwrap();
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "viewer_rejects_special_cache_entries_without_blocking",
                "--nocapture",
            ])
            .env(CHILD_PATH, context.path())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("opening a special preview cache entry blocked");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::remove_file(&cache).unwrap();
    }
}
fn dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("icat-previews-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn command(c: &mut Context, action: &str) -> icatalog_engine::Result<icatalog_engine::Outcome> {
    let mut p = Params::new();
    p.set("action", action);
    icatalog_engine::dispatch(c, "previews", &p)
}
fn fixture(name: &str) -> (PathBuf, Context) {
    let root = dir(name);
    let media = root.join("media");
    std::fs::create_dir(&media).unwrap();
    image::RgbImage::from_fn(2400, 1200, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 251) as u8, 80])
    })
    .save(media.join("generated.png"))
    .unwrap();
    let mut c = Context::open(root.join("catalogue.sqlite")).unwrap();
    icatalog_engine::dispatch(&mut c, "init", &Params::new()).unwrap();
    let mut p = Params::new();
    p.set("path", media.to_str().unwrap());
    icatalog_engine::dispatch(&mut c, "import", &p).unwrap();
    (root, c)
}
#[test]
fn generated_oriented_previews_convert_and_remain_portable_without_original_media() {
    let (root, mut c) = fixture("portable");
    c.catalog()
        .connection()
        .execute("INSERT INTO asset_meta(asset_id,orientation) VALUES(1,6) ON CONFLICT(asset_id) DO UPDATE SET orientation=6", [])
        .unwrap();
    assert!(command(&mut c, "generate")
        .unwrap()
        .render()
        .contains("generated 1"));
    let p = c.catalog().preview(1).unwrap().unwrap();
    assert_eq!((p.width, p.height), (960, 1920));
    let bytes = icatalog_engine::previews::read(c.catalog(), c.path(), &p.hash)
        .unwrap()
        .unwrap();
    assert_eq!(icatalog_engine::previews::hash(&bytes), p.hash);
    assert!(command(&mut c, "generate")
        .unwrap()
        .render()
        .contains("cached 1"));
    command(&mut c, "external").unwrap();
    assert!(c.catalog().preview_bytes(&p.hash).unwrap().is_none());
    assert!(root
        .join("previews")
        .join(format!("{}.jpg", p.hash))
        .exists());
    command(&mut c, "embed").unwrap();
    assert_eq!(
        c.catalog().preview_bytes(&p.hash).unwrap(),
        Some(bytes.clone())
    );
    std::fs::remove_dir_all(root.join("media")).unwrap();
    let portable = root.join("portable");
    std::fs::create_dir(&portable).unwrap();
    let target = portable.join("moved.sqlite");
    std::fs::copy(c.path(), &target).unwrap();
    let moved = Context::open(target).unwrap();
    assert_eq!(
        icatalog_engine::previews::read(moved.catalog(), moved.path(), &p.hash).unwrap(),
        Some(bytes)
    );
}
#[test]
fn missing_or_corrupt_external_bytes_abort_conversion_without_losing_references() {
    let (root, mut c) = fixture("failure");
    command(&mut c, "external").unwrap();
    command(&mut c, "generate").unwrap();
    let p = c.catalog().preview(1).unwrap().unwrap();
    let disk = root.join("previews").join(format!("{}.jpg", p.hash));
    std::fs::write(&disk, b"corrupt").unwrap();
    assert!(command(&mut c, "embed").is_err());
    assert_eq!(c.catalog().preview_mode().unwrap(), "external");
    assert_eq!(c.catalog().preview(1).unwrap(), Some(p.clone()));
    assert!(icatalog_engine::previews::read(c.catalog(), c.path(), &p.hash).is_err());
    for h in ["../outside", "", "ab", &"a".repeat(65)] {
        assert!(icatalog_engine::previews::read(c.catalog(), c.path(), h).is_err());
    }
}

#[test]
fn generate_repairs_corrupt_cached_preview_from_valid_media() {
    let (root, mut c) = fixture("repair-corrupt");
    command(&mut c, "external").unwrap();
    command(&mut c, "generate").unwrap();
    let preview = c.catalog().preview(1).unwrap().unwrap();
    let disk = root.join("previews").join(format!("{}.jpg", preview.hash));
    std::fs::write(&disk, b"corrupt").unwrap();
    assert!(command(&mut c, "generate")
        .unwrap()
        .render()
        .contains("generated 1"));
    assert_eq!(c.catalog().preview(1).unwrap(), Some(preview.clone()));
    assert!(
        icatalog_engine::previews::read(c.catalog(), c.path(), &preview.hash)
            .unwrap()
            .is_some()
    );
}
#[test]
fn changed_media_invalidates_preview_and_replica_generation_is_rejected() {
    let (_, mut c) = fixture("invalidation");
    command(&mut c, "generate").unwrap();
    assert!(c.catalog().preview(1).unwrap().is_some());
    c.catalog()
        .connection()
        .execute("UPDATE asset SET content_hash='changed' WHERE id=1", [])
        .unwrap();
    assert!(c.catalog().preview(1).unwrap().is_none());
    c.catalog().set_replica_base(0, b"test").unwrap();
    assert!(command(&mut c, "generate").is_err());
    assert!(command(&mut c, "external").is_err());
    assert!(command(&mut c, "status").is_ok());
}

#[test]
fn conversion_removes_only_unreferenced_embedded_preview_blobs() {
    let (_, mut c) = fixture("prune-orphan");
    command(&mut c, "generate").unwrap();
    let old = c.catalog().preview(1).unwrap().unwrap();
    let bytes = c.catalog().preview_bytes(&old.hash).unwrap().unwrap();
    c.catalog()
        .connection()
        .execute("UPDATE asset SET content_hash='changed' WHERE id=1", [])
        .unwrap();
    assert!(c.catalog().preview(1).unwrap().is_none());
    assert_eq!(c.catalog().preview_bytes(&old.hash).unwrap(), Some(bytes));

    command(&mut c, "external").unwrap();
    let count: i64 = c
        .catalog()
        .connection()
        .query_row(
            "SELECT count(*) FROM preview_blob WHERE hash=?1",
            [&old.hash],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn shared_preview_blob_survives_until_the_last_reference_is_invalidated() {
    let (_, mut c) = fixture("shared-preview");
    command(&mut c, "generate").unwrap();
    let preview = c.catalog().preview(1).unwrap().unwrap();
    let mut second = c.catalog().asset(1).unwrap().unwrap();
    second.uid = "second-preview-owner".into();
    second.path = "second-preview-owner.png".into();
    let second_id = c.catalog().insert_asset(&second).unwrap();
    c.catalog()
        .store_preview(&icatalog_catalog::Preview {
            asset_id: second_id,
            ..preview.clone()
        })
        .unwrap();
    c.catalog()
        .connection()
        .execute(
            "UPDATE asset SET content_hash='first-changed' WHERE id=1",
            [],
        )
        .unwrap();

    command(&mut c, "external").unwrap();
    assert!(c.catalog().preview(second_id).unwrap().is_some());
    let remaining: i64 = c
        .catalog()
        .connection()
        .query_row(
            "SELECT count(*) FROM preview_blob WHERE hash=?1",
            [&preview.hash],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 1);

    c.catalog()
        .connection()
        .execute(
            "UPDATE asset SET content_hash='second-changed' WHERE id=?1",
            [second_id],
        )
        .unwrap();
    command(&mut c, "external").unwrap();
    let remaining: i64 = c
        .catalog()
        .connection()
        .query_row(
            "SELECT count(*) FROM preview_blob WHERE hash=?1",
            [&preview.hash],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
}
