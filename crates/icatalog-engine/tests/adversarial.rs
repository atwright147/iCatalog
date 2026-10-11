//! Fuzz-style adversarial test (AGENTS golden rule 5): every registered
//! command is driven by hostile parameters, and `import`/`update` are driven
//! by truncated and mutated bytes. The only assertion that matters is that
//! nothing panics. Commands may return `Err`; they must never unwind.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use icatalog_engine::*;
use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};

fn fresh_dir(suffix: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("icat-adv-{suffix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Hostile values for any non-path parameter.
fn general_values() -> Vec<String> {
    let mut v = vec![
        String::new(),
        " ".to_string(),
        "\0".to_string(),
        "a\0b".to_string(),
        "\n\r\t".to_string(),
        "-1".to_string(),
        "0".to_string(),
        "9223372036854775807".to_string(),
        "-9223372036854775808".to_string(),
        "999999999999999999999999999999".to_string(),
        "1e999".to_string(),
        "NaN".to_string(),
        "inf".to_string(),
        "-inf".to_string(),
        "0x10".to_string(),
        "1_000".to_string(),
        "+5".to_string(),
        "1.5".to_string(),
        "--".to_string(),
        "=".to_string(),
        "a=b".to_string(),
        "--path".to_string(),
        "-".to_string(),
        "..".to_string(),
        "../../etc/passwd".to_string(),
        "/".to_string(),
        "/etc/passwd".to_string(),
        "' OR 1=1 --".to_string(),
        "%00%0a".to_string(),
        "\u{202e}reversed".to_string(),
        "🙂🦀🎉".to_string(),
        "مرحبا".to_string(),
        "\u{0301}\u{0301}\u{0301}".to_string(),
    ];
    v.push("a".repeat(100_000));
    v.push("9".repeat(65_536));
    v
}

/// Path values that can never make `import`/`update` walk a real, non-empty
/// tree: everything is empty, a file rather than a directory, or absent.
fn safe_path_values(base: &Path) -> Vec<String> {
    let empty_dir = base.join("empty-scan");
    std::fs::create_dir_all(&empty_dir).unwrap();
    let a_file = base.join("not-a-dir.txt");
    std::fs::write(&a_file, b"x").unwrap();
    let pid = std::process::id();
    vec![
        String::new(),
        "\0".to_string(),
        "a\nb".to_string(),
        format!("{}/nope", empty_dir.display()),
        format!("{}/a/b/c/d/e", empty_dir.display()),
        format!("{}/../i-dont-exist-{pid}", empty_dir.display()),
        format!("{}/../../i-dont-exist-{pid}", empty_dir.display()),
        format!("{}/../passwd-{pid}", empty_dir.display()),
        empty_dir.display().to_string(),
        a_file.display().to_string(),
        "\u{202e}/tmp".to_string(),
        "x".repeat(4096),
    ]
}

/// Parameter sets whose *keys* are hostile or meaningless; commands must
/// ignore unknown keys without panicking.
fn junk_params() -> Vec<Params> {
    let mut out = vec![Params::new()];
    for key in ["", " ", "\0", "a=b", "--path", "path\0name", "🦀"] {
        let mut p = Params::new();
        p.set(key, "hostile\0../value");
        out.push(p);
    }
    let mut long = Params::new();
    long.set("k".repeat(10_000), "v");
    out.push(long);
    let mut many = Params::new();
    for i in 0..1000 {
        many.set(format!("k{i}"), "v\0../");
    }
    out.push(many);
    out
}

/// Hostile parameter sets for one command, derived from its declared params.
fn param_sets(cmd: &dyn Command, general: &[String], paths: &[String]) -> Vec<Params> {
    let mut sets = vec![Params::new()];
    let mut all = Params::new();
    for p in cmd.params() {
        let values = if p.name == "path" { paths } else { general };
        for v in values {
            let mut one = Params::new();
            one.set(p.name, v.clone());
            sets.push(one);
        }
        if let Some(first) = values.first() {
            all.set(p.name, first.clone());
        }
    }
    if !all.is_empty() {
        sets.push(all);
    }
    if cmd.params().iter().any(|p| p.name == "path") {
        for r in [
            "true",
            "false",
            "",
            "maybe",
            "\0",
            "-1",
            "yes",
            "no",
            "4294967296",
        ] {
            let mut p = Params::new();
            p.set("path", format!("i-dont-exist-{}", std::process::id()));
            p.set("recursive", r);
            sets.push(p);
        }
    }
    sets
}

fn assert_no_panic(ctx: &mut Context, id: &str, params: &Params) {
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| dispatch(ctx, id, params)));
    assert!(
        outcome.is_ok(),
        "command '{id}' panicked on params {params:?}"
    );
}

