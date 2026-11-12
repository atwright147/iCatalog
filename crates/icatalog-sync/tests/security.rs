//! Synthetic HTTPS and authentication tests; no external service or personal media.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use icatalog_engine::{Context, Params};
use icatalog_sync::{
    client,
    security::ServerSecurity,
    server,
    transport::{Transport, UreqTransport},
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const TOKEN: &str = "synthetic-test-token-0123456789-abcdef";
fn params(values: &[(&str, &str)]) -> Params {
    let mut p = Params::new();
    for (k, v) in values {
        p.set(*k, *v);
    }
    p
}
fn command(ctx: &mut Context, name: &str, values: &[(&str, &str)]) {
    icatalog_engine::dispatch(ctx, name, &params(values)).unwrap();
}
fn catalogue(dir: &Path, name: &str) -> Context {
    let mut ctx = Context::open(dir.join(name)).unwrap();
    command(&mut ctx, "init", &[("id", "secure-test")]);
    ctx
}
fn credentials(dir: &Path, host: &str) -> Params {
    credentials_with_expiry(dir, host, false)
}
fn credentials_with_expiry(dir: &Path, host: &str, expired: bool) -> Params {
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca = rcgen::CertificateParams::new(Vec::new()).unwrap();
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::DigitalSignature,
    ];
    let ca = ca.self_signed(&ca_key).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let mut cert = rcgen::CertificateParams::new(vec![host.into()]).unwrap();
    cert.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    if expired {
        cert.not_before = rcgen::date_time_ymd(2000, 1, 1);
        cert.not_after = rcgen::date_time_ymd(2001, 1, 1);
    }
    let cert = cert.signed_by(&key, &ca, &ca_key).unwrap();
    let files = [
        ("ca-file", ca.pem()),
        ("tls-cert", cert.pem()),
        ("tls-key", key.serialize_pem()),
        ("token-file", format!("{TOKEN}\n")),
    ];
    let mut p = Params::new();
    for (name, bytes) in files {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        p.set(name, path.to_str().unwrap());
    }
    p
}
fn start(ctx: Context, p: &Params) -> String {
    let mut security = ServerSecurity::from_params(p, "127.0.0.1").unwrap();
    let scheme = if security.uses_tls() { "https" } else { "http" };
    let s = server::bind_with_security("127.0.0.1:0", &mut security).unwrap();
    let url = format!(
        "{scheme}://127.0.0.1:{}",
        s.server_addr().to_ip().unwrap().port()
    );
    std::thread::spawn(move || {
        let mut ctx = ctx;
        let _ = server::serve_loop_with_security(s, &mut ctx, security);
    });
    url
}
fn client_params(server: &Params, url: &str, token: Option<&str>) -> Params {
    let mut p = params(&[("url", url), ("ca-file", server.get("ca-file").unwrap())]);
    if let Some(token) = token {
        p.set("token-file", token);
    }
    p
}
fn bmp() -> Vec<u8> {
    let mut v = vec![0; 58];
    v[0..2].copy_from_slice(b"BM");
    v[2..6].copy_from_slice(&58u32.to_le_bytes());
    v[10..14].copy_from_slice(&54u32.to_le_bytes());
    v[14..18].copy_from_slice(&40u32.to_le_bytes());
    v[18..22].copy_from_slice(&1i32.to_le_bytes());
    v[22..26].copy_from_slice(&1i32.to_le_bytes());
    v[26..28].copy_from_slice(&1u16.to_le_bytes());
    v[28..30].copy_from_slice(&24u16.to_le_bytes());
    v[54..58].copy_from_slice(&[10, 20, 30, 0]);
    v
}

