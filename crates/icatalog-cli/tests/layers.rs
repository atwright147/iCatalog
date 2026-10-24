//! Layer rule test (AGENTS section 2): crates depend only on lower layers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::path::PathBuf;

const LAYERS: &[(&str, u8, &str)] = &[
    ("ivc", 0, "ivc-file-research/crates/ivc"),
    ("icatalog-catalog", 1, "crates/icatalog-catalog"),
    ("icatalog-media", 1, "crates/icatalog-media"),
    ("icatalog-engine", 2, "crates/icatalog-engine"),
    ("icatalog-sync", 3, "crates/icatalog-sync"),
    ("icatalog-cli", 4, "crates/icatalog-cli"),
    ("icatalog-ui", 4, "crates/icatalog-ui"),
    ("testkit", 4, "crates/testkit"),
];
const GUI: &[&str] = &[
    "egui",
    "egui_kittest",
    "eframe",
    "winit",
    "rfd",
    "muda",
    "objc2-app-kit",
    "objc2-foundation",
];

fn layer_of(name: &str) -> Option<u8> {
    LAYERS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, l, _)| *l)
}

fn dep_names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') {
            let inner = t.trim_matches(|c| c == '[' || c == ']').trim();
            let last = inner.rsplit('.').next().unwrap_or("");
            let is_kind = |k: &str| {
                matches!(
                    k,
                    "dependencies" | "dev-dependencies" | "build-dependencies"
                )
            };
            if is_kind(last) {
                in_deps = true;
            } else {
                in_deps = false;
                // [dependencies.<name>] table form
                let mut parts = inner.splitn(2, '.');
                let head = parts.next().unwrap_or("");
                if is_kind(head) {
                    if let Some(n) = parts.next() {
                        out.push(n.trim().trim_matches('"').to_owned());
                    }
                }
            }
            continue;
        }
        if in_deps {
            if let Some((key, _)) = t.split_once('=') {
                let key = key.trim().trim_matches('"');
                let key = key.split('.').next().unwrap_or(key).trim_matches('"');
                out.push(key.to_owned());
            }
        }
    }
    out
}

#[test]
fn layer_rules_hold() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut violations = Vec::new();
    for (name, layer, dir) in LAYERS {
        let Ok(text) = std::fs::read_to_string(root.join(dir).join("Cargo.toml")) else {
            continue;
        };
        for dep in dep_names(&text) {
            if let Some(dl) = layer_of(&dep) {
                if dl >= *layer {
                    violations.push(format!("{name} (L{layer}) depends on {dep} (L{dl})"));
                }
            }
            if GUI.contains(&dep.as_str()) && *name != "icatalog-ui" {
                violations.push(format!("{name} must not depend on {dep}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "layer violations:\n{}",
        violations.join("\n")
    );
}
