# iCatalog development guide

Purpose: how to build, test, run and debug. Status vocabulary: **Decided** (owner decision or golden rule), **Proposed** (designed, not built), **Open** (unresolved). The `ivc`, `icatalog-catalog`, `icatalog-engine`, `icatalog-cli` and `icatalog-ui` crates build today; of the `icatalog` commands below `init/open/import/list/find/status/update` exist.

## 1. Prerequisites

- **Decided:** Rust toolchain, one Cargo workspace at the repo root.
- **Decided:** the repo pins Rust `1.99.0` via `rust-toolchain.toml` (components rustfmt, clippy, rust-src, rust-analyzer), so the terminal and rust-analyzer (VS Code) use the same compiler regardless of the machine's default `stable`. rustup installs it on first use.
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

## 2a. Task shortcuts

A root `Taskfile.yml` ([go-task](https://taskfile.dev)) wraps the common commands:

- `task` lists all tasks.
- `task ci`: the full gate (`fmt:check`, `lint`, `test`, in that order); also what CI will run once set up.
- `task test`, `task lint`, `task fmt:check`: the individual gate steps.
- `task layers`: the crate layer-rule test.
- `task run -- <args>`: run the `icatalog` CLI, e.g. `task run -- init my.sqlite`.
- `task gui -- <catalogue>`: run the `icatalog-ui` desktop shell, e.g. `task gui -- photos.sqlite`.

## 3. Workspace layout

Root `Cargo.toml` today: `members = ["ivc-file-research/crates/ivc", "crates/*"]`; `icatalog-catalog` is the first app crate. App crates go in `crates/` (see `docs/architecture.md`).

Adding a crate (**Proposed** procedure):

1. Create `crates/<name>/` with a valid `Cargo.toml`; write it to a temp file outside the crate dirs and `mv` it into place.
2. The glob `crates/*` is already in the root members, so a new crate under `crates/` is picked up automatically.
3. Start with `unsafe_code = "forbid"` and `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
4. Register the crate in the layer test; respect the layering rule.

## 4. Driving the CLI

EXISTS: `cargo run -p icatalog-cli -- <command> <catalogue>` with `init`, `open`, `import`, `list`, `find`, `status` and `update`; also `--help`, `--version` and `commands` (list registered commands). Example: `cargo run -p icatalog-cli -- init photos.sqlite`; import a raster directory with `cargo run -p icatalog-cli -- import <catalogue> <dir> [--recursive false]`; rescan with `cargo run -p icatalog-cli -- update <catalogue> [--path <dir>] [--recursive false]`.

PLANNED (not built): `sync`, `serve`; those rows below are design sketches from `AGENTS.md` section 4. Argument spelling is **Open**.

| Subcommand | Example | Purpose |
|---|---|---|
| `init` / `new` | `icatalog init photos.sqlite` | Create an empty catalogue |
| `open` | `icatalog open photos.sqlite` | Validate and print a summary |
| `import` | `icatalog import photos.sqlite ~/Pictures` | Import a directory (EXISTS, raster only) or a `.ivc`/`.mpcatalog` (not built) |
| `list` / `find` | `icatalog find photos.sqlite beach` | Browse and search |
| `update` | `icatalog update photos.sqlite` | Rescan folders for added/changed/missing files (EXISTS, D14); metadata editing not built |
| `sync` | `icatalog sync photos.sqlite` | Pull/push against the NAS |
| `serve` | `icatalog serve` | Run the NAS HTTP(S) sync service |
| `status` / `info` | `icatalog status photos.sqlite` | Stats, generation, sync state |

Each subcommand is a thin wrapper over an engine command.

## 4a. Driving the GUI

EXISTS (D15): the `icatalog-ui` desktop shell. `cargo run -p icatalog-ui -- <catalogue>` (or `task gui -- <catalogue>`). A missing catalogue file is created and initialised automatically. The left panel lists assets; selecting one shows its stored grid thumbnail and basic metadata. The **Import…** button pick a folder and dispatches the `import` command; **Update** dispatches `update` (whole-catalogue rescan). The status bar shows the last command's outcome. Native desktop only; the wasm build is PLANNED.

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

The repo is under git with a root `.gitignore` covering `target/` and `log/` (in addition to `ivc-file-research/samples/.gitignore`). `log/devlog.md` is PLANNED (local, gitignored): start it and append a terse entry per task.

## 8. Open questions

- Final CLI argument syntax.
- Per-crate `clippy.toml` (allow panics in tests); check whether `ivc` already has the deny attributes before assuming.
- Samples corpus: resolved (CC0, committed; `ivc-file-research/samples/fetch.sh` is an optional re-fetch/verifier; see `docs/decisions.md` D9).
