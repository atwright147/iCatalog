#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use icatalog_catalog::*;

fn asset(uid: &str, path: &str) -> Asset {
    Asset {
        id: 0,
        uid: uid.into(),
        folder_id: None,
        path: path.into(),
        path_raw: None,
        name: "n".into(),
        name_raw: None,
        format: None,
        file_size: None,
        width: None,
        height: None,
        color_space: None,
        mtime: None,
        content_hash: None,
    }
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("icat-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn count(c: &Catalog, table: &str) -> i64 {
    c.connection()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn thumb(asset_id: i64, size_class: i64) -> Thumbnail {
    Thumbnail {
        asset_id,
        size_class,
        source: "rendered".into(),
        format: "jpeg".into(),
        width: 10,
        height: 20,
        orientation_applied: true,
        bytes: vec![0, 255, 1, 254],
        derived_from: Some("h".into()),
        generator_version: 1,
    }
}

fn meta_raw(asset_id: i64, scheme: &str, key: &str, seq: i64) -> MetadataRaw {
    MetadataRaw {
        asset_id,
        scheme: scheme.into(),
        key: key.into(),
        seq,
        value: vec![1, 2, 0xff],
    }
}

fn annotation(asset_id: i64, rating: i64, label: i64) -> Annotation {
    Annotation {
        asset_id,
        rating,
        label,
        caption: Some("cap".into()),
        headline: None,
        creator: Some("me".into()),
        copyright: None,
        updated_utc: Some("2026-10-10T00:00:00Z".into()),
    }
}

#[test]
fn folder_round_trip() {
    let c = Catalog::open_in_memory().unwrap();
    let root = c.insert_folder(None, "root").unwrap();
    let child = c.insert_folder(Some(root), "child").unwrap();
    assert_eq!(
        c.folder(child).unwrap().unwrap(),
        Folder {
            id: child,
            parent_id: Some(root),
            name: "child".into()
        }
    );
    assert_eq!(c.child_folders(None).unwrap().len(), 1);
    assert_eq!(c.child_folders(Some(root)).unwrap()[0].id, child);
    assert!(c.folder(999).unwrap().is_none());
    assert!(c.insert_folder(Some(999), "bad").is_err());
}

#[test]
fn thumbnail_round_trip_and_constraint() {
    let c = Catalog::open_in_memory().unwrap();
    let a = c.insert_asset(&asset("u", "p")).unwrap();
    let t = thumb(a, 1);
    c.insert_thumbnail(&t).unwrap();
    assert_eq!(c.thumbnail(a, 1).unwrap().unwrap(), t);
    assert!(c.thumbnail(a, 2).unwrap().is_none());
    assert!(c.insert_thumbnail(&t).is_err());
    let mut t2 = thumb(a, 2);
    t2.orientation_applied = false;
    t2.derived_from = None;
    c.insert_thumbnail(&t2).unwrap();
    assert_eq!(c.thumbnail(a, 2).unwrap().unwrap(), t2);
}

#[test]
fn metadata_raw_round_trip_and_constraint() {
    let c = Catalog::open_in_memory().unwrap();
    let a = c.insert_asset(&asset("u", "p")).unwrap();
    let rows = [
        meta_raw(a, "iptc_iim", "2:25", 0),
        meta_raw(a, "iptc_iim", "2:25", 1),
        meta_raw(a, "exif_tiff", "all", 0),
        meta_raw(a, "xmp_packet", "all", 0),
    ];
    for r in &rows {
        c.insert_metadata_raw(r).unwrap();
    }
    let got = c.metadata_raw_for_asset(a).unwrap();
    assert_eq!(got.len(), 4);
    for r in &rows {
        assert!(got.contains(r));
    }
    assert!(c.insert_metadata_raw(&rows[0]).is_err());
}

#[test]
fn asset_meta_round_trip_and_upsert() {
    let c = Catalog::open_in_memory().unwrap();
    let a = c.insert_asset(&asset("u", "p")).unwrap();
    let mut m = AssetMeta {
        asset_id: a,
        captured_local: Some("2020-01-02T03:04:05".into()),
        tz_offset_min: Some(-300),
        make: Some("Make".into()),
        model: None,
        lens: None,
        focal_length_mm: Some(35.5),
        f_number: Some(1.8),
        exposure_s: None,
        iso: Some(400),
        gps_lat: Some(-12.5),
        gps_lon: None,
        orientation: Some(6),
        duration_ms: None,
    };
    c.upsert_asset_meta(&m).unwrap();
    assert_eq!(c.asset_meta(a).unwrap().unwrap(), m);
    m.iso = None;
    m.model = Some("M".into());
    c.upsert_asset_meta(&m).unwrap();
    assert_eq!(c.asset_meta(a).unwrap().unwrap(), m);
    assert_eq!(count(&c, "asset_meta"), 1);
    assert!(c.asset_meta(999).unwrap().is_none());
}

#[test]
fn collection_round_trip_and_constraint() {
    let c = Catalog::open_in_memory().unwrap();
    let a = c.insert_asset(&asset("u", "p")).unwrap();
    let b = c.insert_asset(&asset("v", "p")).unwrap();
    let col = c.insert_collection("Best").unwrap();
    assert_eq!(c.collection(col).unwrap().unwrap().name, "Best");
    assert!(c.collection(999).unwrap().is_none());
    c.add_to_collection(col, a).unwrap();
    c.add_to_collection(col, a).unwrap();
    c.add_to_collection(col, b).unwrap();
    assert_eq!(c.collection_assets(col).unwrap(), vec![a, b]);
    c.remove_from_collection(col, a).unwrap();
    assert_eq!(c.collection_assets(col).unwrap(), vec![b]);
    assert!(c
        .connection()
        .execute(
            "INSERT INTO collection_asset (collection_id, asset_id) VALUES (?1, ?2)",
            [col, b]
        )
        .is_err());
}

#[test]
fn keyword_and_annotation_round_trip() {
    let c = Catalog::open_in_memory().unwrap();
    let a = c.insert_asset(&asset("u", "p")).unwrap();
    let k = c.ensure_keyword("sunset", Some(&[0xff, 0x80])).unwrap();
    assert_eq!(c.ensure_keyword("sunset", None).unwrap(), k);
    let k2 = c.ensure_keyword("beach", None).unwrap();
    c.add_asset_keyword(a, k).unwrap();
    c.add_asset_keyword(a, k).unwrap();
    c.add_asset_keyword(a, k2).unwrap();
    let kws = c.asset_keywords(a).unwrap();
    assert_eq!(kws.len(), 2);
    assert_eq!(kws[1].name_raw, Some(vec![0xff, 0x80]));
    assert_eq!(c.assets_for_keyword(k).unwrap(), vec![a]);
    c.remove_asset_keyword(a, k).unwrap();
    assert!(c.assets_for_keyword(k).unwrap().is_empty());
    assert!(c
        .connection()
        .execute("INSERT INTO keyword (name) VALUES ('beach')", [])
        .is_err());

    let mut an = annotation(a, 3, 2);
    c.set_annotation(&an).unwrap();
    assert_eq!(c.annotation(a).unwrap().unwrap(), an);
    an.rating = 5;
    an.caption = None;
    c.set_annotation(&an).unwrap();
    assert_eq!(c.annotation(a).unwrap().unwrap(), an);
    assert!(c.annotation(999).unwrap().is_none());
}

#[test]
fn annotation_check_constraints() {
    let c = Catalog::open_in_memory().unwrap();
    let a = c.insert_asset(&asset("u", "p")).unwrap();
    assert!(c.set_annotation(&annotation(a, 99, 0)).is_err());
    assert!(c.set_annotation(&annotation(a, 0, 99)).is_err());
    assert!(c.set_annotation(&annotation(a, -1, 0)).is_err());
    assert!(c.set_annotation(&annotation(a, 5, 7)).is_ok());
}

#[test]
fn fk_cascade_on_asset_delete() {
    let c = Catalog::open_in_memory().unwrap();
    let a = c.insert_asset(&asset("u", "p")).unwrap();
    c.insert_thumbnail(&thumb(a, 1)).unwrap();
    c.insert_metadata_raw(&meta_raw(a, "s", "k", 0)).unwrap();
    c.upsert_asset_meta(&AssetMeta {
        asset_id: a,
        captured_local: None,
        tz_offset_min: None,
        make: None,
        model: None,
        lens: None,
        focal_length_mm: None,
        f_number: None,
        exposure_s: None,
        iso: None,
        gps_lat: None,
        gps_lon: None,
        orientation: None,
        duration_ms: None,
    })
    .unwrap();
    let col = c.insert_collection("c").unwrap();
    c.add_to_collection(col, a).unwrap();
    let k = c.ensure_keyword("k", None).unwrap();
    c.add_asset_keyword(a, k).unwrap();
    c.set_annotation(&annotation(a, 1, 1)).unwrap();
    c.delete_asset(a).unwrap();
    for t in [
        "thumbnail",
        "metadata_raw",
        "asset_meta",
        "collection_asset",
        "asset_keyword",
        "annotation",
        "asset",
    ] {
        assert_eq!(count(&c, t), 0, "{t}");
    }
    assert_eq!(count(&c, "keyword"), 1);
    assert_eq!(count(&c, "collection"), 1);
}

#[test]
fn uid_unique_path_not_unique_and_raw_bytes() {
    let c = Catalog::open_in_memory().unwrap();
    let mut a = asset("u1", "same");
    a.path_raw = Some(vec![0x8a, 0xff, 0x00, 0xc4]);
    a.name_raw = Some(vec![0xfe, 0x80]);
    let id = c.insert_asset(&a).unwrap();
    let got = c.asset(id).unwrap().unwrap();
    assert_eq!(got.path_raw, a.path_raw);
    assert_eq!(got.name_raw, a.name_raw);
    assert!(c.insert_asset(&asset("u1", "other")).is_err());
    c.insert_asset(&asset("u2", "same")).unwrap();
    let mut uni = asset("u3", "päth/日本語/🎞");
    uni.name = "名前".into();
    let id = c.insert_asset(&uni).unwrap();
    assert_eq!(c.asset(id).unwrap().unwrap().path, "päth/日本語/🎞");
}

#[test]
fn generation_bump() {
    let c = Catalog::open_in_memory().unwrap();
    assert_eq!(c.generation().unwrap(), None);
    assert!(c.bump_generation().is_err());
    c.insert_catalogue("id", "2026-10-10T00:00:00Z").unwrap();
    assert_eq!(c.generation().unwrap(), Some(0));
    assert_eq!(c.bump_generation().unwrap(), 1);
    assert_eq!(c.bump_generation().unwrap(), 2);
    assert_eq!(c.generation().unwrap(), Some(2));
}

#[test]
fn migrations_apply_once_and_idempotent() {
    let p = tmp("mig");
    {
        let c = Catalog::open(&p).unwrap();
        assert_eq!(c.schema_version().unwrap(), 2);
        assert_eq!(count(&c, "schema_migrations"), 2);
    }
    let c = Catalog::open(&p).unwrap();
    assert_eq!(c.schema_version().unwrap(), 2);
    assert_eq!(count(&c, "schema_migrations"), 2);
    drop(c);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn invalid_and_newer_versions_refused() {
    let p = tmp("neg");
    {
        let conn = rusqlite::Connection::open(&p).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_utc TEXT NOT NULL);
             INSERT INTO schema_migrations VALUES (-5, 'x');",
        )
        .unwrap();
    }
    assert!(matches!(
        Catalog::open(&p),
        Err(CatalogError::InvalidVersion(-5))
    ));
    let _ = std::fs::remove_file(&p);

    let p = tmp("newer");
    {
        let c = Catalog::open(&p).unwrap();
        c.connection()
            .execute(
                "INSERT INTO schema_migrations (version, applied_utc) VALUES (3, 'x')",
                [],
            )
            .unwrap();
    }
    assert!(matches!(
        Catalog::open(&p),
        Err(CatalogError::NewerSchema {
            found: 3,
            supported: 2
        })
    ));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn garbage_file_is_error_not_panic() {
    let p = tmp("garbage");
    let junk: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    std::fs::write(&p, junk).unwrap();
    assert!(Catalog::open(&p).is_err());
    std::fs::write(&p, b"SQLite format 3\0truncated").unwrap();
    assert!(Catalog::open(&p).is_err());
    let _ = std::fs::remove_file(&p);
}
