//! Run the shipped service command with disposable paths; no system service is installed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const UNIT: &str = include_str!("../../../packaging/systemd/icatalog.service");
const HTTPS: &str = include_str!("../../../packaging/systemd/https.conf");

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_icatalog"))
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn service_command(configuration: &str, directory: &Path, port: u16) -> Command {
    let start = configuration
        .lines()
        .filter_map(|line| line.strip_prefix("ExecStart="))
        .rfind(|line| !line.is_empty())
        .unwrap();
    let mut arguments = start.split_ascii_whitespace();
    assert_eq!(arguments.next(), Some("/usr/local/bin/icatalog"));
    let mut command = cli();
    for argument in arguments {
        match argument {
            "/var/lib/icatalog/catalogue.icatalog" => {
                command.arg(directory.join("catalogue.icatalog"));
            }
            "8080" | "8443" => {
                command.arg(port.to_string());
            }
            "/etc/icatalog/sync.token" => {
                command.arg(directory.join("sync.token"));
            }
            "/etc/icatalog/server-chain.pem" => {
                command.arg(directory.join("server-chain.pem"));
            }
            "/etc/icatalog/server-key.pem" => {
                command.arg(directory.join("server-key.pem"));
            }
            _ => {
                command.arg(argument);
            }
        }
    }
    command
}

#[test]
fn systemd_service_limits_identity_writes_and_restart_behavior() {
    // These are deployment invariants: accidentally removing one weakens the
    // installed service even if the CLI's normal integration tests stay green.
    for required in [
        "User=icatalog",
        "Group=icatalog",
        "UMask=0077",
        "ProtectSystem=strict",
        "ProtectHome=yes",
        "ReadWritePaths=/var/lib/icatalog",
        "NoNewPrivileges=yes",
        "CapabilityBoundingSet=",
        "PrivateTmp=yes",
        "Restart=on-failure",
        "StartLimitIntervalSec=60",
        "StartLimitBurst=3",
        "KillSignal=SIGTERM",
        "KillMode=control-group",
        "TimeoutStopSec=30s",
    ] {
        assert!(UNIT.lines().any(|line| line == required), "{required}");
    }
    assert_eq!(UNIT.matches("ReadWritePaths=").count(), 1);
    assert!(!UNIT.contains("ExecStartPre="));
    assert!(UNIT.contains("--bind 127.0.0.1 --port 8080 --token-file"));
    let starts: Vec<_> = HTTPS
        .lines()
        .filter_map(|line| line.strip_prefix("ExecStart="))
        .collect();
    assert_eq!(starts.len(), 2);
    assert!(
        starts[0].is_empty(),
        "the drop-in must reset the HTTP command"
    );
    for option in ["--token-file", "--tls-cert", "--tls-key"] {
        assert!(starts[1].contains(option));
    }
}

#[cfg(unix)]
#[test]
fn systemd_lifecycle_check_rejects_invalid_inputs_before_privileged_operations() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/systemd/test.sh");
    success(
        Command::new("bash")
            .arg("-n")
            .arg(&script)
            .output()
            .unwrap(),
    );
    for arguments in [
        Vec::<&str>::new(),
        vec!["/missing/icatalog"],
        vec!["a", "b"],
    ] {
        let output = Command::new("bash")
            .arg(&script)
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
}

#[test]
fn service_startup_never_initializes_missing_catalogues_or_exposes_bad_secrets() {
    let scratch = tempfile::tempdir().unwrap();
    let catalogue = scratch.path().join("catalogue.icatalog");
    let secret = "invalid-private-fixture-must-not-appear-in-errors";
    for name in ["sync.token", "server-chain.pem", "server-key.pem"] {
        std::fs::write(scratch.path().join(name), secret).unwrap();
    }
    for configuration in [UNIT, HTTPS] {
        let output = service_command(configuration, scratch.path(), 8080)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!catalogue.exists());
    }
    success(cli().arg("init").arg(&catalogue).output().unwrap());
    let before = std::fs::read(&catalogue).unwrap();
    let output = service_command(HTTPS, scratch.path(), 8443)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!error.contains(secret));
    assert!(!error.contains("unknown option"));
    assert_eq!(std::fs::read(&catalogue).unwrap(), before);
    std::fs::remove_file(scratch.path().join("sync.token")).unwrap();
    let output = service_command(UNIT, scratch.path(), 8080)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read(&catalogue).unwrap(), before);
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(directory: &Path, port: u16) -> Server {
    let mut server = Server(
        service_command(UNIT, directory, port)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(server.0.try_wait().unwrap().is_none(), "service exited");
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return server;
        }
        assert!(Instant::now() < deadline, "service did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn service_command_requires_authentication_and_retains_synced_data_after_restart() {
    let scratch = tempfile::tempdir().unwrap();
    let catalogue = scratch.path().join("catalogue.icatalog");
    let token = scratch.path().join("sync.token");
    std::fs::write(
        &token,
        "disposable-service-test-token-with-32-bytes-minimum",
    )
    .unwrap();
    success(cli().arg("init").arg(&catalogue).output().unwrap());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let server = start(scratch.path(), port);
    let url = format!("http://127.0.0.1:{port}");
    let denied = scratch.path().join("denied.icatalog");
    let output = cli()
        .arg("clone")
        .arg(&denied)
        .args(["--url", &url])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!denied.exists());
    let replica = scratch.path().join("replica.icatalog");
    success(
        cli()
            .arg("clone")
            .arg(&replica)
            .args(["--url", &url, "--token-file"])
            .arg(&token)
            .output()
            .unwrap(),
    );
    success(
        cli()
            .arg("collection")
            .arg(&replica)
            .args(["create", "--name", "Survives service restart"])
            .output()
            .unwrap(),
    );
    success(
        cli()
            .arg("sync")
            .arg(&replica)
            .args(["--url", &url, "--token-file"])
            .arg(&token)
            .output()
            .unwrap(),
    );
    drop(server);
    let _restarted = start(scratch.path(), port);
    let fresh = scratch.path().join("after-restart.icatalog");
    success(
        cli()
            .arg("clone")
            .arg(&fresh)
            .args(["--url", &url, "--token-file"])
            .arg(&token)
            .output()
            .unwrap(),
    );
    let listed = success(
        cli()
            .arg("collection")
            .arg(&fresh)
            .arg("list")
            .output()
            .unwrap(),
    );
    assert!(String::from_utf8_lossy(&listed.stdout).contains("Survives service restart"));
}