#[test]
fn authenticated_https_clone_edits_pull_and_preview_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let p = credentials(dir.path(), "127.0.0.1");
    let mut nas = catalogue(dir.path(), "server.icatalog");
    let media = dir.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("generated.bmp"), bmp()).unwrap();
    command(&mut nas, "import", &[("path", media.to_str().unwrap())]);
    command(&mut nas, "previews", &[("action", "external")]);
    command(&mut nas, "previews", &[("action", "generate")]);
    let hash = nas.catalog().preview(1).unwrap().unwrap().hash;
    let server_path = nas.path().to_owned();
    let url = start(nas, &p);
    let cp = client_params(&p, &url, p.get("token-file"));
    let client_dir = dir.path().join("client");
    std::fs::create_dir(&client_dir).unwrap();
    let mut local = Context::for_destination(client_dir.join("local.icatalog")).unwrap();
    icatalog_sync::dispatch_guarded(&mut local, "clone", &cp).unwrap();
    command(
        &mut local,
        "edit",
        &[("id", "1"), ("rating", "5"), ("keyword", "Secure")],
    );
    command(
        &mut local,
        "collection",
        &[("action", "create"), ("name", "Secure collection")],
    );
    icatalog_sync::dispatch_guarded(&mut local, "sync", &cp).unwrap();
    let server_copy = Context::open_existing(&server_path).unwrap();
    assert_eq!(
        server_copy.catalog().annotation(1).unwrap().unwrap().rating,
        5
    );
    assert_eq!(local.catalog().annotation(1).unwrap().unwrap().rating, 5);
    let mut pp = cp;
    pp.set("preview", &hash);
    assert!(icatalog_sync::dispatch_guarded(&mut local, "sync", &pp)
        .unwrap()
        .render()
        .contains("downloaded"));
    let bytes = icatalog_engine::previews::read(local.catalog(), local.path(), &hash)
        .unwrap()
        .unwrap();
    assert_eq!(icatalog_engine::previews::hash(&bytes), hash);
    command(&mut local, "edit", &[("id", "1"), ("rating", "2")]);
    let baseline = local.catalog().replica_base().unwrap();
    let draft = std::fs::read(local.path()).unwrap();
    let wrong_path = dir.path().join("wrong-token");
    std::fs::write(&wrong_path, "wrong-test-token-0123456789-abcdef").unwrap();
    let wrong = client_params(&p, &url, Some(wrong_path.to_str().unwrap()));
    assert!(icatalog_sync::dispatch_guarded(&mut local, "sync", &wrong).is_err());
    assert_eq!(local.catalog().replica_base().unwrap(), baseline);
    assert_eq!(std::fs::read(local.path()).unwrap(), draft);
}

#[test]
fn missing_or_wrong_credentials_protect_every_route_and_keep_local_drafts() {
    let dir = tempfile::tempdir().unwrap();
    let p = credentials(dir.path(), "127.0.0.1");
    let nas = catalogue(dir.path(), "server.icatalog");
    let server_path = nas.path().to_owned();
    let generation = nas.catalog().generation().unwrap();
    let url = start(nas, &p);
    let wrong = dir.path().join("wrong-token");
    std::fs::write(&wrong, "wrong-test-token-0123456789-abcdef").unwrap();
    for token in [None, Some(wrong.to_str().unwrap())] {
        let cp = client_params(&p, &url, token);
        let transport = UreqTransport::from_params(&cp, &url).unwrap();
        for route in ["info", "catalogue", "preview/invalid", "unknown"] {
            assert_eq!(
                transport.get(&format!("{url}/{route}")).unwrap().status,
                401
            );
        }
        for route in ["annotations", "catalogue"] {
            assert_eq!(
                transport
                    .post(&format!("{url}/{route}"), b"invalid")
                    .unwrap()
                    .status,
                401
            );
        }
        let path = dir.path().join("absent.icatalog");
        let mut destination = Context::for_destination(&path).unwrap();
        let err = icatalog_sync::dispatch_guarded(&mut destination, "clone", &cp).unwrap_err();
        assert!(err.to_string().contains("authentication failed"));
        assert!(!err.to_string().contains(TOKEN));
        assert!(!path.exists());
        let mut local = catalogue(
            dir.path(),
            &format!(
                "local-{}.icatalog",
                if token.is_some() { "wrong" } else { "none" }
            ),
        );
        command(
            &mut local,
            "collection",
            &[("action", "create"), ("name", "Local draft")],
        );
        let before = std::fs::read(local.path()).unwrap();
        assert!(icatalog_sync::dispatch_guarded(&mut local, "sync", &cp).is_err());
        assert_eq!(std::fs::read(local.path()).unwrap(), before);
    }
    assert_eq!(
        Context::open_existing(server_path)
            .unwrap()
            .catalog()
            .generation()
            .unwrap(),
        generation
    );
}

