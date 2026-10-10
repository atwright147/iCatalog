# iCatalog development guide

Purpose: how to build, test, run and debug. Status vocabulary: **Decided** (owner decision or golden rule), **Proposed** (designed, not built), **Open** (unresolved). Only the `ivc` crate builds today; every `icatalog` command below is PLANNED.

## 1. Prerequisites

- **Decided:** Rust toolchain, one Cargo workspace at the repo root.
- The `wasm32-unknown-unknown` target is **NOT installed** here (`rustup target list --installed` shows only `aarch64-apple-darwin`). To add it: `rustup target add wasm32-unknown-unknown`. No wasm build has been run.
- Later, **Proposed**: trunk + wasm-bindgen for the browser build; musl targets (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`) for the NAS binary.

## 2. Build, test, lint

The gate (**Decided**) for the whole workspace:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

`cargo xtask layers | ci` is PLANNED and joins the gate once it exists.

**Proposed additions (PLANNED, not yet in the gate; D8):**

- cargo-deny licence allow-list + bans (`rawler`, `rawloader`, `ac-ffmpeg`, `ffmpeg-sys*`, libheif, libvips); `ATTRIBUTION.md` diff-check.
- `cargo check -p ivc -p icatalog-media --target wasm32-unknown-unknown` (once the target is installed).
- A fuzz target per public entry point in `icatalog-media` and `ivc`.

Media/metadata dependencies may appear only in `icatalog-media` (see `docs/architecture.md`, section 7).

## 3. Workspace layout

Root `Cargo.toml` today: `members = ["ivc-file-research/crates/ivc"]`. App crates go in `crates/` (see `docs/architecture.md`).

Adding a crate (**Proposed** procedure):

1. Create `crates/<name>/` with a valid `Cargo.toml`; write it to a temp file outside the crate dirs and `mv` it into place.
2. When the first app crate lands, add the glob `crates/*` to the root members.
3. Start with `unsafe_code = "forbid"` and `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
4. Register the crate in the layer test; respect the layering rule.

## 4. Driving the CLI (PLANNED, Proposed)

Nothing below exists; the examples are design sketches from `AGENTS.md` section 4. Argument spelling is **Open**.

| Subcommand | Example | Purpose |
|---|---|---|
| `init` / `new` | `icatalog init photos.sqlite` | Create an empty catalogue |
| `open` | `icatalog open photos.sqlite` | Validate and print a summary |
| `import` | `icatalog import photos.sqlite ~/Pictures` | Import a directory or a `.ivc`/`.mpcatalog` |
| `list` / `find` | `icatalog find photos.sqlite beach` | Browse and search |
| `update` | `icatalog update photos.sqlite` | Rescan folders; edit metadata |
| `sync` | `icatalog sync photos.sqlite` | Pull/push against the NAS |
| `serve` | `icatalog serve` | Run the NAS HTTP(S) sync service |
| `status` / `info` | `icatalog status photos.sqlite` | Stats, generation, sync state |

Each subcommand is a thin wrapper over an engine command.

## 5. Debugging tricks

- **Legacy files:** the `ivc` crate has `examples/dump.rs` for inspecting `.ivc` files; check `ivc-file-research/AGENTS.md` for current usage. Format spec: `ivc-file-research/docs/ivc-format.md`; sample oracles: `ivc-file-research/crates/ivc/tests/samples.rs`.
- **Parallel agents:** use `CARGO_TARGET_DIR=target/agent-<name>` to avoid the Cargo build lock; delete finished `target/agent-*` dirs. Edit only files you own; shared files get small surgical edits.
- **Devlog:** `log/devlog.md` is PLANNED (local, gitignored); append a terse entry per task once it exists. It does not exist yet.
- **Panics:** the shell will catch an escaped panic around dispatch and import (Decided design, not built); never rely on it.

## 6. Definition of done

Before finishing a task (**Decided**):

- Gate commands from section 2 pass.
- Tests added: synthetic/round-trip for catalogue and sync; a regression test for every crash fix.
- No claim of sync, schema or wasm behaviour that is not built and tested; EXISTS vs PLANNED stays honest.
- Tree builds at every step; no `unwrap`/`expect`/`panic!`/`todo!`/`unsafe` in non-test code.
- Test images are public-domain, never personal photos.
- Devlog entry once `log/devlog.md` exists.

## 7. Repo caveats

The repo is **not under git** and has no repo-level `.gitignore` (only `ivc-file-research/samples/.gitignore`). When initialising, add `target/` and `log/` (plus corpus-style dirs per the guides). Until then there is no history or undo: do not delete or overwrite files you did not create.

## 8. Open questions

- Final CLI argument syntax.
- Per-crate `clippy.toml` (allow panics in tests); check whether `ivc` already has the deny attributes before assuming.
- Samples corpus: resolved (CC0, committed; `ivc-file-research/samples/fetch.sh` is an optional re-fetch/verifier; see `docs/decisions.md` D9).
