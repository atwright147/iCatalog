#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::process::{Command, Output};

fn icatalog(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_icatalog"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn init_treats_sqlite_uri_syntax_as_a_literal_filename() {
    let dir = tempfile::tempdir().unwrap();
    let name = "file:requested.sqlite?mode=memory";
    let created = Command::new(env!("CARGO_BIN_EXE_icatalog"))
        .current_dir(dir.path())
        .args(["init", name])
        .output()
        .unwrap();
    assert!(created.status.success(), "{}", s(&created.stderr));
    assert!(dir.path().join(name).is_file());
    assert!(!dir.path().join("requested.sqlite").exists());
    let reopened = Command::new(env!("CARGO_BIN_EXE_icatalog"))
        .current_dir(dir.path())
        .args(["status", name])
        .output()
        .unwrap();
    assert!(reopened.status.success(), "{}", s(&reopened.stderr));
    for special in [":memory:", ""] {
        let output = Command::new(env!("CARGO_BIN_EXE_icatalog"))
            .current_dir(dir.path())
            .args(["init", special])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{special}");
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_catalogue_path_is_byte_exact_and_text_arguments_fail_cleanly() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(OsString::from_vec(b"library-\xff.sqlite".to_vec()));
    let binary = env!("CARGO_BIN_EXE_icatalog");
    // Linux usually accepts arbitrary filename bytes; macOS APFS may not.
    // Neither outcome may panic or create a replacement-character catalogue.
    let supported = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => {
            drop(file);
            std::fs::remove_file(&path).unwrap();
            true
        }
        Err(_) => false,
    };
    let init = Command::new(binary)
        .arg("init")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!s(&init.stderr).contains("panicked"));
    if supported {
        assert!(init.status.success(), "{}", s(&init.stderr));
        let status = Command::new(binary)
            .arg("status")
            .arg(&path)
            .output()
            .unwrap();
        assert!(status.status.success(), "{}", s(&status.stderr));
        assert!(s(&status.stdout).contains("assets"));
    } else {
        assert!(!init.status.success());
        assert!(s(&init.stderr).contains("non-UTF-8 bytes"));
        assert!(!path.exists());
    }
    assert!(!dir.path().join("library-�.sqlite").exists());

    let invalid = OsString::from_vec(b"bad-\xff".to_vec());
    let valid_path = dir.path().join("valid.sqlite");
    assert!(Command::new(binary)
        .arg("init")
        .arg(&valid_path)
        .output()
        .unwrap()
        .status
        .success());
    let bad_value = Command::new(binary)
        .arg("list")
        .arg(&valid_path)
        .arg("--limit")
        .arg(&invalid)
        .output()
        .unwrap();
    assert_eq!(bad_value.status.code(), Some(2));
    assert!(s(&bad_value.stderr).contains("UTF-8"));
    assert!(!s(&bad_value.stderr).contains("panicked"));

    let bad_source = Command::new(binary)
        .arg("import")
        .arg(&valid_path)
        .arg("--path")
        .arg(&invalid)
        .output()
        .unwrap();
    assert_eq!(bad_source.status.code(), Some(2));
    assert!(s(&bad_source.stderr).contains("option '--path' must be valid UTF-8"));
    assert!(!s(&bad_source.stderr).contains("panicked"));

    let bad_positional = Command::new(binary)
        .arg("find")
        .arg(&valid_path)
        .arg(&invalid)
        .output()
        .unwrap();
    assert_eq!(bad_positional.status.code(), Some(2));
    assert!(s(&bad_positional.stderr).contains("valid UTF-8"));
    assert!(!s(&bad_positional.stderr).contains("panicked"));

    let bad_command = Command::new(binary).arg(invalid).output().unwrap();
    assert_eq!(bad_command.status.code(), Some(2));
    assert!(s(&bad_command.stderr).contains("UTF-8"));
    assert!(!s(&bad_command.stderr).contains("panicked"));
}

