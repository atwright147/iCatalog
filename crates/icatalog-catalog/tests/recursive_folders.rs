use icatalog_catalog::{AssetQuery, Catalog};

fn fixture() -> Catalog {
    let c = Catalog::open_in_memory().unwrap();
    c.insert_catalogue("folders", "2026-01-01T00:00:00Z")
        .unwrap();
    c.connection()
        .execute_batch(
            "INSERT INTO source(id,label,identity) VALUES (1,'Photos','/one'),(2,'Photos','/two');
             INSERT INTO folder(id,parent_id,source_id,path,name) VALUES
             (1,NULL,1,'','Photos'),(2,1,1,'Trips','Trips'),
             (3,2,1,'Trips/Beach','Beach'),(4,1,1,'Trips-old','Trips-old'),
             (5,NULL,2,'','Photos'),(6,NULL,NULL,NULL,'Empty parent'),
             (7,6,NULL,NULL,'Child'),(8,NULL,NULL,NULL,'Empty leaf');
             INSERT INTO asset(id,uid,folder_id,path,name) VALUES
             (1,'root',1,'/one/root.jpg','root.jpg'),
             (2,'child',2,'/one/Trips/keep.jpg','keep.jpg'),
             (3,'grandchild',3,'/unrelated/path/keep.jpg','keep.jpg'),
             (4,'sibling',4,'/one/Trips-old/keep.jpg','keep.jpg'),
             (5,'other-source',5,'/one/Trips/keep.jpg','keep.jpg'),
             (6,'unassigned',NULL,'/one/Trips/loose.jpg','loose.jpg'),
             (7,'empty-parent-child',7,'/manual/keep.jpg','keep.jpg');
             INSERT INTO annotation(asset_id,rating,label) VALUES (2,4,2),(3,5,2),(4,5,2);
             INSERT INTO keyword(id,name) VALUES (1,'holiday');
             INSERT INTO asset_keyword(asset_id,keyword_id) VALUES (2,1),(3,1),(4,1);
             INSERT INTO asset_meta(asset_id,captured_local) VALUES
             (2,'2026-01-02'),(3,'2026-01-03'),(4,'2026-01-04');
             INSERT INTO collection(id,name) VALUES (1,'Favourites');
             INSERT INTO collection_asset(collection_id,asset_id) VALUES (1,2),(1,3),(1,4);",
        )
        .unwrap();
    c
}

fn query(folder_id: i64) -> AssetQuery {
    AssetQuery {
        folder_id: Some(folder_id),
        folder_recursive: true,
        ..Default::default()
    }
}

fn ids(c: &Catalog, q: &AssetQuery) -> Vec<i64> {
    c.query_assets(q)
        .unwrap()
        .into_iter()
        .map(|a| a.id)
        .collect()
}

#[test]
fn recursive_folders_follow_parent_links_and_keep_exact_queries_available() {
    let c = fixture();
    assert_eq!(ids(&c, &query(1)), vec![1, 2, 3, 4]);
    // Similar path prefixes, identical names and another source are not descendants.
    assert_eq!(ids(&c, &query(2)), vec![2, 3]);
    assert_eq!(ids(&c, &query(3)), vec![3]);
    assert_eq!(ids(&c, &query(5)), vec![5]);
    // A folder with no direct assets can still contain assets below it.
    assert_eq!(ids(&c, &query(6)), vec![7]);
    for id in [8, 999, i64::MIN, i64::MAX] {
        assert!(ids(&c, &query(id)).is_empty());
    }
    assert_eq!(
        ids(
            &c,
            &AssetQuery {
                folder_recursive: false,
                ..query(1)
            }
        ),
        vec![1]
    );
    assert_eq!(
        ids(
            &c,
            &AssetQuery {
                folder_id_null: Some(false),
                ..query(2)
            }
        ),
        vec![2, 3]
    );
    assert_eq!(
        ids(
            &c,
            &AssetQuery {
                folder_id_null: Some(true),
                ..query(1)
            }
        ),
        vec![6]
    );
    assert_eq!(
        ids(
            &c,
            &AssetQuery {
                folder_recursive: true,
                ..Default::default()
            }
        ),
        vec![1, 2, 3, 4, 5, 6, 7]
    );
}

#[test]
fn recursive_folder_filters_apply_before_paging_and_compose_with_other_filters() {
    let c = fixture();
    let mut q = AssetQuery {
        text_like: Some("KEEP".into()),
        keyword: Some("holiday".into()),
        min_rating: Some(4),
        label: Some(2),
        captured_from: Some("2026-01-01".into()),
        captured_to: Some("2026-01-31".into()),
        collection_id: Some(1),
        source_ids: Some(vec![1]),
        ..query(2)
    };
    assert_eq!(ids(&c, &q), vec![2, 3]);
    q.limit = Some(1);
    q.offset = Some(1);
    assert_eq!(ids(&c, &q), vec![3]);
    q.offset = None;
    q.min_rating = Some(5);
    assert_eq!(ids(&c, &q), vec![3]);
    q.source_ids = Some(vec![2]);
    assert!(ids(&c, &q).is_empty());
}

#[test]
fn recursive_folders_terminate_on_cycles_without_duplicate_assets() {
    let c = fixture();
    c.connection()
        .execute_batch("UPDATE folder SET parent_id=3 WHERE id=1;")
        .unwrap();
    assert_eq!(ids(&c, &query(1)), vec![1, 2, 3, 4]);
    assert_eq!(ids(&c, &query(2)), vec![1, 2, 3, 4]);
    c.connection()
        .execute_batch("UPDATE folder SET parent_id=id WHERE id=5;")
        .unwrap();
    assert_eq!(ids(&c, &query(5)), vec![5]);
    let mut q = query(1);
    q.limit = Some(2);
    q.offset = Some(1);
    assert_eq!(ids(&c, &q), vec![2, 3]);
}

#[test]
fn recursive_folders_support_deep_trees_without_rust_recursion() {
    let c = Catalog::open_in_memory().unwrap();
    c.connection()
        .execute_batch(
            "WITH RECURSIVE n(id) AS (SELECT 1 UNION ALL SELECT id+1 FROM n WHERE id<2000)
             INSERT INTO folder(id,parent_id,name)
             SELECT id,CASE WHEN id=1 THEN NULL ELSE id-1 END,'Folder' FROM n;
             INSERT INTO asset(uid,folder_id,path,name) VALUES ('deep',2000,'/deep.jpg','deep.jpg');",
        )
        .unwrap();
    assert_eq!(ids(&c, &query(1)), vec![1]);
    assert_eq!(ids(&c, &query(2000)), vec![1]);
}
