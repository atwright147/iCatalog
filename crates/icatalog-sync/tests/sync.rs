//! Sync tests: a real round trip against the service, the generation race,
//! snapshot integrity, deferral when offline, and hostile parameters.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::thread;

use icatalog_engine::{Context, Params};
use icatalog_sync::client::{self, SyncAction};
use icatalog_sync::protocol::{HEADER_GENERATION, HEADER_ID};
use icatalog_sync::transport::{HttpResponse, Transport};

fn temp_dir(suffix: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("icat-sync-{suffix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Create a catalogue with a fixed id and generation, and return its path.
fn make_catalogue(dir: &Path, name: &str, id: &str, generation: i64) -> PathBuf {
    let path = dir.join(name);
    let mut ctx = Context::open(&path).unwrap();
    let mut p = Params::new();
    p.set("id", id);
    icatalog_engine::dispatch_guarded(&mut ctx, "init", &p).unwrap();
    if generation != 0 {
        ctx.catalog().set_generation(generation).unwrap();
    }
    path
}

/// The bytes of a catalogue with `id` at `generation` (a stand-in snapshot).
fn snapshot_bytes(dir: &Path, id: &str, generation: i64) -> Vec<u8> {
    let name = format!("snapshot-{id}-{generation}.sqlite");
    let path = make_catalogue(dir, &name, id, generation);
    std::fs::read(&path).unwrap()
}

fn start_server(catalogue: PathBuf) -> u16 {
    let server = icatalog_sync::server::bind("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    thread::spawn(move || {
        let mut ctx = Context::open(&catalogue).unwrap();
        let _ = icatalog_sync::server::serve_loop(server, &mut ctx);
    });
    port
}

fn sync_params(url: &str) -> Params {
    let mut p = Params::new();
    p.set("url", url);
    p
}

fn generation(path: &Path) -> Option<i64> {
    Context::open(path).unwrap().catalog().generation().unwrap()
}

fn backup_files(dir: &Path) -> Vec<PathBuf> {
    read_matching(dir, |n| n.contains(".rejected-"))
}

fn temp_files(dir: &Path) -> Vec<PathBuf> {
    read_matching(dir, |n| n.ends_with(".tmp"))
}

fn read_matching(dir: &Path, pred: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(&pred)
                .unwrap_or(false)
        })
        .collect()
}

#[test]
fn push_then_pull_round_trip() {
    let dir = temp_dir("roundtrip");
    let server_path = make_catalogue(&dir, "nas.sqlite", "cat-roundtrip", 0);
    let client_path = dir.join("local.sqlite");
    std::fs::copy(&server_path, &client_path).unwrap();
    let port = start_server(server_path.clone());
    let url = format!("http://127.0.0.1:{port}");

    let mut client = Context::open(&client_path).unwrap();

    // Level generations: local state is published to the NAS.
    let out = icatalog_sync::dispatch_guarded(&mut client, "sync", &sync_params(&url)).unwrap();
    assert!(out.render().contains("push"), "{}", out.render());
    assert_eq!(generation(&server_path), Some(1));
    assert_eq!(client.catalog().generation().unwrap(), Some(1));

    // Another client pushes to the NAS: the server generation moves ahead.
    Context::open(&server_path)
        .unwrap()
        .catalog()
        .bump_generation()
        .unwrap();

    // The client is behind: it pulls, backing up first.
    let out = icatalog_sync::dispatch_guarded(&mut client, "sync", &sync_params(&url)).unwrap();
    let text = out.render();
    assert!(text.contains("pull"), "{text}");
    assert!(text.contains("backup"), "{text}");
    assert_eq!(client.catalog().generation().unwrap(), Some(2));
    assert!(!backup_files(&dir).is_empty());
}

#[test]
fn refused_push_backs_up_and_resyncs() {
    let dir = temp_dir("refused");
    let local = make_catalogue(&dir, "local.sqlite", "cat-refused", 0);
    let snapshot = snapshot_bytes(&dir, "cat-refused", 1);

    let scripted = ScriptedTransport::new(vec![
        info_response("cat-refused", 0),
        response(409, "cat-refused", 1, Vec::new()),
        response(200, "cat-refused", 1, snapshot),
    ]);

    let mut ctx = Context::open(&local).unwrap();
    let report = client::sync_once(&mut ctx, &scripted, "http://example").unwrap();
    match report.action {
        SyncAction::Resynced { backup } => assert!(backup.is_some()),
        other => panic!("expected a resync, got {other:?}"),
    }
    // The resynced copy is at the server generation, and the edits survive.
    assert_eq!(ctx.catalog().generation().unwrap(), Some(1));
    assert!(!backup_files(&dir).is_empty());
}

#[test]
fn invalid_snapshot_leaves_local_untouched() {
    let dir = temp_dir("integrity");
    let local = make_catalogue(&dir, "local.sqlite", "cat-int", 0);

    let scripted = ScriptedTransport::new(vec![
        info_response("cat-int", 1),
        response(200, "cat-int", 1, b"not a sqlite database".to_vec()),
    ]);

    let mut ctx = Context::open(&local).unwrap();
    let err = client::sync_once(&mut ctx, &scripted, "http://example").unwrap_err();
    assert!(!err.to_string().is_empty());
    // Nothing was replaced, and the temp file was cleaned up.
    assert_eq!(ctx.catalog().generation().unwrap(), Some(0));
    assert!(temp_files(&dir).is_empty(), "{:?}", temp_files(&dir));
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

fn info_response(id: &str, generation: i64) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: Default::default(),
        body: format!("id={id}\ngeneration={generation}\n").into_bytes(),
    }
}

fn response(status: u16, id: &str, generation: i64, body: Vec<u8>) -> HttpResponse {
    let mut headers = std::collections::BTreeMap::new();
    headers.insert(HEADER_ID.to_ascii_lowercase(), id.to_owned());
    headers.insert(
        HEADER_GENERATION.to_ascii_lowercase(),
        generation.to_string(),
    );
    HttpResponse {
        status,
        headers,
        body,
    }
}

/// A transport that replays scripted responses in order.
struct ScriptedTransport {
    queue: RefCell<VecDeque<HttpResponse>>,
}

impl ScriptedTransport {
    fn new(responses: Vec<HttpResponse>) -> Self {
        ScriptedTransport {
            queue: RefCell::new(responses.into()),
        }
    }
    fn next(&self) -> icatalog_sync::Result<HttpResponse> {
        self.queue
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| icatalog_sync::SyncError::Protocol("no scripted response".into()))
    }
}

impl Transport for ScriptedTransport {
    fn get(&self, _url: &str) -> icatalog_sync::Result<HttpResponse> {
        self.next()
    }
    fn post(&self, _url: &str, _body: &[u8]) -> icatalog_sync::Result<HttpResponse> {
        self.next()
    }
}
