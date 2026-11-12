#[cfg(feature = "ssl-rustls")]
use std::convert::TryInto;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tiny_http::{Response, Server};

#[test]
fn refusal_closes_unread_small_huge_chunked_and_continue_bodies_promptly() {
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = server.server_addr().to_ip().unwrap();
    let bodies = [
        "Content-Length: 1\r\n".to_owned(),
        "Content-Length: 4096\r\n".to_owned(),
        format!("Content-Length: {}\r\n", usize::MAX),
        "Content-Length: 4096\r\nExpect: 100-continue\r\n".to_owned(),
        "Transfer-Encoding: chunked\r\n".to_owned(),
    ];
    let count = bodies.len();
    let worker = std::thread::spawn(move || {
        for _ in 0..count {
            let request = server
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap();
            request.respond_and_close(Response::empty(401)).unwrap();
        }
        let request = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        request.respond(Response::from_string("healthy")).unwrap();
    });
    for body in bodies {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(stream, "POST / HTTP/1.1\r\nHost: localhost\r\n{body}\r\n").unwrap();
        // Intentionally keep the sending side open and send no body at all.
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 401"), "{}", response);
        assert!(response.contains("Connection: close"), "{}", response);
        assert!(!response.contains("100 Continue"), "{}", response);
    }
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.contains("healthy"));
    worker.join().unwrap();
}

#[cfg(feature = "ssl-rustls")]
#[test]
fn tls_refusal_closes_before_unread_body_cleanup() {
    let generated = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(generated.cert.der().clone()).unwrap();
    let server = Server::https(
        "127.0.0.1:0",
        tiny_http::SslConfig {
            certificate: generated.cert.pem().into_bytes(),
            private_key: generated.key_pair.serialize_pem().into_bytes(),
        },
    )
    .unwrap();
    let address = server.server_addr().to_ip().unwrap();
    let worker = std::thread::spawn(move || {
        for _ in 0..2 {
            let request = server
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap();
            request.respond_and_close(Response::empty(401)).unwrap();
        }
    });
    let client_config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let client_config = std::sync::Arc::new(client_config);
    for _ in 0..2 {
        let socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let connection =
            rustls::ClientConnection::new(client_config.clone(), "localhost".try_into().unwrap())
                .unwrap();
        let mut stream = rustls::StreamOwned::new(connection, socket);
        write!(stream, "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 999999999\r\nExpect: 100-continue\r\n\r\n").unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        // Abrupt shutdown deliberately interrupts unread bodies; HTTP framing
        // still gives the client the complete empty final response.
        match stream.read_to_string(&mut response) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
            Err(error) => panic!("{}", error),
        }
        assert!(response.starts_with("HTTP/1.1 401"), "{}", response);
        assert!(!response.contains("100 Continue"), "{}", response);
    }
    worker.join().unwrap();
}

#[cfg(feature = "ssl-rustls")]
#[test]
fn invalid_tls_pem_returns_errors_without_unwinding() {
    let generated = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate = generated.cert.pem().into_bytes();
    for key in [
        "",
        "not a key",
        "-----BEGIN PRIVATE KEY-----\n!\n-----END PRIVATE KEY-----\n",
    ] {
        let result = std::panic::catch_unwind(|| {
            Server::https(
                "127.0.0.1:0",
                tiny_http::SslConfig {
                    certificate: certificate.clone(),
                    private_key: key.as_bytes().to_vec(),
                },
            )
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
    assert!(Server::https(
        "127.0.0.1:0",
        tiny_http::SslConfig {
            certificate: b"not a certificate".to_vec(),
            private_key: generated.key_pair.serialize_pem().into_bytes(),
        }
    )
    .is_err());
}
