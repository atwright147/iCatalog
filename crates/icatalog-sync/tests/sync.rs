//! Real HTTP v2 replication and recovery checks, with synthetic catalogues only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use icatalog_engine::{Context, Params};
use icatalog_sync::{
    client::{self, SyncAction},
    transport::{HttpResponse, Transport, UreqTransport},
};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::thread;

fn temp_dir(suffix: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("icat-v2-{suffix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn make_catalogue(dir: &Path, name: &str, id: &str, generation: i64) -> PathBuf {
    let path = dir.join(name);
    let mut c = Context::open(&path).unwrap();
    let mut p = Params::new();
    p.set("id", id);
    icatalog_engine::dispatch(&mut c, "init", &p).unwrap();
    c.catalog().set_generation(generation).unwrap();
    path
}
fn command(c: &mut Context, id: &str, args: &[(&str, &str)]) {
    let mut p = Params::new();
    for (k, v) in args {
        p.set(*k, *v);
    }
    icatalog_engine::dispatch(c, id, &p).unwrap();
}
fn start_server(path: PathBuf) -> u16 {
    let s = icatalog_sync::server::bind("127.0.0.1:0").unwrap();
    let port = s.server_addr().to_ip().unwrap().port();
    thread::spawn(move || {
        let mut c = Context::open(path).unwrap();
        let _ = icatalog_sync::server::serve_loop(s, &mut c);
    });
    port
}
fn seed(dir: &Path) -> (PathBuf, String) {
    let p = make_catalogue(dir, "server.sqlite", "test", 0);
    let c = Context::open(&p).unwrap();
    c.catalog().connection().execute_batch("INSERT INTO asset(id,uid,name,path,format) VALUES (1,'one','One.jpg','/synthetic/One.jpg','JPEG'),(2,'two','Two.jpg','/synthetic/Two.jpg','JPEG'); INSERT INTO collection(id,name) VALUES(1,'Manual'),(2,'Smart'); INSERT INTO smart_collection(collection_id,min_rating) VALUES(2,4);").unwrap();
    drop(c);
    let url = format!("http://127.0.0.1:{}", start_server(p.clone()));
    (p, url)
}
fn replica(server: &Path, name: &str, url: &str) -> Context {
    let p = server.with_file_name(name);
    std::fs::copy(server, &p).unwrap();
    let mut c = Context::open(p).unwrap();
    let r = client::sync_once(&mut c, &UreqTransport::new(), url).unwrap();
    assert!(matches!(r.action, SyncAction::Pulled { .. }));
    assert!(r.backup.is_some());
    c
}
fn sync(c: &mut Context, url: &str) -> icatalog_sync::client::SyncReport {
    client::sync_once(c, &UreqTransport::new(), url).unwrap()
}
fn generation(p: &Path) -> Option<i64> {
    Context::open(p).unwrap().catalog().generation().unwrap()
}
fn sync_params(url: &str) -> Params {
    let mut p = Params::new();
    p.set("url", url);
    p
}
fn temp_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|s| s == "tmp"))
        .collect()
}

#[test]
fn different_asset_edits_merge_and_idle_sync_has_no_upload_or_generation_change() {
    let dir = temp_dir("independent");
    let (server, url) = seed(&dir);
    let mut a = replica(&server, "a.sqlite", &url);
    let mut b = replica(&server, "b.sqlite", &url);
    let initial = generation(&server);
    let r = sync(&mut a, &url);
    assert_eq!(r.action, SyncAction::UpToDate);
    assert_eq!(r.bytes, 0);
    assert_eq!(r.sent_bytes, 0);
    assert_eq!(generation(&server), initial);
    command(
        &mut a,
        "edit",
        &[
            ("id", "1"),
            ("rating", "5"),
            ("label", "3"),
            ("keyword", "new keyword"),
        ],
    );
    command(&mut b, "edit", &[("id", "2"), ("rating", "4")]);
    let r = sync(&mut a, &url);
    assert_eq!(r.action, SyncAction::Pushed);
    assert!(r.sent_bytes < 1024);
    assert!(r.conflicts.is_empty());
    let r = sync(&mut b, &url);
    assert!(r.conflicts.is_empty());
    assert_eq!(b.catalog().annotation(1).unwrap().unwrap().rating, 5);
    assert_eq!(b.catalog().annotation(2).unwrap().unwrap().rating, 4);
    assert_eq!(
        b.catalog().asset_keywords(1).unwrap()[0].name,
        "new keyword"
    );
    assert_eq!(b.catalog().collection_assets(2).unwrap(), vec![1, 2]);
    assert_eq!(sync(&mut b, &url).action, SyncAction::UpToDate);
    let nas = Context::open(&server).unwrap();
    assert_eq!(
        nas.catalog().asset(1).unwrap().unwrap().path,
        "/synthetic/One.jpg"
    );
    assert!(!nas.catalog().is_replica().unwrap());
}

