#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::process::{Command, Output};

fn icatalog(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_icatalog"))
        .args(args)
        .output()
        .unwrap()
}

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn version_and_help() {
    let o = icatalog(&["--version"]);
    assert!(o.status.success());
    assert!(s(&o.stdout).contains("icatalog"));
    let o = icatalog(&["--help"]);
    assert!(o.status.success());
    let out = s(&o.stdout);
    assert!(out.contains("Usage") && out.contains("init"));
    // The sync registry is merged into the shell's listing.
    assert!(out.contains("sync") && out.contains("serve"));
}

#[test]
fn commands_lists_sync_and_serve() {
    let o = icatalog(&["commands"]);
    assert!(o.status.success());
    let out = s(&o.stdout);
    assert!(out.contains("sync") && out.contains("serve"), "{out}");
}

#[test]
fn sync_requires_url_and_serve_requires_port() {
    let path =
        std::env::temp_dir().join(format!("icatalog-cli-sync-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_str().unwrap();
    assert!(icatalog(&["init", p]).status.success());

    let o = icatalog(&["sync", p]);
    assert!(!o.status.success());
    assert!(s(&o.stderr).contains("url"));

    let o = icatalog(&["serve", p]);
    assert!(!o.status.success());
    assert!(s(&o.stderr).contains("port"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn unknown_command_fails() {
    let o = icatalog(&["frobnicate"]);
    assert!(!o.status.success());
    assert!(s(&o.stderr).contains("unknown command"));
}

#[test]
fn end_to_end() {
    let path = std::env::temp_dir().join(format!("icatalog-cli-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_str().unwrap();
    assert!(icatalog(&["init", p]).status.success());
    let o = icatalog(&["status", p]);
    assert!(o.status.success());
    assert!(s(&o.stdout).contains("assets"));
    assert!(icatalog(&["list", p]).status.success());
    let o = icatalog(&["find", p]);
    assert!(!o.status.success());
    assert!(s(&o.stderr).contains("query"));
    let o = icatalog(&["find", p, "x", "--rating", "abc"]);
    assert!(!o.status.success());
    assert!(!o.stderr.is_empty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn collection_cli_round_trip_and_argument_errors() {
    let path =
        std::env::temp_dir().join(format!("icat-cli-collection-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_str().unwrap();
    assert!(icatalog(&["init", p]).status.success());
    let ctx = icatalog_engine::Context::open(&path).unwrap();
    ctx.catalog().connection().execute_batch(
        "INSERT INTO asset(id,uid,path,name) VALUES (7,'synthetic','/synthetic/Photo.jpg','Photo.jpg'), (8,'other','/synthetic/Other.jpg','Other.jpg');"
    ).unwrap();
    drop(ctx);
    let created = icatalog(&["collection", p, "create", "--name", "Holiday favourites"]);
    assert!(created.status.success(), "{}", s(&created.stderr));
    assert!(s(&created.stdout).contains("Holiday favourites"));
    assert!(s(&icatalog(&["collection", p, "list"]).stdout).contains("Holiday favourites"));
    for _ in 0..2 {
        assert!(
            icatalog(&["collection", p, "add", "--id", "1", "--asset", "7"])
                .status
                .success()
        );
    }
    for args in [
        vec!["collection", p, "show", "--id", "1"],
        vec!["list", p, "--collection", "1"],
        vec!["find", p, "Photo", "--collection", "1"],
    ] {
        let out = icatalog(&args);
        assert!(out.status.success(), "{}", s(&out.stderr));
        assert!(s(&out.stdout).contains("Photo.jpg"));
    }
    assert!(
        icatalog(&["collection", p, "remove", "--id", "1", "--asset", "7"])
            .status
            .success()
    );
    assert!(!s(&icatalog(&["collection", p, "show", "--id", "1"]).stdout).contains("Photo.jpg"));
    assert!(
        icatalog(&["collection", p, "add", "--id", "1", "--assets", "7,8"])
            .status
            .success()
    );
    let members = icatalog(&["collection", p, "show", "--id", "1"]);
    assert!(s(&members.stdout).contains("Photo.jpg") && s(&members.stdout).contains("Other.jpg"));
    assert!(
        icatalog(&["collection", p, "remove", "--id", "1", "--assets", "7,8"])
            .status
            .success()
    );
    assert!(!s(&icatalog(&["collection", p, "show", "--id", "1"]).stdout).contains(".jpg"));
    for args in [
        vec!["collection", p],
        vec!["collection", p, "list", "extra"],
        vec!["collection", p, "add", "--id", "1", "--asset", "999"],
    ] {
        assert!(!icatalog(&args).status.success());
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn smart_collection_cli_create_browse_rules_edit_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart.sqlite");
    let p = path.to_str().unwrap();
    assert!(icatalog(&["init", p]).status.success());
    let c = icatalog_engine::Context::open(&path).unwrap();
    c.catalog().connection().execute_batch("INSERT INTO asset(id,uid,name,path,format) VALUES(1,'one','One.jpg','/one.jpg','JPEG'),(2,'two','Two.nef','/two.nef','NEF');").unwrap();
    drop(c);
    let preview = icatalog(&["collection", p, "preview-smart", "--format", "jpeg"]);
    assert!(preview.status.success(), "{}", s(&preview.stderr));
    assert!(s(&preview.stdout).contains("1 matching item\nFormat is \"jpeg\""));
    let o = icatalog(&[
        "collection",
        p,
        "create-smart",
        "--name",
        "JPEGs",
        "--format",
        "jpeg",
    ]);
    assert!(o.status.success(), "{}", s(&o.stderr));
    let list = icatalog(&["collection", p, "list"]);
    assert!(s(&list.stdout).contains("smart"));
    for args in [
        vec!["collection", p, "show", "--id", "1"],
        vec!["list", p, "--collection", "1"],
    ] {
        let o = icatalog(&args);
        assert!(o.status.success());
        let text = s(&o.stdout);
        assert!(text.contains("One.jpg"));
        assert!(!text.contains("Two.nef"));
    }
    let o = icatalog(&[
        "collection",
        p,
        "set-rules",
        "--id",
        "1",
        "--format",
        "NEF",
        "--name",
        "RAW files",
    ]);
    assert!(o.status.success(), "{}", s(&o.stderr));
    let o = icatalog(&["collection", p, "rules", "--id", "1"]);
    assert!(s(&o.stdout).contains("NEF"));
    let o = icatalog(&["find", p, "Two", "--collection", "1"]);
    assert!(s(&o.stdout).contains("Two.nef"));
    assert!(
        !icatalog(&["collection", p, "add", "--id", "1", "--asset", "1"])
            .status
            .success()
    );
    assert!(
        !icatalog(&["collection", p, "create-smart", "--name", "Empty"])
            .status
            .success()
    );
    let c = icatalog_engine::Context::open(&path).unwrap();
    assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![2]);
    assert_eq!(
        c.catalog().collection(1).unwrap().unwrap().name,
        "RAW files"
    );
}

#[test]
fn rename_both_collection_kinds_through_cli() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rename.sqlite");
    let p = path.to_str().unwrap();
    assert!(icatalog(&["init", p]).status.success());
    assert!(icatalog(&["collection", p, "create", "--name", "Album"])
        .status
        .success());
    assert!(icatalog(&[
        "collection",
        p,
        "create-smart",
        "--name",
        "Picks",
        "--min-rating",
        "4"
    ])
    .status
    .success());
    for id in ["1", "2"] {
        let out = icatalog(&[
            "collection",
            p,
            "rename",
            "--id",
            id,
            "--name",
            "  New name  ",
        ]);
        assert!(out.status.success(), "{}", s(&out.stderr));
        assert!(
            !icatalog(&["collection", p, "rename", "--id", id, "--name", ""])
                .status
                .success()
        );
    }
    let out = icatalog(&["collection", p, "list"]);
    let text = s(&out.stdout);
    assert!(text.contains("New name") && !text.contains("Album") && !text.contains("Picks"));
    let ctx = icatalog_engine::Context::open(&path).unwrap();
    for id in [1, 2] {
        assert_eq!(
            ctx.catalog().collection(id).unwrap().unwrap().name,
            "New name"
        );
    }
    assert_eq!(
        ctx.catalog().smart_rules(2).unwrap().unwrap().min_rating,
        Some(4)
    );
}
