#![allow(clippy::unwrap_used)]
use icatalog_catalog::{AssetQuery, Catalog, SmartRules};

#[test]
fn maximum_and_rating_ranges_share_live_membership_counts_and_preview() {
    let c = Catalog::open_in_memory().unwrap();
    c.insert_catalogue("rating-range", "t").unwrap();
    c.connection().execute_batch("INSERT INTO asset(id,uid,name,path) VALUES(1,'a','a','/a'),(2,'b','b','/b'),(3,'c','c','/c'); INSERT INTO annotation(asset_id,rating) VALUES(1,5),(2,4);").unwrap();
    let id = c.insert_collection("Range").unwrap();
    for (min, max, expected) in [
        (None, Some(4), vec![2, 3]),
        (Some(4), Some(4), vec![2]),
        (None, Some(0), vec![3]),
        (None, Some(5), vec![1, 2, 3]),
        (Some(5), None, vec![1]),
    ] {
        let rules = SmartRules {
            min_rating: min,
            max_rating: max,
            ..Default::default()
        };
        c.set_smart_rules(id, &rules).unwrap();
        assert_eq!(c.smart_rules(id).unwrap(), Some(rules.clone()));
        assert_eq!(c.collection_assets(id).unwrap(), expected);
        assert_eq!(
            c.count_smart_matches(&rules).unwrap(),
            expected.len() as i64
        );
        assert_eq!(
            c.collection_asset_counts().unwrap(),
            vec![(id, expected.len() as i64)]
        );
        let rows = c
            .query_assets(&AssetQuery {
                collection_id: Some(id),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.iter().map(|a| a.id).collect::<Vec<_>>(), expected);
    }
    c.set_smart_rules(
        id,
        &SmartRules {
            max_rating: Some(4),
            ..Default::default()
        },
    )
    .unwrap();
    c.connection()
        .execute("UPDATE annotation SET rating=3 WHERE asset_id=1", [])
        .unwrap();
    assert_eq!(c.collection_assets(id).unwrap(), vec![1, 2, 3]);
    let original = c.smart_rules(id).unwrap();
    let generation = c.generation().unwrap();
    for (min, max) in [
        (None, Some(-1)),
        (None, Some(6)),
        (Some(4), Some(3)),
        (Some(i64::MIN), Some(i64::MAX)),
    ] {
        let rules = SmartRules {
            min_rating: min,
            max_rating: max,
            ..Default::default()
        };
        assert!(c.set_smart_rules(id, &rules).is_err());
        assert!(c.count_smart_matches(&rules).is_err());
    }
    for value in ["-1", "6", "1.5", "'bad'"] {
        assert!(c
            .connection()
            .execute(
                &format!("UPDATE smart_collection SET max_rating={value} WHERE collection_id=?1"),
                [id]
            )
            .is_err());
    }
    assert!(c
        .connection()
        .execute(
            "UPDATE smart_collection SET min_rating=5,max_rating=4 WHERE collection_id=?1",
            [id]
        )
        .is_err());
    assert_eq!(c.smart_rules(id).unwrap(), original);
    assert_eq!(c.generation().unwrap(), generation);
}

fn make_v7(path: &std::path::Path) -> (i64, i64, String) {
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,applied_utc TEXT NOT NULL);").unwrap();
    for (i, sql) in [
        include_str!("../src/v1.sql"),
        include_str!("../src/v2.sql"),
        include_str!("../src/v3.sql"),
        include_str!("../src/v4.sql"),
        include_str!("../src/v5.sql"),
        include_str!("../src/v6.sql"),
        include_str!("../src/v7.sql"),
    ]
    .into_iter()
    .enumerate()
    {
        c.execute_batch(sql).unwrap();
        c.execute("INSERT INTO schema_migrations VALUES(?1,'t')", [i + 1])
            .unwrap();
    }
    c.execute_batch("INSERT INTO catalogue VALUES('v7-rating',0,'t',7); INSERT INTO asset(id,uid,name,path) VALUES(1,'a','a','/a'); INSERT INTO collection(id,name) VALUES(1,'Manual'),(2,'Smart'); INSERT INTO collection_asset VALUES(1,1); INSERT INTO smart_collection(collection_id,min_rating) VALUES(2,3); INSERT INTO replica_base VALUES(1,0,x'010203');").unwrap();
    c.query_row("SELECT generation,(SELECT revision FROM sync_revision WHERE kind='collection' AND entity_id=2),(SELECT uid FROM collection WHERE id=2) FROM catalogue", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
}

#[test]
fn v7_migration_preserves_rules_uids_revisions_members_and_replica_baseline() {
    let path = std::env::temp_dir().join(format!(
        "icat-v8-rating-migration-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let (generation, revision, uid) = make_v7(&path);
    let c = Catalog::open(&path).unwrap();
    assert_eq!(c.schema_version().unwrap(), 8);
    assert_eq!(c.generation().unwrap(), Some(generation + 1));
    assert_eq!(c.smart_rules(2).unwrap().unwrap().min_rating, Some(3));
    assert_eq!(c.smart_rules(2).unwrap().unwrap().max_rating, None);
    assert_eq!(c.collection_assets(1).unwrap(), vec![1]);
    let smart = c
        .collection_states()
        .unwrap()
        .into_iter()
        .find(|v| v.uid == uid)
        .unwrap();
    assert_eq!(smart.revision, revision);
    let baseline: Vec<u8> = c
        .connection()
        .query_row("SELECT data FROM replica_base", [], |r| r.get(0))
        .unwrap();
    assert_eq!(baseline, vec![1, 2, 3]);
    c.set_smart_rules(
        2,
        &SmartRules {
            min_rating: Some(3),
            max_rating: Some(4),
            ..Default::default()
        },
    )
    .unwrap();
    let changed = c
        .collection_states()
        .unwrap()
        .into_iter()
        .find(|v| v.uid == uid)
        .unwrap();
    assert_eq!(changed.revision, revision + 1);
    assert_eq!(c.generation().unwrap(), Some(generation + 2));
    assert!(c.add_to_collection(2, 1).is_err());
    assert!(c
        .connection()
        .execute("UPDATE collection_asset SET collection_id=2", [])
        .is_err());
    assert!(c
        .set_smart_rules(
            1,
            &SmartRules {
                max_rating: Some(5),
                ..Default::default()
            }
        )
        .is_err());
    let violations: i64 = c
        .connection()
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    let generation = c.generation().unwrap();
    drop(c);
    let c = Catalog::open(&path).unwrap();
    assert_eq!(c.generation().unwrap(), generation);
    assert_eq!(c.smart_rules(2).unwrap().unwrap().max_rating, Some(4));
    drop(c);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn failed_v8_migration_rolls_back_table_rebuild_and_can_be_retried() {
    let path = std::env::temp_dir().join(format!(
        "icat-v8-rating-rollback-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    make_v7(&path);
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute("UPDATE catalogue SET generation=?1", [i64::MAX])
            .unwrap();
    }
    assert!(Catalog::open(&path).is_err());
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        let version: i64 = c
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 7);
        let max_columns: i64 = c.query_row("SELECT count(*) FROM pragma_table_info('smart_collection') WHERE name='max_rating'", [], |r| r.get(0)).unwrap();
        assert_eq!(max_columns, 0);
        let min: i64 = c
            .query_row(
                "SELECT min_rating FROM smart_collection WHERE collection_id=2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(min, 3);
        c.execute("UPDATE catalogue SET generation=0", []).unwrap();
    }
    let c = Catalog::open(&path).unwrap();
    assert_eq!(c.schema_version().unwrap(), 8);
    assert_eq!(c.collection_assets(1).unwrap(), vec![1]);
    drop(c);
    std::fs::remove_file(path).unwrap();
}