#[test]
fn collection_names_rules_members_and_concurrent_creations_sync_by_uid() {
    let dir = temp_dir("collections");
    let (server, url) = seed(&dir);
    let mut a = replica(&server, "a.sqlite", &url);
    let mut b = replica(&server, "b.sqlite", &url);
    command(
        &mut a,
        "collection",
        &[("action", "rename"), ("id", "1"), ("name", "Trip")],
    );
    command(
        &mut a,
        "collection",
        &[("action", "add"), ("id", "1"), ("asset", "1")],
    );
    command(
        &mut b,
        "collection",
        &[
            ("action", "set-rules"),
            ("id", "2"),
            ("name", "Top"),
            ("min-rating", "5"),
        ],
    );
    command(
        &mut a,
        "collection",
        &[("action", "create"), ("name", "From A")],
    );
    command(
        &mut b,
        "collection",
        &[("action", "create"), ("name", "From B")],
    );
    // Both new rows have local id 3, but unrelated stable identities.
    let auid = a
        .catalog()
        .collection_states()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "From A")
        .unwrap()
        .uid;
    let buid = b
        .catalog()
        .collection_states()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "From B")
        .unwrap()
        .uid;
    assert_ne!(auid, buid);
    assert!(sync(&mut a, &url).conflicts.is_empty());
    assert!(sync(&mut b, &url).conflicts.is_empty());
    sync(&mut a, &url);
    assert_eq!(a.catalog().collection_assets(1).unwrap(), vec![1]);
    assert_eq!(a.catalog().collection(1).unwrap().unwrap().name, "Trip");
    assert_eq!(
        a.catalog().smart_rules(2).unwrap().unwrap().min_rating,
        Some(5)
    );
    assert_eq!(a.catalog().collection(2).unwrap().unwrap().name, "Top");
    let rows = a.catalog().collection_states().unwrap();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().any(|c| c.uid == auid));
    assert!(rows.iter().any(|c| c.uid == buid));
}

#[test]
fn same_asset_and_collection_conflicts_preserve_local_work_and_apply_independent_changes() {
    let dir = temp_dir("conflicts");
    let (server, url) = seed(&dir);
    let mut a = replica(&server, "a.sqlite", &url);
    let mut b = replica(&server, "b.sqlite", &url);
    command(&mut a, "edit", &[("id", "1"), ("rating", "5")]);
    command(
        &mut a,
        "collection",
        &[("action", "rename"), ("id", "1"), ("name", "A wins")],
    );
    command(&mut b, "edit", &[("id", "1"), ("rating", "2")]);
    command(&mut b, "edit", &[("id", "2"), ("rating", "4")]);
    command(
        &mut b,
        "collection",
        &[("action", "rename"), ("id", "1"), ("name", "B draft")],
    );
    sync(&mut a, &url);
    let r = sync(&mut b, &url);
    assert!(matches!(r.action, SyncAction::Resynced { .. }));
    assert_eq!(r.conflicts.len(), 2);
    assert!(r.conflicts.contains(&"asset:one".to_owned()));
    let backup = Context::open(r.backup.unwrap()).unwrap();
    assert_eq!(backup.catalog().annotation(1).unwrap().unwrap().rating, 2);
    assert_eq!(
        backup.catalog().collection(1).unwrap().unwrap().name,
        "B draft"
    );
    assert_eq!(b.catalog().annotation(1).unwrap().unwrap().rating, 5);
    assert_eq!(b.catalog().annotation(2).unwrap().unwrap().rating, 4);
    assert_eq!(b.catalog().collection(1).unwrap().unwrap().name, "A wins");
}

