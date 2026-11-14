#![allow(clippy::unwrap_used, clippy::panic)]

use icatalog_engine::{dispatch, Context, Params};

#[test]
fn read_only_context_does_not_migrate_or_write_catalogue_bytes() {
    let path =
        std::env::temp_dir().join(format!("icatalog-read-only-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut writable = Context::open(&path).unwrap();
    dispatch(&mut writable, "init", &Params::new()).unwrap();
    drop(writable);

    let before = std::fs::read(&path).unwrap();
    let readonly = Context::open_read_only(&path).unwrap();
    assert!(readonly.is_read_only());
    assert!(readonly.catalog().catalogue().unwrap().is_some());
    assert!(readonly
        .catalog()
        .connection()
        .execute("UPDATE catalogue SET generation=generation+1", [])
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);

    let _ = std::fs::remove_file(path);
}
