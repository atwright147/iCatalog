#![allow(clippy::unwrap_used, clippy::panic)]

use icatalog_catalog::{sync_state::SyncStateDb, Catalog};

fn catalogue(path: &std::path::Path, id: &str) {
    let c = Catalog::open(path).unwrap();
    c.insert_catalogue(id, "2026-11-13T00:00:00Z").unwrap();
}

#[test]
fn companion_latch_is_durable_and_bound_to_catalogue_identity() {
    let dir = std::env::temp_dir().join(format!("icat-sync-state-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("local.icatalog");
    catalogue(&path, "replica-a");
    let state = SyncStateDb::open(&path, "replica-a", true).unwrap();
    assert!(!state.required().unwrap());
    state.set_required(true).unwrap();
    drop(state);
    assert!(SyncStateDb::open(&path, "replica-a", false)
        .unwrap()
        .required()
        .unwrap());
    assert!(SyncStateDb::open(&path, "replica-b", true).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unrelated_existing_companion_is_rejected_without_modification() {
    let dir = std::env::temp_dir().join(format!("icat-sync-state-invalid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("local.sqlite");
    catalogue(&path, "replica");
    let sidecar = icatalog_catalog::sync_state::companion_path(&path).unwrap();
    let conn = rusqlite::Connection::open(&sidecar).unwrap();
    conn.execute_batch("CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('keep');")
        .unwrap();
    drop(conn);
    let before = std::fs::read(&sidecar).unwrap();
    assert!(SyncStateDb::open(&path, "replica", true).is_err());
    assert_eq!(std::fs::read(&sidecar).unwrap(), before);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn companion_symlink_is_rejected() {
    use std::os::unix::fs::symlink;
    let dir = std::env::temp_dir().join(format!("icat-sync-state-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("local.sqlite");
    catalogue(&path, "replica");
    let link = icatalog_catalog::sync_state::companion_path(&path).unwrap();
    let target = dir.join("target.sqlite");
    std::fs::write(&target, b"keep").unwrap();
    symlink(&target, &link).unwrap();
    assert!(SyncStateDb::open(&path, "replica", true).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    let _ = std::fs::remove_dir_all(dir);
}