#[test]
fn untrusted_expired_and_wrong_hostname_certificates_fail_without_offline_deferral() {
    for (host, expired) in [
        ("127.0.0.1", false),
        ("wrong.invalid", false),
        ("127.0.0.1", true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let p = credentials_with_expiry(dir.path(), host, expired);
        let url = start(catalogue(dir.path(), "server.icatalog"), &p);
        let mut cp = params(&[("url", &url), ("token-file", p.get("token-file").unwrap())]);
        if host == "wrong.invalid" || expired {
            cp.set("ca-file", p.get("ca-file").unwrap());
        }
        let mut local = catalogue(dir.path(), "local.icatalog");
        let before = std::fs::read(local.path()).unwrap();
        let error = icatalog_sync::dispatch_guarded(&mut local, "sync", &cp)
            .unwrap_err()
            .to_string();
        assert!(error.contains("TLS connection failed"), "{error}");
        assert!(!error.contains("deferred"));
        assert_eq!(std::fs::read(local.path()).unwrap(), before);
        let path = dir.path().join("clone.icatalog");
        let mut new = Context::for_destination(&path).unwrap();
        assert!(icatalog_sync::dispatch_guarded(&mut new, "clone", &cp).is_err());
        assert!(!path.exists());
    }
}

#[test]
fn urls_and_credentials_are_validated_before_network_without_echoing_secrets() {
    for url in [
        "http://",
        "https:///",
        "http://secret@nas",
        "http://@nas",
        "https://user:secret@nas",
        "https://nas?secret",
        "https://nas#secret",
        "http://nas\\other",
        " http://nas",
        "http://nas\n",
    ] {
        let error = icatalog_sync::validate_server_url(url)
            .unwrap_err()
            .to_string();
        assert!(!error.contains("secret"));
    }
    for url in [
        "https://nas",
        "https://nas/catalogue/",
        "http://127.0.0.1:8080",
        "https://[::1]:8443",
    ] {
        icatalog_sync::validate_server_url(url).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, TOKEN).unwrap();
    let p = params(&[("token-file", token.to_str().unwrap())]);
    for base in [
        "http://nas",
        "http://localhost",
        "http://127.1",
        "http://2130706433",
        "http://127.0.0.2",
    ] {
        assert!(UreqTransport::from_params(&p, base).is_err(), "{base}");
    }
    for base in ["http://127.0.0.1:8", "http://[::1]:8", "https://nas"] {
        assert!(UreqTransport::from_params(&p, base).is_ok());
    }
    assert!(ServerSecurity::from_params(&p, "0.0.0.0").is_err());
    assert!(ServerSecurity::from_params(&p, "localhost").is_err());
    assert!(ServerSecurity::from_params(&p, "127.0.0.1").is_ok());
}

#[test]
fn malformed_secret_certificate_and_key_files_return_errors_without_panics() {
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    for bytes in [
        Vec::new(),
        b"short".to_vec(),
        vec![b'a'; 4097],
        vec![255; 32],
        format!("{TOKEN}\n\n").into_bytes(),
        format!("{TOKEN}\rsecret").into_bytes(),
    ] {
        std::fs::write(&token, bytes).unwrap();
        let p = params(&[("token-file", token.to_str().unwrap())]);
        assert!(UreqTransport::from_params(&p, "https://nas").is_err());
    }
    for suffix in ["", "\n", "\r\n"] {
        std::fs::write(&token, format!("{TOKEN}{suffix}")).unwrap();
        assert!(UreqTransport::from_params(
            &params(&[("token-file", token.to_str().unwrap())]),
            "https://nas"
        )
        .is_ok());
    }
    let p = credentials(dir.path(), "127.0.0.1");
    let key = PathBuf::from(p.get("tls-key").unwrap());
    for bytes in [
        "",
        "-----BEGIN PRIVATE KEY-----\n!invalid!\n-----END PRIVATE KEY-----",
        "secret-invalid-key",
    ] {
        std::fs::write(&key, bytes).unwrap();
        let error = match ServerSecurity::from_params(&p, "127.0.0.1") {
            Err(e) => e,
            Ok(_) => panic!("accepted invalid key"),
        };
        assert!(!error.to_string().contains("secret-invalid-key"));
    }
    let p = credentials(dir.path(), "127.0.0.1");
    std::fs::write(
        p.get("tls-key").unwrap(),
        rcgen::KeyPair::generate().unwrap().serialize_pem(),
    )
    .unwrap();
    assert!(ServerSecurity::from_params(&p, "127.0.0.1").is_err());
    let mut partial = Params::new();
    partial.set("tls-cert", p.get("tls-cert").unwrap());
    assert!(ServerSecurity::from_params(&partial, "127.0.0.1").is_err());
    for name in ["ca-file", "tls-cert"] {
        let p = credentials(dir.path(), "127.0.0.1");
        std::fs::write(p.get(name).unwrap(), "not a certificate").unwrap();
        if name == "ca-file" {
            assert!(UreqTransport::from_params(&p, "https://nas").is_err());
        } else {
            assert!(ServerSecurity::from_params(&p, "127.0.0.1").is_err());
        }
    }
    let mut directory = Params::new();
    directory.set("token-file", dir.path().to_str().unwrap());
    assert!(UreqTransport::from_params(&directory, "https://nas").is_err());
    let oversized = dir.path().join("oversized.pem");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(1024 * 1024 + 1)
        .unwrap();
    assert!(UreqTransport::from_params(
        &params(&[("ca-file", oversized.to_str().unwrap())]),
        "https://nas"
    )
    .is_err());
}

#[test]
fn redirects_and_origin_changes_never_forward_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, TOKEN).unwrap();
    let destination = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let target = format!("http://{}/info", destination.server_addr());
    let redirector = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", redirector.server_addr());
    let p = params(&[("token-file", token.to_str().unwrap())]);
    let transport = UreqTransport::from_params(&p, &base).unwrap();
    let worker = std::thread::spawn(move || {
        let request = redirector.recv().unwrap();
        assert!(request.headers().iter().any(
            |h| h.field.equiv("Authorization") && h.value.as_str() == format!("Bearer {TOKEN}")
        ));
        request
            .respond(
                tiny_http::Response::empty(302)
                    .with_header(tiny_http::Header::from_bytes("Location", target).unwrap()),
            )
            .unwrap();
    });
    assert_eq!(transport.get(&format!("{base}/info")).unwrap().status, 302);
    worker.join().unwrap();
    assert!(destination
        .recv_timeout(Duration::from_millis(50))
        .unwrap()
        .is_none());
    assert!(transport
        .get(&format!("http://{}/info", destination.server_addr()))
        .is_err());
}

