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
