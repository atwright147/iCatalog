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
        "INSERT INTO asset(id,uid,path,name) VALUES (7,'synthetic','/synthetic/Photo.jpg','Photo.jpg');"
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
    for args in [
        vec!["collection", p],
        vec!["collection", p, "list", "extra"],
        vec!["collection", p, "add", "--id", "1", "--asset", "999"],
    ] {
        assert!(!icatalog(&args).status.success());
    }
    let _ = std::fs::remove_file(path);
}