struct LostReply {
    lost: Cell<bool>,
}
impl Transport for LostReply {
    fn get(&self, url: &str) -> icatalog_sync::Result<HttpResponse> {
        UreqTransport::new().get(url)
    }
    fn post(&self, url: &str, body: &[u8]) -> icatalog_sync::Result<HttpResponse> {
        let r = UreqTransport::new().post(url, body)?;
        if !self.lost.replace(true) {
            return Err(icatalog_sync::SyncError::Unreachable(
                "lost reply after commit".into(),
            ));
        }
        Ok(r)
    }
}
#[test]
fn retry_after_lost_commit_response_is_idempotent_and_preserves_pending_work() {
    let dir = temp_dir("retry");
    let (server, url) = seed(&dir);
    let mut c = replica(&server, "c.sqlite", &url);
    command(&mut c, "edit", &[("id", "1"), ("rating", "5")]);
    let t = LostReply {
        lost: Cell::new(false),
    };
    assert!(client::sync_once(&mut c, &t, &url).is_err());
    let committed = generation(&server);
    assert_eq!(c.catalog().annotation(1).unwrap().unwrap().rating, 5);
    let r = client::sync_once(&mut c, &t, &url).unwrap();
    assert!(r.conflicts.is_empty());
    assert_eq!(generation(&server), committed);
    assert_eq!(sync(&mut c, &url).action, SyncAction::UpToDate);
}

struct ScriptedTransport {
    queue: RefCell<VecDeque<HttpResponse>>,
}
impl Transport for ScriptedTransport {
    fn get(&self, _: &str) -> icatalog_sync::Result<HttpResponse> {
        self.queue
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| icatalog_sync::SyncError::Protocol("unexpected GET".into()))
    }
    fn post(&self, _: &str, _: &[u8]) -> icatalog_sync::Result<HttpResponse> {
        panic!("an unbased client must never upload")
    }
}
fn response(id: &str, generation: i64, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: [
            ("x-catalogue-id".into(), id.into()),
            ("x-generation".into(), generation.to_string()),
        ]
        .into(),
        body,
    }
}
fn info_response(id: &str, generation: i64) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: Default::default(),
        body: format!("protocol=2\nid={id}\ngeneration={generation}\n").into_bytes(),
    }
}

#[test]
fn invalid_snapshot_or_header_leaves_local_untouched_and_removes_temporary_files() {
    let dir = temp_dir("bad-download");
    let local = make_catalogue(&dir, "local.sqlite", "test", 0);
    let good = make_catalogue(&dir, "download.sqlite", "test", 1);
    for body in [b"not sqlite".to_vec(), std::fs::read(good).unwrap()] {
        let t = ScriptedTransport {
            queue: RefCell::new(vec![info_response("test", 2), response("test", 2, body)].into()),
        };
        let mut c = Context::open(&local).unwrap();
        assert!(client::sync_once(&mut c, &t, "http://example").is_err());
        assert_eq!(c.catalog().generation().unwrap(), Some(0));
        assert!(!c.catalog().is_replica().unwrap());
        assert!(temp_files(&dir).is_empty());
    }
}
#[test]
fn first_sync_preserves_unknown_local_edits_and_never_uploads_media() {
    let dir = temp_dir("bootstrap");
    let (server, url) = seed(&dir);
    let local = server.with_file_name("local.sqlite");
    std::fs::copy(&server, &local).unwrap();
    let mut c = Context::open(&local).unwrap();
    c.catalog()
        .connection()
        .execute_batch(
            "INSERT INTO asset(uid,name,path) VALUES('private','Private','/synthetic/private');",
        )
        .unwrap();
    command(&mut c, "edit", &[("id", "1"), ("rating", "3")]);
    let r = sync(&mut c, &url);
    assert_eq!(r.sent_bytes, 0);
    let backup = Context::open(r.backup.unwrap()).unwrap();
    assert!(backup.catalog().asset_by_uid("private").unwrap().is_some());
    assert_eq!(backup.catalog().annotation(1).unwrap().unwrap().rating, 3);
    assert!(c.catalog().asset_by_uid("private").unwrap().is_none());
    let nas = Context::open(server).unwrap();
    assert!(nas.catalog().asset_by_uid("private").unwrap().is_none());
    assert!(c
        .catalog()
        .connection()
        .execute(
            "INSERT INTO asset(uid,name,path) VALUES('blocked','B','/b')",
            []
        )
        .is_err());
}

