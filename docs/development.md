# iCatalog development guide

Purpose: how to build, test, run and debug. Status vocabulary: **Decided** (owner decision or golden rule), **Proposed** (designed, not built), **Open** (unresolved). The `ivc`, `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli` and `icatalog-ui` crates build today; of the `icatalog` commands below `init/open/import/list/find/status/update/edit/sync/serve` exist.

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
- `task sync`: test the `icatalog-sync` crate.
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

EXISTS: `cargo run -p icatalog-cli -- <command> <catalogue>` with `init`, `open`, `import`, `list`, `find`, `status`, `update`, `edit`, `sync` and `serve`; also `--help`, `--version` and `commands` (list registered commands). Example: `cargo run -p icatalog-cli -- init photos.sqlite`; import a raster directory with `cargo run -p icatalog-cli -- import photos.sqlite --path <dir>` (add `--kind optical|removable|network` to declare the source kind; default `local`); set annotations with `cargo run -p icatalog-cli -- edit photos.sqlite --id 3 --rating 5 --keyword sunset`.

Sync (D18) is two processes. On the NAS: `cargo run -p icatalog-cli -- serve nas.sqlite --port 8080` (optional `--bind <addr>`, default `127.0.0.1`). On a client, whose local copy shares the NAS catalogue id: `cargo run -p icatalog-cli -- sync local.sqlite --url http://nas:8080`. Plain HTTP only in this slice (`https://` is rejected). An idle sync with nothing changed prints `already up to date` and does not bump the generation; the client records a `local.sqlite.sync-state` fingerprint marker beside the catalogue (D19). A quick local smoke test: init `nas.sqlite`, copy it to `local.sqlite` (so the ids match), serve on one terminal and sync from another.

The table below mirrors `AGENTS.md` section 4; rows marked EXISTS are built. The `info` alias for `status` is **Open**.

| Subcommand | Example | Purpose |
|---|---|---|
| `init` / `new` | `icatalog init photos.sqlite` | Create an empty catalogue |
| `open` | `icatalog open photos.sqlite` | Validate and print a summary |
| `import` | `icatalog import photos.sqlite --path ~/Pictures --kind removable` | Import a directory (EXISTS, raster only; builds a source + folder tree, D21) or a `.ivc`/`.mpcatalog` (not built) |
| `list` / `find` | `icatalog find photos.sqlite beach` | Browse and search |
| `update` | `icatalog update photos.sqlite` | Rescan folders for added/changed/missing files (EXISTS, D14); an absent source root is reported `offline` and never marked missing (D21) |
| `edit` | `icatalog edit photos.sqlite --id 3 --rating 5 --keyword sunset` | Set one asset's keywords, rating and label (EXISTS, D16) |
| `sync` | `icatalog sync photos.sqlite --url http://nas:8080` | Pull/push against the NAS (EXISTS, D18) |
| `serve` | `icatalog serve photos.sqlite --port 8080` | Run the NAS-side HTTP sync service (EXISTS, D18) |
| `status` / `info` | `icatalog status photos.sqlite` | Stats, generation, sync state |

Each subcommand is a thin wrapper over a command: the engine registry (`init`-`edit`) or the sync registry in `icatalog-sync` (`sync`, `serve`); the shell merges both (D18).

## 4a. Driving the GUI

EXISTS (D15): the `icatalog-ui` desktop shell. `cargo run -p icatalog-ui -- <catalogue>` (or `task gui -- <catalogue>`). A missing catalogue file is created and initialised automatically. The left panel holds a source-rooted folder tree grouped under device headers such as "This Mac" or a volume/share name (D21), with collapsible device sections; clicking a device name lists every asset on that device, plus an "Unassigned" entry; the centre has a full-width toolbar with the grid/list switch on the left and the search box (name/path, Enter or Go to apply, × to clear) on the right, above the thumbnails; selecting an asset shows its stored grid thumbnail, basic metadata (including the device and source it was imported from), EXIF and an **Annotations** section (rating 0-5, label 0-7, keyword add/remove/clear) in the right panel. The **Import…** button picks a folder and dispatches the `import` command; **Update** dispatches `update` (whole-catalogue rescan); each annotation click/save dispatches the `edit` command. The status bar shows the last command's outcome, including how many assets belong to offline sources. Native desktop only; the wasm build is PLANNED.

