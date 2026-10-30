#![allow(clippy::unwrap_used, clippy::panic)]
use icatalog_catalog::{AssetQuery, Catalog, SmartRules};

fn fixture() -> Catalog {
    let c = Catalog::open_in_memory().unwrap();
    c.insert_catalogue("smart-test", "t").unwrap();
    c.connection().execute_batch("INSERT INTO asset(id,uid,name,path,format) VALUES
        (1,'one','One.jpg','/synthetic/One.jpg','JPEG'),(2,'two','Two.nef','/synthetic/Two.nef','NEF'),(3,'three','Three.jpg','/synthetic/Three.jpg','JPEG');
        INSERT INTO annotation(asset_id,rating,label) VALUES (1,5,2),(2,4,2);
        INSERT INTO asset_meta(asset_id,captured_local,make,model,lens) VALUES
        (1,'2024-02-29T23:59:59','Example','Camera','24-70 mm'),(2,'2024-03-01T00:00:00','Other','Camera','50 mm');
        INSERT INTO keyword(id,name) VALUES (1,'Keep'); INSERT INTO asset_keyword(asset_id,keyword_id) VALUES (1,1),(2,1);").unwrap();
    c
}

fn smart(c: &Catalog, rules: SmartRules) -> i64 {
    let id = c.insert_collection("Smart").unwrap();
    c.set_smart_rules(id, &rules).unwrap();
    id
}

#[test]
fn live_rules_counts_memberships_union_and_paging_share_the_same_results() {
    let c = fixture();
    let id = smart(
        &c,
        SmartRules {
            format: Some("jpeg".into()),
            captured_from: Some("2024-02-29".into()),
            captured_to: Some("2024-02-29".into()),
            camera: Some("example camera".into()),
            lens: Some("70".into()),
            min_rating: Some(5),
            label: Some(2),
            keyword: Some("Keep".into()),
            text: Some("ONE".into()),
        },
    );
    assert_eq!(c.collection_assets(id).unwrap(), vec![1]);
    assert_eq!(c.collection_asset_counts().unwrap(), vec![(id, 1)]);
    assert_eq!(c.asset_collection_ids(1).unwrap(), vec![id]);
    assert_eq!(c.collection_memberships(&[3, 1, 1]).unwrap(), vec![(1, id)]);
    let album = c.insert_collection("Manual").unwrap();
    c.add_to_collection(album, 1).unwrap();
    c.add_to_collection(album, 2).unwrap();
    let q = AssetQuery {
        collection_ids: Some(vec![id, album, id]),
        limit: Some(1),
        offset: Some(1),
        ..Default::default()
    };
    assert_eq!(
        c.query_assets(&q)
            .unwrap()
            .iter()
            .map(|a| a.id)
            .collect::<Vec<_>>(),
        vec![2]
    );
    let q = AssetQuery {
        collection_id: Some(id),
        text_like: Some("absent".into()),
        ..Default::default()
    };
    assert!(c.query_assets(&q).unwrap().is_empty());
    c.connection()
        .execute("UPDATE annotation SET rating=3 WHERE asset_id=1", [])
        .unwrap();
    assert!(c.collection_assets(id).unwrap().is_empty());
    assert!(c.asset_collection_ids(1).unwrap().contains(&album));
    assert_eq!(
        c.collection_asset_counts()
            .unwrap()
            .into_iter()
            .find(|(i, _)| *i == id),
        Some((id, 0))
    );
    assert!(c.add_to_collection(id, 2).is_err());
    assert!(c.remove_from_collection(id, 1).is_err());
    assert!(c
        .connection()
        .execute("INSERT INTO collection_asset VALUES (?1,2)", [id])
        .is_err());
    c.connection()
        .execute("DELETE FROM collection WHERE id=?1", [id])
        .unwrap();
    assert!(c.smart_rules(id).unwrap().is_none());
    assert_eq!(c.stats().unwrap().assets, 3);
}

#[test]
fn missing_metadata_zero_annotations_and_literal_text_are_explicit() {
    let c = fixture();
    let id = smart(
        &c,
        SmartRules {
            label: Some(0),
            ..Default::default()
        },
    );
    assert_eq!(c.collection_assets(id).unwrap(), vec![3]);
    for rules in [
        SmartRules {
            min_rating: Some(0),
            ..Default::default()
        },
        SmartRules {
            format: Some("JPEG".into()),
            ..Default::default()
        },
    ] {
        c.set_smart_rules(id, &rules).unwrap();
        assert!(c.collection_assets(id).unwrap().contains(&3));
    }
    for rules in [
        SmartRules {
            lens: Some("mm".into()),
            ..Default::default()
        },
        SmartRules {
            captured_to: Some("2025-01-01".into()),
            ..Default::default()
        },
    ] {
        c.set_smart_rules(id, &rules).unwrap();
        assert_eq!(c.collection_assets(id).unwrap(), vec![1, 2]);
    }
    for text in ["%", "_", "' OR 1=1 --", "🦀"] {
        c.set_smart_rules(
            id,
            &SmartRules {
                text: Some(text.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(c.collection_assets(id).unwrap().is_empty());
    }
    c.set_smart_rules(
        id,
        &SmartRules {
            keyword: Some("keep".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(c.collection_assets(id).unwrap().is_empty()); // exact keyword case
}

#[test]
fn individual_rules_follow_new_assets_metadata_and_keywords() {
    let c = fixture();
    let id = smart(
        &c,
        SmartRules {
            format: Some("NEF".into()),
            ..Default::default()
        },
    );
    for (rules, expected) in [
        (
            SmartRules {
                camera: Some("OTHER".into()),
                ..Default::default()
            },
            vec![2],
        ),
        (
            SmartRules {
                lens: Some("50".into()),
                ..Default::default()
            },
            vec![2],
        ),
        (
            SmartRules {
                format: Some("NEF".into()),
                ..Default::default()
            },
            vec![2],
        ),
        (
            SmartRules {
                captured_to: Some("2024-02-29".into()),
                ..Default::default()
            },
            vec![1],
        ),
        (
            SmartRules {
                captured_from: Some("2024-03-01".into()),
                ..Default::default()
            },
            vec![2],
        ),
        (
            SmartRules {
                label: Some(2),
                ..Default::default()
            },
            vec![1, 2],
        ),
        (
            SmartRules {
                min_rating: Some(5),
                ..Default::default()
            },
            vec![1],
        ),
        (
            SmartRules {
                keyword: Some("Keep".into()),
                ..Default::default()
            },
            vec![1, 2],
        ),
        (
            SmartRules {
                text: Some("TWO.NEF".into()),
                ..Default::default()
            },
            vec![2],
        ),
    ] {
        assert_eq!(
            c.count_smart_matches(&rules).unwrap(),
            expected.len() as i64
        );
        c.set_smart_rules(id, &rules).unwrap();
        assert_eq!(c.collection_assets(id).unwrap(), expected);
    }
    c.set_smart_rules(
        id,
        &SmartRules {
            lens: Some("50".into()),
            ..Default::default()
        },
    )
    .unwrap();
    c.connection()
        .execute_batch("INSERT INTO asset_meta(asset_id,lens) VALUES (3,'50 mm');")
        .unwrap();
    assert_eq!(c.collection_assets(id).unwrap(), vec![2, 3]);
    c.connection()
        .execute("UPDATE asset_meta SET lens=NULL WHERE asset_id=2", [])
        .unwrap();
    assert_eq!(c.collection_assets(id).unwrap(), vec![3]);
    c.set_smart_rules(
        id,
        &SmartRules {
            format: Some("JPEG".into()),
            ..Default::default()
        },
    )
    .unwrap();
    c.connection().execute_batch("INSERT INTO asset(id,uid,name,path,format) VALUES(4,'four','Four','/synthetic/Four','jpeg');").unwrap();
    assert_eq!(c.collection_assets(id).unwrap(), vec![1, 3, 4]);
    c.set_smart_rules(
        id,
        &SmartRules {
            keyword: Some("Keep".into()),
            ..Default::default()
        },
    )
    .unwrap();
    c.add_asset_keyword(4, 1).unwrap();
    c.remove_asset_keyword(1, 1).unwrap();
    assert_eq!(c.collection_assets(id).unwrap(), vec![2, 4]);
}

#[test]
fn rule_validation_is_bounded_and_rejects_invalid_dates_and_ranges() {
    for date in [
        "",
        "0000-01-01",
        "2023-02-29",
        "1900-02-29",
        "2024-04-31",
        "2024-13-01",
        "2024-01-00",
        "2024-1-01",
        "2024-01-01T00:00:00",
        "😺-01-01",
        "999999999999999999",
    ] {
        assert!(
            SmartRules {
                captured_from: Some(date.into()),
                ..Default::default()
            }
            .validate()
            .is_err(),
            "{date}"
        );
    }
    for date in ["2000-02-29", "2024-02-29", "2024-12-31"] {
        assert!(SmartRules {
            captured_from: Some(date.into()),
            ..Default::default()
        }
        .validate()
        .is_ok());
    }
    for rules in [
        SmartRules::default(),
        SmartRules {
            text: Some("x".repeat(1025)),
            ..Default::default()
        },
        SmartRules {
            text: Some("bad\0text".into()),
            ..Default::default()
        },
        SmartRules {
            min_rating: Some(i64::MAX),
            ..Default::default()
        },
        SmartRules {
            label: Some(-1),
            ..Default::default()
        },
        SmartRules {
            captured_from: Some("2025-01-01".into()),
            captured_to: Some("2024-01-01".into()),
            ..Default::default()
        },
    ] {
        assert!(rules.validate().is_err());
    }
}

#[test]
fn migration_from_v5_preserves_manual_membership_and_asset_identity() {
    // Construct the actual preceding schema, then use the normal opener.
    let path = std::env::temp_dir().join(format!(
        "icat-smart-migration-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,applied_utc TEXT NOT NULL);").unwrap();
        for (index, sql) in [
            include_str!("../src/v1.sql"),
            include_str!("../src/v2.sql"),
            include_str!("../src/v3.sql"),
            include_str!("../src/v4.sql"),
            include_str!("../src/v5.sql"),
        ]
        .into_iter()
        .enumerate()
        {
            c.execute_batch(sql).unwrap();
            c.execute("INSERT INTO schema_migrations VALUES(?1,'t')", [index + 1])
                .unwrap();
        }
        c.execute_batch("INSERT INTO catalogue VALUES ('legacy',0,'t',5); INSERT INTO asset(id,uid,name,path) VALUES(1,'stable','One','/one'); INSERT INTO collection VALUES(1,'Manual'); INSERT INTO collection_asset VALUES(1,1);").unwrap();
    }
    let c = Catalog::open(&path).unwrap();
    assert_eq!(c.schema_version().unwrap(), 6);
    assert_eq!(c.collection_assets(1).unwrap(), vec![1]);
    assert_eq!(c.asset(1).unwrap().unwrap().uid, "stable");
    assert!(c.collection(1).unwrap().unwrap().rules.is_none());
    let rules = SmartRules {
        text: Some("One".into()),
        ..Default::default()
    };
    let id = smart(&c, rules.clone());
    drop(c);
    let c = Catalog::open(&path).unwrap();
    assert_eq!(c.smart_rules(id).unwrap(), Some(rules));
    assert_eq!(c.collection_assets(id).unwrap(), vec![1]);
    drop(c);
    let _ = std::fs::remove_file(path);
}