#[test]
fn snapshot_uploads_are_retired_and_wrong_ids_and_invalid_batches_do_not_write() {
    let dir = temp_dir("retired");
    let (server, url) = seed(&dir);
    let before = generation(&server);
    let t = UreqTransport::new();
    assert_eq!(
        t.post(&format!("{url}/catalogue?expected=0"), b"hostile")
            .unwrap()
            .status,
        405
    );
    assert_eq!(
        t.post(&format!("{url}/annotations"), b"hostile")
            .unwrap()
            .status,
        400
    );
    let body = icatalog_sync::delta::encode("wrong", &[]).unwrap();
    assert_eq!(
        t.post(&format!("{url}/annotations"), &body).unwrap().status,
        400
    );
    assert_eq!(generation(&server), before);
}

#[test]
fn offline_is_deferred_not_fatal() {
    // A port with nothing listening.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let dir = temp_dir("offline");
    let local = make_catalogue(&dir, "local.sqlite", "cat-offline", 0);
    let mut ctx = Context::open(&local).unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let out = icatalog_sync::dispatch_guarded(&mut ctx, "sync", &sync_params(&url)).unwrap();
    assert!(out.render().contains("deferred"), "{}", out.render());
    assert_eq!(ctx.catalog().generation().unwrap(), Some(0));
}

#[test]
fn hostile_params_never_panic() {
    let dir = temp_dir("hostile");
    let local = make_catalogue(&dir, "local.sqlite", "cat-hostile", 0);
    let mut ctx = Context::open(&local).unwrap();

    let mut urls: Vec<String> = [
        "",
        " ",
        "http://",
        "ftp://nas",
        "https://nas",
        "http://127.0.0.1:1",
        "http://127.0.0.1:0",
        "file:///etc/passwd",
        "http://exa mple.com",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    urls.push("http://127.0.0.1\0:80".to_owned());
    urls.push("http://127.0.0.1\n:80".to_owned());
    urls.push("h".repeat(4096));

    for url in &urls {
        let p = sync_params(url);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            icatalog_sync::dispatch_guarded(&mut ctx, "sync", &p)
        }));
        assert!(r.is_ok(), "panicked on url {url:?}");
        let destination = dir.join("hostile-clone.sqlite");
        let mut clone = Context::for_destination(&destination).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            icatalog_sync::dispatch_guarded(&mut clone, "clone", &p)
        }));
        assert!(result.is_ok(), "clone panicked on URL {url:?}");
        assert!(result.unwrap().is_err());
        assert!(!destination.exists());
    }

    // Only invalid ports reach binding; a valid port would block, so avoid it.
    for port in ["", " ", "abc", "0", "-1", "65536", "999999999999", "1e9"] {
        let mut p = Params::new();
        p.set("port", port);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            icatalog_sync::dispatch_guarded(&mut ctx, "serve", &p)
        }));
        assert!(r.is_ok(), "panicked on port {port:?}");
        assert!(r.unwrap().is_err(), "port {port:?} should be rejected");
    }
    assert!(icatalog_sync::dispatch_guarded(&mut ctx, "serve", &Params::new()).is_err());
}