## 5. Debugging tricks

- **Debug builds and breakpoints (Decided):** `cargo build`/`cargo run` use the dev profile (symbols, `debug_assertions`, no optimisation), so an ordinary debugger attaches with no extra flags. There is no in-process inspector like a browser's devtools; you drive a debugger instead.
- **VS Code (recommended):** install the CodeLLDB extension (`vadimcn.vscode-lldb`, recommended in `.vscode/extensions.json`). `.vscode/launch.json` (committed) provides: *Debug icatalog-ui (GUI)*, *Debug icatalog CLI (edit)*, *Debug engine tests*, *Debug edit integration tests* and *Attach to icatalog-ui*. Pick one, set a breakpoint (e.g. in `crates/icatalog-engine/src/commands/edit.rs`) and press F5. Edit the `args` in the config to point at your catalogue. `dbg!(x)` and `eprintln!` work as console-style output and appear in the Debug Console.
- **Terminal:** `rust-lldb target/debug/icatalog-ui -- catalogue.sqlite`, then `breakpoint set --file edit.rs --line <n>` (or by symbol), `run`, `bt`, `frame variable`. `rust-gdb` is the same for gdb users.
- **GUI caveat:** the shell dispatches commands synchronously on the main thread, so the window freezes while execution is stopped at a breakpoint; the catalogue is not left in a bad state and stepping resumes normally.
- **Panic backtraces:** run with `RUST_BACKTRACE=1`. Because `dispatch_guarded` turns an escaped panic into an error, a panicking command shows an error rather than a backtrace; to see the backtrace, call the engine directly (e.g. `cargo test`) instead of through the guard.
- **Logging:** nothing wires a logger yet, so there is no `RUST_LOG`; use `dbg!`/`eprintln!` or the debugger for now.
- **Legacy files:** the `ivc` crate has `examples/dump.rs` for inspecting `.ivc` files; check `ivc-file-research/AGENTS.md` for current usage. Format spec: `ivc-file-research/docs/ivc-format.md`; sample oracles: `ivc-file-research/crates/ivc/tests/samples.rs`.
- **Parallel agents:** use `CARGO_TARGET_DIR=target/agent-<name>` to avoid the Cargo build lock; delete finished `target/agent-*` dirs. Edit only files you own; shared files get small surgical edits.
- **Devlog:** `log/devlog.md` (local, gitignored) records a terse entry per task; read its "Still open" bullets when picking work.
- **Panics:** the CLI `main` and the GUI shell catch an escaped panic around dispatch and import and report it as an error, leaving the catalogue untouched (the `dispatch_guarded` guard, D12). It is a safety net, never a licence to panic.

## 6. Definition of done

Before finishing a task (**Decided**):

- Gate commands from section 2 pass.
- Tests added: synthetic/round-trip for catalogue and sync; a regression test for every crash fix.
- No claim of sync, schema or wasm behaviour that is not built and tested; EXISTS vs PLANNED stays honest.
- Tree builds at every step; no `unwrap`/`expect`/`panic!`/`todo!`/`unsafe` in non-test code.
- Test images are public-domain, never personal photos.
- Devlog entry in `log/devlog.md`.

## 7. Repo caveats

The repo is under git with a root `.gitignore` covering `target/`, `log/` and `*.sqlite` (in addition to `ivc-file-research/samples/.gitignore`). `log/devlog.md` (local, gitignored) records a terse entry per task.

## 8. Open questions

- Final CLI argument syntax.
- Per-crate `clippy.toml` (allow panics in tests); check whether `ivc` already has the deny attributes before assuming.
- Samples corpus: resolved (CC0, committed; `ivc-file-research/samples/fetch.sh` is an optional re-fetch/verifier; see `docs/decisions.md` D9).