#[test]
fn maximum_rating_cli_previews_saves_and_rejects_inverted_ranges() {
    let path =
        std::env::temp_dir().join(format!("icat-cli-max-rating-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_str().unwrap();
    assert!(icatalog(&["init", p]).status.success());
    {
        let c = icatalog_engine::Context::open(&path).unwrap();
        c.catalog().connection().execute_batch("INSERT INTO asset(id,uid,name,path) VALUES(1,'a','a','/a'),(2,'b','b','/b'); INSERT INTO annotation(asset_id,rating) VALUES(1,5);").unwrap();
    }
    let preview = icatalog(&["collection", p, "preview-smart", "--max-rating", "0"]);
    assert!(preview.status.success(), "{}", s(&preview.stderr));
    assert!(s(&preview.stdout).contains("1 matching item\nRating is at most 0 out of 5"));
    assert!(icatalog(&[
        "collection",
        p,
        "create-smart",
        "--name",
        "Unrated",
        "--max-rating",
        "0"
    ])
    .status
    .success());
    assert!(s(&icatalog(&["collection", p, "rules", "--id", "1"]).stdout).contains("max-rating"));
    let invalid = icatalog(&[
        "collection",
        p,
        "set-rules",
        "--id",
        "1",
        "--name",
        "Bad",
        "--min-rating",
        "4",
        "--max-rating",
        "3",
    ]);
    assert!(!invalid.status.success());
    assert!(s(&invalid.stderr).contains("minimum rating must not exceed maximum rating"));
    let c = icatalog_engine::Context::open(&path).unwrap();
    assert_eq!(c.catalog().collection(1).unwrap().unwrap().name, "Unrated");
    assert_eq!(c.catalog().collection_assets(1).unwrap(), vec![2]);
    drop(c);
    std::fs::remove_file(path).unwrap();
}

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn unknown_long_option_rejects_edit_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unknown-option.sqlite");
    let p = path.to_str().unwrap();
    assert!(icatalog(&["init", p]).status.success());
    let catalogue = icatalog_engine::Context::open(&path).unwrap();
    catalogue
        .catalog()
        .connection()
        .execute_batch("INSERT INTO asset(id,uid,name,path) VALUES(1,'a','a','/a')")
        .unwrap();
    drop(catalogue);

    let result = icatalog(&["edit", p, "--id", "1", "--rating", "5", "--raiting", "1"]);
    assert_eq!(result.status.code(), Some(2));
    assert!(s(&result.stderr).contains("unknown option '--raiting'"));
    let catalogue = icatalog_engine::Context::open(&path).unwrap();
    assert!(catalogue.catalog().annotation(1).unwrap().is_none());
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
    assert!(out.contains("sync") && out.contains("serve") && out.contains("clone"));
}

#[test]
fn commands_lists_sync_and_serve() {
    let o = icatalog(&["commands"]);
    assert!(o.status.success());
    let out = s(&o.stdout);
    assert!(
        out.contains("sync") && out.contains("serve") && out.contains("clone"),
        "{out}"
    );
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
fn serve_requires_an_existing_file_without_creating_a_catalogue() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.sqlite");
    for path in [
        missing.to_str().unwrap(),
        dir.path().to_str().unwrap(),
        ":memory:",
        "",
    ] {
        let o = icatalog(&["serve", path, "--port", "0"]);
        assert!(!o.status.success(), "{}", s(&o.stdout));
        assert!(s(&o.stderr).contains("existing file"), "{}", s(&o.stderr));
        assert!(!missing.exists());
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

    // The explicit creation command still works; existing catalogues reach the
    // serve command's parameter validation rather than the opening guard.
    assert!(icatalog(&["init", missing.to_str().unwrap()])
        .status
        .success());
    let o = icatalog(&["serve", missing.to_str().unwrap(), "--port", "0"]);
    assert!(!o.status.success());
    assert!(s(&o.stderr).contains("port"), "{}", s(&o.stderr));
    assert!(!s(&o.stderr).contains("existing file"));
}

#[test]
fn commands_other_than_init_refuse_missing_catalogues_without_creating_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.sqlite");
    let p = path.to_str().unwrap();
    for (command, tail) in [
        ("open", &[][..]),
        ("list", &[][..]),
        ("find", &["photo"][..]),
        ("status", &[][..]),
        ("collection", &["list"][..]),
        ("previews", &["status"][..]),
        ("update", &[][..]),
        ("edit", &["--id", "1", "--rating", "5"][..]),
    ] {
        let mut args = vec![command, p];
        args.extend_from_slice(tail);
        let output = icatalog(&args);
        assert!(!output.status.success(), "{command}: {}", s(&output.stdout));
        assert!(s(&output.stderr).contains("existing file"));
        assert!(!path.exists(), "{command} created a catalogue");
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(icatalog(&["init", p]).status.success());
    assert!(icatalog(&["open", p]).status.success());
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
fn find_zero_rating_and_label_include_unannotated_assets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalogue.sqlite");
    let p = path.to_str().unwrap();
    assert!(icatalog(&["init", p]).status.success());
    let ctx = icatalog_engine::Context::open(&path).unwrap();
    ctx.catalog()
        .connection()
        .execute_batch(
            "INSERT INTO asset(id,uid,path,name) VALUES
             (1,'rated','/synthetic/RatedPhoto.jpg','RatedPhoto.jpg'),
             (2,'zero','/synthetic/ZeroPhoto.jpg','ZeroPhoto.jpg'),
             (3,'missing','/synthetic/MissingPhoto.jpg','MissingPhoto.jpg');
             INSERT INTO annotation(asset_id,rating,label) VALUES (1,5,2),(2,0,0);",
        )
        .unwrap();
    drop(ctx);

    let rating = icatalog(&["find", p, "Photo", "--rating", "0"]);
    assert!(rating.status.success(), "{}", s(&rating.stderr));
    for name in ["RatedPhoto", "ZeroPhoto", "MissingPhoto"] {
        assert!(s(&rating.stdout).contains(name));
    }
    let label = icatalog(&["find", p, "Photo", "--label", "0"]);
    assert!(label.status.success(), "{}", s(&label.stderr));
    assert!(!s(&label.stdout).contains("RatedPhoto"));
    for name in ["ZeroPhoto", "MissingPhoto"] {
        assert!(s(&label.stdout).contains(name));
    }
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

#[test]
fn previews_are_registered_and_validate_actions_through_the_cli() {
    let p = std::env::temp_dir().join(format!("icat-cli-previews-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let path = p.to_str().unwrap();
    assert!(icatalog(&["init", path]).status.success());
    let status = icatalog(&["previews", path, "status"]);
    assert!(status.status.success());
    assert!(s(&status.stdout).contains("embedded"));
    assert!(icatalog(&["previews", path, "external"]).status.success());
    assert!(s(&icatalog(&["previews", path, "status"]).stdout).contains("external"));
    assert!(icatalog(&["previews", path, "embed"]).status.success());
    assert!(!icatalog(&["previews", path]).status.success());
    assert!(!icatalog(&["previews", path, "invalid"]).status.success());
    let _ = std::fs::remove_file(p);
}

#[test]
fn clone_errors_do_not_create_or_open_the_destination() {
    let dir = std::env::temp_dir().join(format!("icat-cli-clone-errors-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("new.sqlite");
    let p = path.to_str().unwrap();
    let missing = icatalog(&["clone", p]);
    assert!(!missing.status.success());
    assert!(s(&missing.stderr).contains("url"));
    assert!(!path.exists());
    let bad = icatalog(&["clone", p, "--url", "https://example"]);
    assert!(!bad.status.success());
    assert!(!path.exists());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let url = format!("http://127.0.0.1:{port}");
    let offline = icatalog(&["clone", p, "--url", &url]);
    assert!(!offline.status.success());
    assert!(!path.exists());
    std::fs::write(&path, b"existing non-SQLite file").unwrap();
    let existing = icatalog(&["clone", p, "--url", &url]);
    assert!(!existing.status.success());
    assert!(s(&existing.stderr).contains("already exists"));
    assert_eq!(std::fs::read(path).unwrap(), b"existing non-SQLite file");
}

#[test]
fn clone_initialises_a_client_through_the_cli_without_init_or_a_manual_copy() {
    let dir = std::env::temp_dir().join(format!("icat-cli-clone-success-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let server_path = dir.join("server.sqlite");
    let mut server = icatalog_engine::Context::open(&server_path).unwrap();
    icatalog_engine::dispatch(&mut server, "init", &icatalog_engine::Params::new()).unwrap();
    server.catalog().connection().execute_batch("INSERT INTO asset(uid,name,path) VALUES('clone-asset','Photo.jpg','/synthetic/Photo.jpg')").unwrap();
    let listener = icatalog_sync::server::bind("127.0.0.1:0").unwrap();
    let port = listener.server_addr().to_ip().unwrap().port();
    std::thread::spawn(move || {
        let _ = icatalog_sync::server::serve_loop(listener, &mut server);
    });
    let path = dir.join("local.sqlite");
    let p = path.to_str().unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let out = icatalog(&["clone", p, "--url", &url]);
    assert!(out.status.success(), "{}", s(&out.stderr));
    assert!(s(&out.stdout).contains("local replica ready"));
    assert!(s(&icatalog(&["list", p]).stdout).contains("Photo.jpg"));
    let idle = icatalog(&["sync", p, "--url", &url]);
    assert!(idle.status.success());
    assert!(s(&idle.stdout).contains("up to date"));
    assert!(icatalog(&["edit", p, "--id", "1", "--rating", "5"])
        .status
        .success());
    let pushed = icatalog(&["sync", p, "--url", &url]);
    assert!(pushed.status.success());
    assert!(s(&pushed.stdout).contains("push: accepted"));
    let nas = icatalog_engine::Context::open(server_path).unwrap();
    assert_eq!(nas.catalog().annotation(1).unwrap().unwrap().rating, 5);
}