#[test]
#[ignore = "10,000-asset snapshot and annotation round trip; run task scale"]
fn large_snapshot_round_trip() {
    let dir = temp_dir("scale");
    let server = make_catalogue(&dir, "server.sqlite", "test", 0);
    let c = Context::open(&server).unwrap();
    c.catalog().connection().execute_batch("BEGIN; WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10000) INSERT INTO asset(uid,path,name) SELECT 'scale-'||x,'/synthetic/'||x,'image-'||x FROM n; INSERT INTO thumbnail(asset_id,size_class,format,bytes,source,orientation_applied,width,height,generator_version) SELECT id,256,'jpeg',zeroblob(1024),'synthetic',0,8,8,1 FROM asset; COMMIT;").unwrap();
    drop(c);
    let url = format!("http://127.0.0.1:{}", start_server(server.clone()));
    let mut a = replica(&server, "a.sqlite", &url);
    let mut b = replica(&server, "b.sqlite", &url);
    command(&mut a, "edit", &[("id", "1"), ("rating", "5")]);
    let r = sync(&mut a, &url);
    assert!(r.sent_bytes < 1024);
    sync(&mut b, &url);
    assert_eq!(b.catalog().stats().unwrap().assets, 10000);
    assert_eq!(b.catalog().annotation(1).unwrap().unwrap().rating, 5);
    assert_eq!(sync(&mut a, &url).action, SyncAction::UpToDate);
}

fn bmp() -> Vec<u8> {
    let mut b = vec![0; 246];
    b[0..2].copy_from_slice(b"BM");
    b[2..6].copy_from_slice(&246u32.to_le_bytes());
    b[10..14].copy_from_slice(&54u32.to_le_bytes());
    b[14..18].copy_from_slice(&40u32.to_le_bytes());
    b[18..22].copy_from_slice(&8u32.to_le_bytes());
    b[22..26].copy_from_slice(&8u32.to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b[28..30].copy_from_slice(&24u16.to_le_bytes());
    b[54..].fill(90);
    b
}
struct GetOnly {
    gets: Cell<usize>,
}
impl Transport for GetOnly {
    fn get(&self, url: &str) -> icatalog_sync::Result<HttpResponse> {
        self.gets.set(self.gets.get() + 1);
        UreqTransport::new().get(url)
    }
    fn post(&self, _: &str, _: &[u8]) -> icatalog_sync::Result<HttpResponse> {
        panic!("preview retrieval must never upload")
    }
}
#[test]
fn previews_are_external_pull_only_verified_and_cached_without_original_media() {
    let dir = temp_dir("previews");
    let server = make_catalogue(&dir, "server.sqlite", "test", 0);
    let media = dir.join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("generated.bmp"), bmp()).unwrap();
    let mut nas = Context::open(&server).unwrap();
    command(&mut nas, "import", &[("path", media.to_str().unwrap())]);
    command(&mut nas, "previews", &[("action", "external")]);
    command(&mut nas, "previews", &[("action", "generate")]);
    let hash = nas.catalog().preview(1).unwrap().unwrap().hash;
    drop(nas);
    let url = format!("http://127.0.0.1:{}", start_server(server.clone()));
    let client_dir = dir.join("client");
    std::fs::create_dir(&client_dir).unwrap();
    let client_path = client_dir.join("replica.sqlite");
    std::fs::copy(&server, &client_path).unwrap();
    let mut c = Context::open(client_path).unwrap();
    sync(&mut c, &url);
    assert!(
        icatalog_engine::previews::read(c.catalog(), c.path(), &hash)
            .unwrap()
            .is_none()
    );
    let corrupt = ScriptedTransport {
        queue: RefCell::new(vec![response("test", 0, b"corrupt preview".to_vec())].into()),
    };
    assert!(client::fetch_preview(&c, &corrupt, &url, &hash).is_err());
    assert!(
        icatalog_engine::previews::read(c.catalog(), c.path(), &hash)
            .unwrap()
            .is_none()
    );
    let t = GetOnly { gets: Cell::new(0) };
    assert!(client::fetch_preview(&c, &t, &url, &hash).unwrap());
    assert!(!client::fetch_preview(&c, &t, &url, &hash).unwrap());
    assert_eq!(t.gets.get(), 1);
    assert!(client_dir
        .join("previews")
        .join(format!("{hash}.jpg"))
        .exists());
    let bytes = icatalog_engine::previews::read(c.catalog(), c.path(), &hash)
        .unwrap()
        .unwrap();
    assert_eq!(icatalog_engine::previews::hash(&bytes), hash);
    let r = UreqTransport::new()
        .get(&format!("{url}/preview/{}", "a".repeat(64)))
        .unwrap();
    assert_eq!(r.status, 404);
    assert!(client::fetch_preview(&c, &t, &url, "../private").is_err());
}