#[test]
fn adversarial_params_uninitialised() {
    let base = fresh_dir("uninit");
    let general = general_values();
    let paths = safe_path_values(&base);
    let mut ctx = Context::open(base.join("cat.sqlite")).unwrap();
    for cmd in commands() {
        // `init` would initialise this context; it is covered in the other test.
        if cmd.id() == "init" {
            continue;
        }
        let mut sets = param_sets(*cmd, &general, &paths);
        sets.extend(junk_params());
        for params in sets {
            assert_no_panic(&mut ctx, cmd.id(), &params);
        }
    }
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn adversarial_params_initialised_covers_every_command() {
    let base = fresh_dir("init");
    let general = general_values();
    let paths = safe_path_values(&base);
    let mut ctx = Context::open(base.join("cat.sqlite")).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    let mut exercised: BTreeSet<&str> = BTreeSet::new();
    for cmd in commands() {
        exercised.insert(cmd.id());
        let mut sets = param_sets(*cmd, &general, &paths);
        sets.extend(junk_params());
        for params in sets {
            assert_no_panic(&mut ctx, cmd.id(), &params);
        }
    }
    for id in command_ids() {
        assert!(
            exercised.contains(id),
            "command '{id}' is not exercised by the fuzz test"
        );
    }
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn truncated_and_mutated_bytes_never_panic() {
    let base = fresh_dir("bytes");
    let scan = base.join("scan");
    std::fs::create_dir_all(&scan).unwrap();

    let mut png = Vec::new();
    {
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([3, 4, 5]));
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
    }

    std::fs::write(scan.join("zero.png"), b"").unwrap();
    std::fs::write(scan.join("sig.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    std::fs::write(scan.join("trunc.png"), &png[..png.len() / 2]).unwrap();
    let noise: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    std::fs::write(scan.join("rand.jpg"), noise).unwrap();
    std::fs::write(scan.join("text.png"), b"definitely not an image").unwrap();
    std::fs::create_dir_all(scan.join("dir.png")).unwrap();
    std::fs::write(scan.join(format!("{}.png", "n".repeat(200))), &png).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(scan.join("zero.png"), scan.join("link.png")).unwrap();

    let mut ctx = Context::open(base.join("cat.sqlite")).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();

    for id in ["import", "update"] {
        let mut p = Params::new();
        p.set("path", scan.to_str().unwrap());
        p.set("recursive", "true");
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| dispatch(&mut ctx, id, &p)));
        assert!(r.is_ok(), "{id} panicked on truncated/mutated bytes");
        assert!(r.unwrap().is_ok(), "{id} should still return an outcome");
    }

    // Mutate and remove files to exercise update's changed and missing paths.
    std::fs::write(scan.join("zero.png"), &png).unwrap();
    let _ = std::fs::remove_file(scan.join("rand.jpg"));
    let mut p = Params::new();
    p.set("path", scan.to_str().unwrap());
    let r = std::panic::catch_unwind(AssertUnwindSafe(|| dispatch(&mut ctx, "update", &p)));
    assert!(r.is_ok(), "update panicked on changed/missing files");
    assert!(r.unwrap().is_ok());

    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}