#[test]
fn unauthorized_stalled_and_huge_bodies_are_closed_without_blocking_service() {
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, TOKEN).unwrap();
    let p = params(&[("token-file", token.to_str().unwrap())]);
    let url = start(catalogue(dir.path(), "server.icatalog"), &p);
    let addr = url.strip_prefix("http://").unwrap();
    for length in ["16", "18446744073709551615"] {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(
            stream,
            "POST /annotations HTTP/1.1\r\nHost: localhost\r\nContent-Length: {length}\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    }
    let transport = UreqTransport::from_params(&p, &url).unwrap();
    assert!(client::info(&transport, &url).is_ok());
}

#[test]
fn authenticated_non_edit_routes_cancel_unexpected_stalled_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, TOKEN).unwrap();
    let p = params(&[("token-file", token.to_str().unwrap())]);
    let url = start(catalogue(dir.path(), "server.icatalog"), &p);
    let address = url.strip_prefix("http://").unwrap();
    for (method, route, expected) in [
        ("GET", "info", "200"),
        ("POST", "unknown", "404"),
        ("POST", "catalogue", "405"),
    ] {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(stream, "{method} /{route} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 100\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {expected}")),
            "{response}"
        );
    }
    let transport = UreqTransport::from_params(&p, &url).unwrap();
    assert!(client::info(&transport, &url).is_ok());
}