#[test]
fn server_batch_failure_rolls_back_earlier_user_edits() {
    let dir = temp_dir("rollback");
    let (server, _) = seed(&dir);
    let nas = Context::open(server).unwrap();
    let before = nas.catalog().generation().unwrap();
    let mut changes = icatalog_sync::delta::current(nas.catalog()).unwrap();
    changes.retain(|r| matches!(r.value, icatalog_sync::delta::Value::Annotation(_)));
    for r in &mut changes {
        if let icatalog_sync::delta::Value::Annotation(a) = &mut r.value {
            a.rating = 5;
        }
    }
    nas.catalog().connection().execute_batch("CREATE TRIGGER fail_second BEFORE INSERT ON annotation WHEN NEW.asset_id=2 BEGIN SELECT RAISE(FAIL,'synthetic failure'); END;").unwrap();
    assert!(icatalog_sync::server::apply_edits(&nas, "test", &changes).is_err());
    assert!(nas.catalog().annotation(1).unwrap().is_none());
    assert!(nas.catalog().annotation(2).unwrap().is_none());
    assert_eq!(nas.catalog().generation().unwrap(), before);
}

#[test]
fn local_writer_cannot_change_state_during_an_upload_or_download() {
    let dir = temp_dir("local-lock");
    let (server, url) = seed(&dir);
    let mut c = replica(&server, "client.sqlite", &url);
    command(&mut c, "edit", &[("id", "1"), ("rating", "5")]);
    struct Locked<'a> {
        writer: &'a Context,
    }
    impl Transport for Locked<'_> {
        fn get(&self, url: &str) -> icatalog_sync::Result<HttpResponse> {
            assert!(self
                .writer
                .catalog()
                .connection()
                .execute("UPDATE annotation SET rating=2 WHERE asset_id=1", [])
                .is_err());
            UreqTransport::new().get(url)
        }
        fn post(&self, url: &str, b: &[u8]) -> icatalog_sync::Result<HttpResponse> {
            assert!(self
                .writer
                .catalog()
                .connection()
                .execute("UPDATE annotation SET rating=2 WHERE asset_id=1", [])
                .is_err());
            UreqTransport::new().post(url, b)
        }
    }
    let writer = Context::open(c.path()).unwrap();
    writer
        .catalog()
        .connection()
        .busy_timeout(std::time::Duration::ZERO)
        .unwrap();
    client::sync_once(&mut c, &Locked { writer: &writer }, &url).unwrap();
    assert_eq!(c.catalog().annotation(1).unwrap().unwrap().rating, 5);
    assert!(
        writer
            .catalog()
            .connection()
            .execute("UPDATE annotation SET rating=2 WHERE asset_id=1", [])
            .is_err(),
        "an old connection must not successfully write to the replaced file"
    );
}

#[test]
fn legacy_servers_and_broken_foreign_key_downloads_are_refused_before_replacement() {
    let dir = temp_dir("old-and-fk");
    let local = make_catalogue(&dir, "local.sqlite", "test", 0);
    let old = ScriptedTransport {
        queue: RefCell::new(
            vec![HttpResponse {
                status: 200,
                headers: Default::default(),
                body: b"id=test\ngeneration=0\n".to_vec(),
            }]
            .into(),
        ),
    };
    let mut c = Context::open(&local).unwrap();
    assert!(client::sync_once(&mut c, &old, "http://old").is_err());
    assert!(!c.catalog().is_replica().unwrap());
    let broken = make_catalogue(&dir, "broken.sqlite", "test", 1);
    let db = Context::open(&broken).unwrap();
    db.catalog().connection().execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO asset(uid,name,path,folder_id) VALUES('broken','Broken','/broken',999);").unwrap();
    let generation = db.catalog().generation().unwrap().unwrap();
    drop(db);
    let t = ScriptedTransport {
        queue: RefCell::new(
            vec![
                info_response("test", generation),
                response("test", generation, std::fs::read(broken).unwrap()),
            ]
            .into(),
        ),
    };
    assert!(client::sync_once(&mut c, &t, "http://broken").is_err());
    assert_eq!(c.catalog().stats().unwrap().assets, 0);
    assert!(temp_files(&dir).is_empty());
}

