use icatalog_catalog::{Annotation, Catalog, SmartRules};
fn fixture() -> Catalog {
    let c = Catalog::open_in_memory().unwrap();
    c.insert_catalogue("revision", "t").unwrap();
    c.connection().execute_batch("INSERT INTO asset(id,uid,path,name) VALUES(1,'one','/one','One'); INSERT INTO collection(id,name) VALUES(1,'Album');").unwrap();
    c
}
#[test]
fn generation_and_revisions_are_transactional_and_include_keyword_and_collection_edits() {
    let c = fixture();
    let generation = c.generation().unwrap();
    let a = c.annotation_states().unwrap();
    let collection = c.collection_states().unwrap();
    c.connection().execute_batch("BEGIN; INSERT INTO annotation(asset_id,rating) VALUES(1,5); INSERT INTO keyword(id,name) VALUES(1,'Trip'); INSERT INTO asset_keyword VALUES(1,1); INSERT INTO collection_asset VALUES(1,1);").unwrap();
    assert!(c.generation().unwrap() > generation);
    assert!(c.annotation_states().unwrap()[0].revision > a[0].revision);
    assert!(c.collection_states().unwrap()[0].revision > collection[0].revision);
    c.connection().execute_batch("ROLLBACK").unwrap();
    assert_eq!(c.generation().unwrap(), generation);
    assert_eq!(c.annotation_states().unwrap(), a);
    assert_eq!(c.collection_states().unwrap(), collection);
    c.rename_collection(1, "Renamed").unwrap();
    let r = c.collection_states().unwrap()[0].revision;
    c.set_smart_rules(
        1,
        &SmartRules {
            min_rating: Some(4),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(c.collection_states().unwrap()[0].revision > r);
    let g = c.generation().unwrap();
    c.connection()
        .execute("UPDATE asset SET name='Changed' WHERE id=1", [])
        .unwrap();
    assert!(c.generation().unwrap() > g);
}
#[test]
fn replica_guards_preserve_server_media_and_allow_user_annotations_and_collections() {
    let c = fixture();
    c.set_replica_base(0, b"synthetic baseline").unwrap();
    assert!(c
        .connection()
        .execute("UPDATE asset SET path='/elsewhere'", [])
        .is_err());
    assert!(c.insert_folder(None, "No").is_err());
    assert!(c.set_preview_mode("external").is_err());
    c.set_annotation(&Annotation {
        asset_id: 1,
        rating: 5,
        label: 2,
        caption: None,
        headline: None,
        creator: None,
        copyright: None,
        updated_utc: None,
    })
    .unwrap();
    c.rename_collection(1, "Allowed").unwrap();
    c.add_to_collection(1, 1).unwrap();
    assert_eq!(c.collection_assets(1).unwrap(), vec![1]);
}
#[test]
fn overflow_is_an_error_and_rolls_back_the_triggering_write() {
    let c = fixture();
    c.set_generation(i64::MAX).unwrap();
    assert!(c.rename_collection(1, "Overflow").is_err());
    assert_eq!(c.collection(1).unwrap().unwrap().name, "Album");
    assert_eq!(c.generation().unwrap(), Some(i64::MAX));
    c.set_generation(1).unwrap();
    c.connection()
        .execute(
            "UPDATE sync_revision SET revision=?1 WHERE kind='asset'",
            [i64::MAX],
        )
        .unwrap();
    assert!(c
        .connection()
        .execute("INSERT INTO annotation(asset_id,rating) VALUES(1,5)", [])
        .is_err());
    assert!(c.annotation(1).unwrap().is_none());
}
#[test]
fn migration_from_v6_is_repeatable_and_existing_collection_uids_are_deterministic() {
    let dir = std::env::temp_dir().join(format!("icat-v6-revisions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.sqlite");
    let b = dir.join("b.sqlite");
    {
        let c = rusqlite::Connection::open(&a).unwrap();
        c.execute_batch(
            "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,applied_utc TEXT NOT NULL)",
        )
        .unwrap();
        for (i, sql) in [
            include_str!("../src/v1.sql"),
            include_str!("../src/v2.sql"),
            include_str!("../src/v3.sql"),
            include_str!("../src/v4.sql"),
            include_str!("../src/v5.sql"),
            include_str!("../src/v6.sql"),
        ]
        .into_iter()
        .enumerate()
        {
            c.execute_batch(sql).unwrap();
            c.execute("INSERT INTO schema_migrations VALUES(?1,'t')", [i + 1])
                .unwrap();
        }
        c.execute_batch("INSERT INTO catalogue VALUES('shared',9,'t',6); INSERT INTO asset(uid,name,path) VALUES('stable','One','/one'); INSERT INTO collection(name) VALUES('Existing'); INSERT INTO collection_asset VALUES(1,1);").unwrap();
    }
    std::fs::copy(&a, &b).unwrap();
    let first = Catalog::open(&a).unwrap();
    let second = Catalog::open(&b).unwrap();
    assert_eq!(
        first.collection_states().unwrap(),
        second.collection_states().unwrap()
    );
    assert_eq!(first.collection_assets(1).unwrap(), vec![1]);
    let uid = first.collection_states().unwrap()[0].uid.clone();
    drop(first);
    assert_eq!(
        Catalog::open(&a).unwrap().collection_states().unwrap()[0].uid,
        uid
    );
}
