//! Fast input regressions run in the workspace gate; the ignored test exercises
//! the packaged CLI in real disposable containers on an isolated network.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[cfg(unix)]
mod unix {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::time::{Duration, Instant};

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[track_caller]
    fn success(output: Output) -> Output {
        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            text(&output.stdout),
            text(&output.stderr)
        );
        output
    }

    #[test]
    fn image_build_rejects_bad_targets_and_missing_or_corrupt_binaries() {
        let scratch = tempfile::tempdir().unwrap();
        let binary = scratch.path().join("input with spaces");
        for target in [
            "",
            "../../outside",
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
        ] {
            let output = Command::new("bash")
                .current_dir(root())
                .args(["packaging/docker/build.sh", target])
                .env("ICATALOG_DOCKER_BINARY", &binary)
                .output()
                .unwrap();
            assert!(!output.status.success(), "{target}: {output:?}");
            assert!(!scratch.path().join("catalogue").exists());
        }
        std::fs::write(&binary, b"not an ELF executable").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let output = Command::new("bash")
            .current_dir(root())
            .args(["packaging/docker/build.sh", "aarch64-unknown-linux-musl"])
            .env("ICATALOG_DOCKER_BINARY", binary)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
    }

    #[test]
    fn container_test_wrapper_rejects_bad_arguments_before_running_cargo() {
        for (args, engine, message) in [
            (vec!["one", "two"], "docker", "Usage:"),
            (vec!["--unexpected"], "docker", "image tag"),
            (vec!["icatalog:test"], "unknown", "must be docker or podman"),
        ] {
            let output = Command::new("bash")
                .current_dir(root())
                .arg("packaging/docker/test.sh")
                .args(args)
                .env("ICATALOG_CONTAINER_ENGINE", engine)
                .output()
                .unwrap();
            assert!(!output.status.success(), "{output:?}");
            assert!(text(&output.stderr).contains(message), "{output:?}");
        }
    }

    struct Containers {
        engine: String,
        image: String,
        server: String,
        network: String,
        catalogue: PathBuf,
        client: PathBuf,
        media: PathBuf,
        network_created: bool,
        compose_override: Option<PathBuf>,
    }

    impl Containers {
        fn engine(&self, args: &[&str]) -> Output {
            Command::new(&self.engine).args(args).output().unwrap()
        }

        fn run(&self, catalogue: &Path, readonly: bool) -> Command {
            let mut command = Command::new(&self.engine);
            command.args([
                "run",
                "--pull=never",
                "--read-only",
                "--user",
                "1000:1000",
                "--cap-drop=ALL",
                "--security-opt=no-new-privileges",
                "--network",
                &self.network,
                "--mount",
            ]);
            command.arg(format!(
                "type=bind,src={},dst=/catalogue{}",
                catalogue.display(),
                if readonly { ",readonly" } else { "" }
            ));
            command.arg("--mount").arg(format!(
                "type=bind,src={},dst=/media,readonly",
                self.media.display()
            ));
            if self.engine == "podman" {
                // The Fedora Podman VM labels /tmp for host use. Disable label
                // separation only for these disposable mounts; keep user,
                // capability and filesystem restrictions identical to Docker.
                command.arg("--security-opt=label=disable");
            }
            command
        }

        fn cli(&self, catalogue: &Path, args: &[&str]) -> Output {
            self.run(catalogue, false)
                .arg("--rm")
                .arg(&self.image)
                .args(args)
                .output()
                .unwrap()
        }

        fn start(&self) {
            if self.compose_override.is_some() {
                success(self.compose(&["up", "--detach", "--pull", "never"]));
                return;
            }
            // No command override: exercise the image's default ENTRYPOINT/CMD.
            success(
                self.run(&self.catalogue, false)
                    .args(["--init", "--detach", "--name", &self.server])
                    .arg(&self.image)
                    .output()
                    .unwrap(),
            );
        }

        fn stop(&self) {
            if self.compose_override.is_some() {
                success(self.compose(&["stop", "--timeout", "5"]));
            } else {
                success(self.engine(&["stop", "--time", "5", &self.server]));
            }
            let exit =
                success(self.engine(&["inspect", "--format", "{{.State.ExitCode}}", &self.server]));
            assert_eq!(
                text(&exit.stdout).trim(),
                "143",
                "expected forwarded SIGTERM, not timeout/SIGKILL"
            );
            if self.compose_override.is_some() {
                success(self.compose(&["down", "--remove-orphans"]));
            } else {
                success(self.engine(&["rm", &self.server]));
            }
        }

        fn compose(&self, args: &[&str]) -> Output {
            let mut command = Command::new(&self.engine);
            command
                .current_dir(root())
                .args(["compose", "--project-name", &self.network, "--file"])
                .arg(root().join("packaging/docker/compose.yml"))
                .env("ICATALOG_IMAGE", &self.image)
                .env("ICATALOG_CATALOGUE_DIR", &self.catalogue)
                .env("ICATALOG_MEDIA_DIR", &self.media)
                .env("ICATALOG_UID", "1000")
                .env("ICATALOG_GID", "1000")
                .env("ICATALOG_BIND", "127.0.0.1")
                .env("ICATALOG_PORT", "0");
            if let Some(path) = &self.compose_override {
                command.arg("--file").arg(path);
            }
            command.args(args).output().unwrap()
        }

        fn clone_when_ready(&self, path: &str) {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let output = self.cli(&self.client, &["clone", path, "--url", &self.url()]);
                if output.status.success() {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "server readiness/clone failed: {} {}\nlogs: {}",
                    text(&output.stdout),
                    text(&output.stderr),
                    text(&self.engine(&["logs", &self.server]).stderr)
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        fn url(&self) -> String {
            format!("http://{}:8080", self.server)
        }
    }

    impl Drop for Containers {
        fn drop(&mut self) {
            if self.compose_override.is_some() {
                let _ = self.compose(&["down", "--remove-orphans"]);
            }
            let _ = Command::new(&self.engine)
                .args(["rm", "--force", &self.server])
                .output();
            if self.network_created {
                let _ = Command::new(&self.engine)
                    .args(["network", "rm", &self.network])
                    .output();
            }
        }
    }

    fn writable_directory(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path).unwrap();
        // Disposable test data only. Accommodate rootless Podman UID mapping and
        // Docker Desktop bind mounts without changing host library ownership.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777)).unwrap();
    }

    fn synthetic_bmp() -> Vec<u8> {
        // One generated red pixel: 14-byte BMP header, 40-byte DIB, padded BGR row.
        let mut bytes = vec![0_u8; 58];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&58_u32.to_le_bytes());
        bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&1_u32.to_le_bytes());
        bytes[22..26].copy_from_slice(&1_u32.to_le_bytes());
        bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
        bytes[34..38].copy_from_slice(&4_u32.to_le_bytes());
        bytes[56] = 255;
        bytes
    }

    #[test]
    #[ignore = "requires a built NAS image and Docker/Podman; run task docker:test"]
    fn container_round_trip() {
        let scratch = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let unique = format!(
            "icatalog-{}-{}",
            std::process::id(),
            scratch
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace('.', "")
                .to_ascii_lowercase()
        );
        let mut containers = Containers {
            engine: std::env::var("ICATALOG_CONTAINER_ENGINE").unwrap_or_else(|_| "docker".into()),
            image: std::env::var("ICATALOG_DOCKER_IMAGE")
                .unwrap_or_else(|_| "icatalog:local".into()),
            server: format!("{unique}-server"),
            network: unique,
            catalogue: scratch.path().join("catalogue with spaces"),
            client: scratch.path().join("replica with spaces"),
            media: scratch.path().join("media with spaces"),
            network_created: false,
            compose_override: None,
        };
        assert!(matches!(containers.engine.as_str(), "docker" | "podman"));
        for path in [&containers.catalogue, &containers.client, &containers.media] {
            writable_directory(path);
        }
        let bmp = synthetic_bmp();
        std::fs::write(containers.media.join("synthetic.bmp"), &bmp).unwrap();
        success(containers.engine(&["network", "create", &containers.network]));
        containers.network_created = true;
        let catalogue = "/catalogue/catalogue.sqlite";

        // A wrong mount/path must fail, leaving no new catalogue behind.
        let missing = containers.cli(
            &containers.catalogue,
            &["serve", catalogue, "--port", "8080"],
        );
        assert!(!missing.status.success(), "{missing:?}");
        assert!(!containers.catalogue.join("catalogue.sqlite").exists());
        success(containers.cli(&containers.catalogue, &["init", catalogue]));
        success(containers.cli(
            &containers.catalogue,
            &["import", catalogue, "--path", "/media"],
        ));
        success(containers.cli(&containers.catalogue, &["previews", catalogue, "external"]));
        success(containers.cli(&containers.catalogue, &["previews", catalogue, "generate"]));
        let previews: Vec<_> = std::fs::read_dir(containers.catalogue.join("previews"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension() == Some(OsStr::new("jpg")))
            .collect();
        assert_eq!(previews.len(), 1, "expected a generated external JPEG");
        let preview = &previews[0];
        let original_preview = std::fs::read(preview).unwrap();
        let hash = preview.file_stem().unwrap().to_str().unwrap();

        // Both filesystem protections are enforced by the runtime, even for init.
        let media_write =
            containers.cli(&containers.catalogue, &["init", "/media/forbidden.sqlite"]);
        assert!(!media_write.status.success(), "{media_write:?}");
        assert!(!containers.media.join("forbidden.sqlite").exists());
        let denied = containers
            .run(&containers.catalogue, true)
            .arg("--rm")
            .arg(&containers.image)
            .args(["init", "/catalogue/denied.sqlite"])
            .output()
            .unwrap();
        assert!(!denied.status.success(), "{denied:?}");
        assert!(!containers.catalogue.join("denied.sqlite").exists());
        let user = success(containers.engine(&[
            "image",
            "inspect",
            "--format",
            "{{.Config.User}}",
            &containers.image,
        ]));
        assert_eq!(text(&user.stdout).trim(), "1000:1000");

        containers.start();
        containers.clone_when_ready(catalogue);
        success(containers.cli(
            &containers.client,
            &[
                "edit",
                catalogue,
                "--id",
                "1",
                "--rating",
                "5",
                "--keyword",
                "container-round-trip",
            ],
        ));
        success(containers.cli(
            &containers.client,
            &[
                "collection",
                catalogue,
                "create",
                "--name",
                "Container collection",
            ],
        ));
        success(containers.cli(
            &containers.client,
            &["sync", catalogue, "--url", &containers.url()],
        ));
        success(containers.cli(
            &containers.client,
            &[
                "sync",
                catalogue,
                "--url",
                &containers.url(),
                "--preview",
                hash,
            ],
        ));
        assert_eq!(
            std::fs::read(
                containers
                    .client
                    .join("previews")
                    .join(format!("{hash}.jpg"))
            )
            .unwrap(),
            original_preview
        );
        containers.stop();

        // Replace the container, retaining only mounted data; clone a fresh replica.
        if std::env::var("ICATALOG_TEST_COMPOSE").as_deref() == Ok("1") {
            assert_eq!(
                containers.engine, "docker",
                "Compose checks require Docker Compose v2"
            );
            let override_path = scratch.path().join("compose-test.yml");
            // Reuse the isolated test network for container-to-container clients;
            // the supplied service/mount/user/init configuration stays in charge.
            std::fs::write(&override_path, format!("services:\n  icatalog:\n    networks: [testing]\nnetworks:\n  testing:\n    external: true\n    name: {}\n", containers.network)).unwrap();
            containers.compose_override = Some(override_path);
            containers.server = format!("{}-icatalog-1", containers.network);
        }
        containers.start();
        containers.clone_when_ready("/catalogue/after-restart.sqlite");
        if containers.compose_override.is_some() {
            let media_write = containers.engine(&[
                "exec",
                &containers.server,
                "/icatalog",
                "init",
                "/media/forbidden-compose.sqlite",
            ]);
            assert!(!media_write.status.success(), "{media_write:?}");
            assert!(!containers.media.join("forbidden-compose.sqlite").exists());
            let root_write = containers.engine(&[
                "exec",
                &containers.server,
                "/icatalog",
                "init",
                "/forbidden-compose.sqlite",
            ]);
            assert!(!root_write.status.success(), "{root_write:?}");
            let readonly = success(containers.engine(&[
                "inspect",
                "--format",
                "{{.HostConfig.ReadonlyRootfs}}",
                &containers.server,
            ]));
            assert_eq!(text(&readonly.stdout).trim(), "true");
        }
        let found = success(containers.cli(
            &containers.client,
            &[
                "find",
                "/catalogue/after-restart.sqlite",
                "synthetic",
                "--rating",
                "5",
                "--keyword",
                "container-round-trip",
            ],
        ));
        assert!(text(&found.stdout).contains("synthetic.bmp"));
        let collections = success(containers.cli(
            &containers.client,
            &["collection", "/catalogue/after-restart.sqlite", "list"],
        ));
        assert!(text(&collections.stdout).contains("Container collection"));
        assert_eq!(std::fs::read(preview).unwrap(), original_preview);
        assert_eq!(
            std::fs::read(containers.media.join("synthetic.bmp")).unwrap(),
            bmp
        );
        containers.stop();
        if containers.compose_override.is_some() {
            containers.start();
            containers.clone_when_ready("/catalogue/after-compose-replacement.sqlite");
            let found = success(containers.cli(
                &containers.client,
                &[
                    "find",
                    "/catalogue/after-compose-replacement.sqlite",
                    "synthetic",
                    "--rating",
                    "5",
                ],
            ));
            assert!(text(&found.stdout).contains("synthetic.bmp"));
            assert_eq!(std::fs::read(preview).unwrap(), original_preview);
            containers.stop();
            println!("supplied Compose up/stop/down/replacement checks passed");
        }
        success(containers.cli(&containers.catalogue, &["open", catalogue]));
        println!(
            "container init/import/previews/clone/edit/sync/replacement/read-only checks passed"
        );
    }
}