#[test]
fn authenticated_error_responses_cannot_echo_tokens_into_sync_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, TOKEN).unwrap();
    let service = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", service.server_addr());
    let worker = std::thread::spawn(move || {
        for status in [500, 302, 201] {
            for _ in 0..4 {
                let request = service.recv().unwrap();
                let authorization = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("Authorization"))
                    .map(|header| header.value.as_str())
                    .unwrap_or("none");
                let echoed = format!("server diagnostic: Authorization: {authorization}");
                request
                    .respond_and_close(
                        tiny_http::Response::from_string(echoed).with_status_code(status),
                    )
                    .unwrap();
            }
        }
        let request = service.recv().unwrap();
        let mut bytes = vec![0, 255];
        bytes.extend_from_slice(TOKEN.as_bytes());
        request
            .respond_and_close(tiny_http::Response::from_data(bytes))
            .unwrap();
    });
    let p = params(&[("url", &base), ("token-file", token.to_str().unwrap())]);
    let transport = UreqTransport::from_params(&p, &base).unwrap();
    let mut local = catalogue(dir.path(), "local.icatalog");
    let before = std::fs::read(local.path()).unwrap();
    for status in [500, 302, 201] {
        let response = transport.get(&format!("{base}/info")).unwrap();
        assert_eq!(response.status, status);
        assert!(
            response.body.is_empty(),
            "authenticated errors must not expose response bodies"
        );
        let response = transport.post(&format!("{base}/annotations"), b"").unwrap();
        assert_eq!(response.status, status);
        assert!(response.body.is_empty());
        let error = icatalog_sync::dispatch_guarded(&mut local, "sync", &p)
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!("HTTP {status}")), "{error}");
        assert!(!error.contains(TOKEN), "token exposed in sync diagnostics");
        assert!(!error.contains("Authorization"));
        let public = UreqTransport::new().get(&format!("{base}/info")).unwrap();
        assert_eq!(public.status, status);
        assert!(String::from_utf8(public.body)
            .unwrap()
            .contains("server diagnostic"));
    }
    assert_eq!(std::fs::read(local.path()).unwrap(), before);
    let successful = transport.get(&format!("{base}/catalogue")).unwrap();
    let mut expected = vec![0, 255];
    expected.extend_from_slice(TOKEN.as_bytes());
    assert_eq!(successful.body, expected);
    worker.join().unwrap();
}

#[test]
fn malformed_success_info_cannot_echo_tokens_into_sync_or_clone_errors() {
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, TOKEN).unwrap();
    let service = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", service.server_addr());
    let worker = std::thread::spawn(move || {
        for body in [
            format!("Authorization: Bearer {TOKEN}"),
            format!("protocol=2\nid=secure-test\ngeneration={TOKEN}\n"),
        ] {
            for _ in 0..2 {
                let request = service.recv().unwrap();
                assert!(request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("Authorization")));
                request
                    .respond_and_close(tiny_http::Response::from_string(body.clone()))
                    .unwrap();
            }
        }
    });
    let p = params(&[("url", &base), ("token-file", token.to_str().unwrap())]);
    let mut local = catalogue(dir.path(), "local.icatalog");
    let before = std::fs::read(local.path()).unwrap();
    for _ in 0..2 {
        let error = icatalog_sync::dispatch_guarded(&mut local, "sync", &p)
            .unwrap_err()
            .to_string();
        assert!(error.contains("info"), "{error}");
        assert!(!error.contains(TOKEN));
        assert!(!error.contains("Authorization"));
        let path = dir.path().join("absent.icatalog");
        let mut destination = Context::for_destination(&path).unwrap();
        let error = icatalog_sync::dispatch_guarded(&mut destination, "clone", &p)
            .unwrap_err()
            .to_string();
        assert!(error.contains("info"), "{error}");
        assert!(!error.contains(TOKEN));
        assert!(!error.contains("Authorization"));
        assert!(!path.exists());
    }
    assert_eq!(std::fs::read(local.path()).unwrap(), before);
    worker.join().unwrap();
}