#[test]
fn clone_downloads_a_ready_replica_and_the_first_edit_is_uploaded() {
    let dir = temp_dir("clone-ready");
    let (server, url) = seed(&dir);
    let destination = dir.join("cloned.sqlite");
    let before = generation(&server);
    let mut c = Context::for_destination(&destination).unwrap();
    assert!(!destination.exists());
    let report = icatalog_sync::clone_catalogue(&mut c, &UreqTransport::new(), &url).unwrap();
    assert_eq!(report.catalogue_id, "test");
    assert_eq!(Some(report.generation), before);
    assert!(report.bytes > 0);
    assert!(c.catalog().is_replica().unwrap());
    assert_eq!(c.catalog().stats().unwrap().assets, 2);
    assert_eq!(
        c.catalog().collection_states().unwrap(),
        Context::open(&server)
            .unwrap()
            .catalog()
            .collection_states()
            .unwrap()
    );
    assert_eq!(sync(&mut c, &url).action, SyncAction::UpToDate);
    assert!(!std::fs::read_dir(&dir).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains(".rejected-")));
    command(&mut c, "edit", &[("id", "1"), ("rating", "5")]);
    let r = sync(&mut c, &url);
    assert_eq!(r.action, SyncAction::Pushed);
    assert!(!r.initial);
    assert_eq!(
        Context::open(server)
            .unwrap()
            .catalog()
            .annotation(1)
            .unwrap()
            .unwrap()
            .rating,
        5
    );
    assert!(temp_files(&dir).is_empty());
}

struct NoCloneRequests;
impl Transport for NoCloneRequests {
    fn get(&self, _: &str) -> icatalog_sync::Result<HttpResponse> {
        panic!("clone must reject an existing destination before contacting the server")
    }
    fn post(&self, _: &str, _: &[u8]) -> icatalog_sync::Result<HttpResponse> {
        panic!("clone never uploads")
    }
}
#[test]
fn clone_refuses_existing_files_directories_and_orphan_sqlite_sidecars() {
    let dir = temp_dir("clone-existing");
    let file = dir.join("existing.sqlite");
    std::fs::write(&file, b"untouched user data").unwrap();
    let missing_parent = dir.join("missing").join("clone.sqlite");
    for path in [&file, &dir, &missing_parent] {
        let mut c = Context::for_destination(path).unwrap();
        assert!(
            icatalog_sync::clone_catalogue(&mut c, &NoCloneRequests, "http://example").is_err()
        );
    }
    assert_eq!(std::fs::read(&file).unwrap(), b"untouched user data");
    for suffix in ["-journal", "-wal", "-shm"] {
        let path = dir.join(format!("new{suffix}.sqlite"));
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        std::fs::write(&sidecar, b"existing sidecar").unwrap();
        let mut c = Context::for_destination(&path).unwrap();
        assert!(
            icatalog_sync::clone_catalogue(&mut c, &NoCloneRequests, "http://example").is_err()
        );
        assert!(!path.exists());
        assert_eq!(std::fs::read(&sidecar).unwrap(), b"existing sidecar");
    }
    assert!(temp_files(&dir).is_empty());
}

#[cfg(unix)]
#[test]
fn clone_refuses_dangling_symlinks_and_handles_non_utf8_destination_paths() {
    let dir = temp_dir("clone-paths");
    let link = dir.join("dangling.sqlite");
    let absent = dir.join("absent.sqlite");
    std::os::unix::fs::symlink(&absent, &link).unwrap();
    let mut c = Context::for_destination(&link).unwrap();
    assert!(icatalog_sync::clone_catalogue(&mut c, &NoCloneRequests, "http://example").is_err());
    assert!(link.is_symlink());
    assert!(!absent.exists());
    use std::os::unix::ffi::OsStringExt;
    let destination = dir.join(std::ffi::OsString::from_vec(b"local-\xff.sqlite".to_vec()));
    // Linux filesystems commonly support arbitrary name bytes; macOS APFS
    // rejects these names with EILSEQ. Neither outcome may leave a partial clone.
    let supported = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
    {
        Ok(file) => {
            drop(file);
            std::fs::remove_file(&destination).unwrap();
            true
        }
        Err(_) => false,
    };
    let (_, url) = seed(&dir);
    let mut c = Context::for_destination(&destination).unwrap();
    let result = icatalog_sync::clone_catalogue(&mut c, &UreqTransport::new(), &url);
    if supported {
        result.unwrap();
        assert!(destination.exists());
        assert!(c.catalog().is_replica().unwrap());
    } else {
        assert!(result.is_err());
        assert!(!destination.exists());
        assert!(!c.catalog().is_replica().unwrap());
    }
    assert!(temp_files(&dir).is_empty());
}

