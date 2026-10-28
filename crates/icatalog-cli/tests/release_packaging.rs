//! Exercise the release scripts against executable fixtures, then inspect and run
//! their extracted output. Linux package installation is tested in distro containers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn release_tags_must_match_the_binary_versions() {
    let version = env!("CARGO_PKG_VERSION");
    for tag in [version.to_owned(), format!("v{version}")] {
        let output = Command::new("bash")
            .current_dir(root())
            .args(["packaging/version.sh", &tag])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), version);
        success(output);
    }
    for tag in ["v999.0.0", "", "v0.1.0/../../other", "$(echo injected)"] {
        let output = Command::new("bash")
            .current_dir(root())
            .args(["packaging/version.sh", tag])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Release tag must"));
    }
}

#[cfg(unix)]
#[test]
fn unix_packaging_rejects_unsupported_targets_and_missing_binaries() {
    let scratch = tempfile::tempdir().unwrap();
    for target in [
        "x86_64-apple-darwin",
        "../../outside",
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
    ] {
        let output = Command::new("bash")
            .current_dir(root())
            .args(["packaging/package-unix.sh", target])
            .env("ICATALOG_RELEASE_BINARY_DIR", scratch.path())
            .env("ICATALOG_RELEASE_OUTPUT_DIR", scratch.path().join("output"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!scratch.path().join("output").exists());
    }
}

#[cfg(unix)]
#[test]
fn arch_package_metadata_rejects_spaced_dependencies_and_missing_glibc() {
    let scratch = tempfile::tempdir().unwrap();
    let archive = scratch.path().join("package.tar.gz");
    for (dependencies, valid) in [
        ("depend = glibc>=2.35\ndepend = gcc-libs\n", true),
        ("depend = glibc >= 2.35\ndepend = gcc-libs\n", false),
        ("depend = glibc>=2.35\ndepend = libx11 >= 1\n", false),
        ("depend = gcc-libs\n", false),
    ] {
        std::fs::write(
            scratch.path().join(".PKGINFO"),
            format!("pkgname = icatalog\narch = x86_64\n{dependencies}"),
        )
        .unwrap();
        success(
            Command::new("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(scratch.path())
                .arg(".PKGINFO")
                .env("COPYFILE_DISABLE", "1")
                .output()
                .unwrap(),
        );
        let output = Command::new("bash")
            .current_dir(root())
            .arg("packaging/check-arch-package.sh")
            .arg(&archive)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            valid,
            "{dependencies}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn desktop_archive_contains_both_executables_and_a_working_cli() {
    let scratch = tempfile::tempdir().unwrap();
    let binaries = scratch.path().join("binaries with spaces");
    let assets = scratch.path().join("assets with spaces");
    let extracted = scratch.path().join("extracted");
    std::fs::create_dir_all(&binaries).unwrap();
    std::fs::create_dir_all(&extracted).unwrap();
    // Real CLI bytes stand in for both binaries: package tests need no GUI/display.
    for name in ["icatalog", "icatalog-ui"] {
        std::fs::copy(
            env!("CARGO_BIN_EXE_icatalog"),
            binaries.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        )
        .unwrap();
    }

    #[cfg(target_os = "macos")]
    let (mut package, archive, cli, ui) = {
        let mut package = Command::new("bash");
        package.args(["packaging/package-unix.sh", "aarch64-apple-darwin"]);
        (
            package,
            assets.join(format!(
                "icatalog-{}-macos-arm64.tar.gz",
                env!("CARGO_PKG_VERSION")
            )),
            extracted.join("icatalog"),
            extracted.join("iCatalog.app/Contents/MacOS/icatalog-ui"),
        )
    };
    #[cfg(windows)]
    let (mut package, archive, cli, ui) = {
        let (target, arch) = if cfg!(target_arch = "aarch64") {
            ("aarch64-pc-windows-msvc", "arm64")
        } else {
            ("x86_64-pc-windows-msvc", "x64")
        };
        let mut package = Command::new("pwsh");
        package.args([
            "-NoProfile",
            "-File",
            "packaging/package-windows.ps1",
            "-Target",
            target,
        ]);
        (
            package,
            assets.join(format!(
                "icatalog-{}-windows-{arch}.zip",
                env!("CARGO_PKG_VERSION")
            )),
            extracted.join("icatalog.exe"),
            extracted.join("icatalog-ui.exe"),
        )
    };
    success(
        package
            .current_dir(root())
            .env("ICATALOG_RELEASE_BINARY_DIR", &binaries)
            .env("ICATALOG_RELEASE_OUTPUT_DIR", &assets)
            .output()
            .unwrap(),
    );
    assert!(archive.is_file());
    #[cfg(target_os = "macos")]
    {
        let listing = Command::new("tar")
            .arg("-tzf")
            .arg(&archive)
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&listing.stdout)
            .lines()
            .any(|name| name.split('/').any(|part| part.starts_with("._"))));
        success(listing);
        success(
            Command::new("tar")
                .arg("-xzf")
                .arg(&archive)
                .arg("-C")
                .arg(&extracted)
                .output()
                .unwrap(),
        );
        let plist =
            std::fs::read_to_string(extracted.join("iCatalog.app/Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>iCatalog</string>"));
        success(
            Command::new("codesign")
                .args(["--verify", "--strict"])
                .arg(extracted.join("iCatalog.app"))
                .output()
                .unwrap(),
        );
    }
    #[cfg(windows)]
    success(
        Command::new("pwsh")
            .args([
                "-NoProfile",
                "-Command",
                "Expand-Archive -LiteralPath $env:ARCHIVE -DestinationPath $env:EXTRACTED",
            ])
            .env("ARCHIVE", &archive)
            .env("EXTRACTED", &extracted)
            .output()
            .unwrap(),
    );
    assert!(ui.is_file());
    success(Command::new(&cli).arg("--version").output().unwrap());
    let catalogue = scratch.path().join("smoke.sqlite");
    success(
        Command::new(&cli)
            .arg("init")
            .arg(&catalogue)
            .output()
            .unwrap(),
    );
    success(
        Command::new(&cli)
            .arg("open")
            .arg(catalogue)
            .output()
            .unwrap(),
    );
}