#[test]
fn failed_clone_downloads_leave_no_destination_and_can_be_retried() {
    let dir = temp_dir("clone-invalid");
    let path = make_catalogue(&dir, "snapshot.sqlite", "test", 1);
    let good = std::fs::read(&path).unwrap();
    let replica = Context::open(&path).unwrap();
    replica
        .catalog()
        .set_replica_base(1, b"already a replica")
        .unwrap();
    drop(replica);
    let already_replica = std::fs::read(path).unwrap();
    let wal_path = make_catalogue(&dir, "wal.sqlite", "test", 1);
    let wal = Context::open(&wal_path).unwrap();
    wal.catalog()
        .connection()
        .execute_batch("PRAGMA journal_mode=WAL")
        .unwrap();
    drop(wal);
    let wal_bytes = std::fs::read(wal_path).unwrap();
    let cases = vec![
        vec![
            info_response("test", 1),
            response("test", 1, b"not sqlite".to_vec()),
        ],
        vec![info_response("test", 2), response("test", 1, good.clone())],
        vec![info_response("test", 1), response("wrong", 1, good.clone())],
        vec![info_response("test", 2), response("test", 2, good)],
        vec![
            info_response("test", 1),
            response("test", 1, already_replica),
        ],
        vec![info_response("test", 1), response("test", 1, wal_bytes)],
    ];
    for (index, responses) in cases.into_iter().enumerate() {
        let dest = dir.join(format!("failed-{index}.sqlite"));
        let mut c = Context::for_destination(&dest).unwrap();
        let t = ScriptedTransport {
            queue: RefCell::new(responses.into()),
        };
        assert!(icatalog_sync::clone_catalogue(&mut c, &t, "http://example").is_err());
        assert!(!dest.exists());
        assert!(!c.catalog().is_replica().unwrap());
        assert!(temp_files(&dir).is_empty());
    }
    let (server, url) = seed(&dir);
    let dest = dir.join("failed-0.sqlite");
    let mut c = Context::for_destination(&dest).unwrap();
    icatalog_sync::clone_catalogue(&mut c, &UreqTransport::new(), &url).unwrap();
    assert_eq!(
        c.catalog().catalogue().unwrap().unwrap().id,
        Context::open(server)
            .unwrap()
            .catalog()
            .catalogue()
            .unwrap()
            .unwrap()
            .id
    );
}

#[test]
fn clone_does_not_overwrite_a_destination_created_during_download() {
    let dir = temp_dir("clone-race");
    let path = make_catalogue(&dir, "snapshot.sqlite", "test", 1);
    let body = std::fs::read(path).unwrap();
    let destination = dir.join("raced.sqlite");
    struct Race {
        path: PathBuf,
        queue: RefCell<VecDeque<HttpResponse>>,
    }
    impl Transport for Race {
        fn get(&self, url: &str) -> icatalog_sync::Result<HttpResponse> {
            if url.ends_with("/catalogue") {
                std::fs::write(&self.path, b"concurrently created file").unwrap();
            }
            self.queue
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| icatalog_sync::SyncError::Protocol("unexpected request".into()))
        }
        fn post(&self, _: &str, _: &[u8]) -> icatalog_sync::Result<HttpResponse> {
            panic!("clone never uploads")
        }
    }
    let t = Race {
        path: destination.clone(),
        queue: RefCell::new(vec![info_response("test", 1), response("test", 1, body)].into()),
    };
    let mut c = Context::for_destination(&destination).unwrap();
    assert!(icatalog_sync::clone_catalogue(&mut c, &t, "http://example").is_err());
    assert_eq!(
        std::fs::read(destination).unwrap(),
        b"concurrently created file"
    );
    assert!(temp_files(&dir).is_empty());
}
