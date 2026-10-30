# AGENTS.md: iCatalog

iCatalog is an open-source, native, Rust-only rebuild of the discontinued iView Media Pro media catalogue/organiser (the lineage that became Microsoft Expression Media and Phase One Media Pro). Our own catalogue format is **SQLite**; the legacy `.ivc` / `.mpcatalog` format is **import-only**. **Rust only: no JavaScript or TypeScript. No Tauri, Electron or webview shells, ever:** the GUI will be native egui/eframe (also compiled to wasm for the browser, see below), and the primary interface is the `icatalog` CLI. Never add Tauri (or any webview/JS UI framework) as a dependency, build step or packaging target. The product name is always written **iCatalog** in user-facing prose; machine names stay lowercase: crates (`ivc`, `icatalog-*`), the binary (`icatalog`), file names, ids. **Clean-room:** we study behaviour and the on-disk format only; never copy iView/Expression/Phase One code or assets. Read this file first, then the docs.

**Architecture origin:** these guides were adapted from PhotoCraft's AGENTS.md (`https://raw.githubusercontent.com/storytold/photocraft/main/AGENTS.md`, repo `github.com/storytold/photocraft`). PhotoCraft also references a shared `../craftrules` standards repo, which is **not present on this machine**. Decision history: `docs/decisions.md`.

Use `icatalog_catalog::APP_NAME` for the user-facing product name in Rust code (window/menu titles and messages). Documentation and Cargo package descriptions spell it **iCatalog**; machine names remain lowercase.

**Conventions:** this repo has two parts: the app (this guide; `docs/`, future `crates/`) and the clean-room format study in `ivc-file-research/` (guide: `ivc-file-research/AGENTS.md`; spec, 16 sample oracles and the `ivc` parser crate). This is the single root guide and stands alone. All paths are repo-root-relative (e.g. `crates/`). Format knowledge lives in the nested guide `ivc-file-research/AGENTS.md`. The repo is under git with a root `.gitignore` (covers `target/`, `log/`, `*.sqlite`, plus `ivc-file-research/samples/.gitignore`).

Web/wasm build: **a goal, like PhotoCraft** (trunk + wasm-bindgen, Rust only). The browser build uses the same command engine and model as desktop: every client keeps a local SQLite copy and syncs over the same HTTP(S) API (section 5). The browser storage strategy (e.g. OPFS) is an open design item (see `docs/architecture.md`). Nothing wasm-related is built yet.

## 1. Orientation (5 minutes)

| Read | Status | Why |
|---|---|---|
| `docs/decisions.md` | EXISTS | ADR-lite log of owner decisions (hosting, sync transport, web, naming, layout, format) |
| `ivc-file-research/docs/ivc-format.md` | EXISTS (ivc-file-research) | Byte-level spec of the legacy catalogue format, needed for import; see `ivc-file-research/AGENTS.md` |
| `docs/architecture.md` | EXISTS | Crate map, layers, engine/UI seam |
| `docs/development.md` | EXISTS | Build, test, run, drive the CLI, debug tricks |
| `docs/sync.md` | EXISTS | Full protocol and conflict policy (summary in section 5 below) |
| `docs/schema.md` | EXISTS | SQLite catalogue schema and migrations |
| `docs/roadmap.md` | EXISTS | Milestones and **current focus** |

The repo root holds `docs/` (the orientation docs above) and `ivc-file-research/` today, plus the app crates under `crates/` (`icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli`, `icatalog-ui`: EXISTS; the rest below is PLANNED). Nothing listed as PLANNED may be cited as fact until created. (The legacy `samples/` corpus is CC0 and committed; `ivc-file-research/samples/fetch.sh` can optionally re-fetch/verify it.)

## 2. Workspace map

ONE Cargo workspace rooted at the repo root (`Cargo.toml` there: `[workspace]`, resolver 2, `members = ["ivc-file-research/crates/ivc", "crates/*"]`). `cargo test --workspace` / `cargo clippy --workspace` are the gate for both parts.

App crates under `crates/` (EXISTS or PLANNED per each line):

```text
crates/
  icatalog-catalog           EXISTS. L1 SQLite schema + model (rusqlite, "bundled" feature: the chosen dependency)
  icatalog-media             EXISTS. L1 media decode, thumbnails, metadata extraction (only crate allowed media deps)
  icatalog-engine            EXISTS. L2 session + command registry (every action is a command)
  icatalog-sync              EXISTS. L3 whole-snapshot sync: NAS-side HTTP `serve` + client `sync` (D18; deps tiny_http, ureq)
  icatalog-cli               EXISTS. L4 binary `icatalog` (init/open/import/list/find/update/edit/collection/sync/serve/status)
  icatalog-ui                EXISTS. L4 thin native egui desktop shell (browse + thumbnail + metadata + rating/label/keyword edit; init/import/update/edit/collection actions); wasm PLANNED
  testkit                    test helpers
xtask/                       cargo xtask layers | ci   (PLANNED, at repo root)
packaging/          EXISTS: desktop release scripts/launcher (D34); Docker image + compose example PLANNED
```

`ivc` (L0, EXISTS) lives in `ivc-file-research/crates/ivc` by design: standalone with no workspace deps; the app consumes it via a path dependency.

**Layering rule:** a crate may depend only on lower layers; `ivc` depends on nothing in the workspace. Nothing below `icatalog-ui` may use egui, eframe, winit or rfd. Until `cargo xtask layers` exists (PLANNED), the rule is enforced by a documented rule plus a layer test (EXISTS at `crates/icatalog-cli/tests/layers.rs`: parses each `Cargo.toml` and fails on an upward or forbidden dependency). New crates must be registered there. Only `icatalog-media` may hold media/metadata dependencies (image, EXIF, XMP, RAW, video); see D8 in `docs/decisions.md` and `ATTRIBUTION.md` (PLANNED) for licences.

## 3. Golden rules

### Never crash (outranks feature work)

People trust iCatalog with catalogues of their life's photos, and a crash can lose them. A malformed catalogue, a hostile `.ivc`, a bad command param, a corrupt settings file, an interrupted sync or a full disk must produce an error the user can act on, never a panic. Fix a crash before building on top of it.

- **Non-test code never panics.** No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!`. Return the crate's error type and propagate with `?`; use `ok_or(..)?`, `let .. else { return Err(..) }`, `if let`, or `unwrap_or*` where a fallback is truly correct. Unfinished features return an "unsupported" error. The only exception is a provably infallible literal: `#[allow(clippy::expect_used)]` plus `.expect("why it can't fail")`.
- **No `unsafe` at all.** The workspace sets `unsafe_code = "forbid"`. There is no exception crate.
- **Input-derived numbers are hostile.** Use `get()` rather than `[i]`/`[a..b]` for indices from files, params or arithmetic on them; use `checked_*`/`saturating_*` for lengths, offsets and counts; guard division by zero and NaN/inf casts; cap allocations sized by input.
- **Paths, labels and text from foreign machines are hostile strings.** Never assume UTF-8 (legacy catalogues are likely Mac Roman; the check is an open RE item), never assume a path exists or that a file type matches its extension or 4cc, and never trust a catalogued path to stay inside a directory.
- **Bound recursion** with depth limits or seen-sets (folder trees and chunk lists can be deep or cyclic).
- **Don't cascade.** Handle lock poisoning (`lock().unwrap_or_else(PoisonError::into_inner)`) and treat thread joins as `Result`s.
- **Last-resort guard.** The shell (the `icatalog` CLI `main` and the GUI shell) catches an escaped panic around command dispatch and import, reports it as an error and leaves the catalogue untouched. It's a safety net, not a licence to panic. Keep `panic = "unwind"`. Engine dispatch now enforces a per-command SQLite transaction (D23): errors, panics and failed commits roll back that command. Outer shell guards must not claim earlier completed commands were undone. Sync retains its separate backup/verify/swap policy.
- **Prove it.** Every crash fix comes with a small synthetic regression test that panicked before the fix.
- **Enforced by clippy.** Clean crates carry `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`, with tests allowed via `clippy.toml` (PLANNED; check whether `ivc-file-research/crates/ivc` already has them before assuming); new crates start with it.

### Other rules

1. **Everything is a command.** New user-visible behaviour = a command with id, label, params doc, `enabled` and `run`, plus tests. Engine commands live in `icatalog-engine` (EXISTS); the sync commands `sync`/`serve` live in `icatalog-sync` (L3, D18) because the engine (L2) may not depend on it. The CLI and the GUI both dispatch commands by id; the CLI subcommands are thin wrappers, and the L4 shells merge the engine and sync registries. Only pure view/window state belongs to the shell.
2. **Clean-room.** Implement from observation and `ivc-file-research/docs/ivc-format.md`. Third-party assets need a permissive licence and a row in `ATTRIBUTION.md` (PLANNED).
3. **Tests are the gate.** Every change comes with tests. Add synthetic and round-trip tests for the catalogue and sync. Demo/test images must be public-domain and **never personal photos**; never put personal details in URLs or User-Agents.
4. **The UI is thin and data-driven.** `icatalog-ui` holds no logic; all actions go through engine commands.
5. **Never panic on input** (see *Never crash*). Commands return `Err` for any input. A fuzz-style test feeding adversarial params to every command (and truncated/mutated bytes to import/update) EXISTS (`crates/icatalog-engine/tests/adversarial.rs`) and must stay green; `crates/icatalog-sync/tests/sync.rs` does the same for the `sync`/`serve` params (invalid ports only, so `serve` never blocks).
6. **Never break wasm.** Crates from L0 up (except the CLI/server-only parts of `icatalog-sync` and `icatalog-cli`) must stay compilable for `wasm32-unknown-unknown` once that target is installed. Status: the target is NOT installed in this environment today (`rustup target list --installed` shows only `aarch64-apple-darwin`), so `cargo check -p ivc --target wasm32-unknown-unknown` has not been run and no wasm status is claimed.
7. **Sync safety.** A failed or interrupted sync must never destroy the only copy of a catalogue. SQLite writes are transactional; a rejected local state is kept as a timestamped backup before anything is replaced; downloads land in a temp file and are swapped in atomically after an integrity check; no silent data loss, no silent last-writer-wins.
8. **Docs track code.** When reality changes, update the matching doc in the same change (see §7); a stale "PLANNED"/"EXISTS" claim is a bug, not a note.

## 4. CLI and NAS deployment

Requirements: start and update a catalogue from a CLI, and install that CLI on a NAS (bare metal or Docker). The CLI and the `serve` service EXIST (D18); the musl build and Docker packaging are PLANNED.

`icatalog` subcommands (`icatalog init|open|import|list|find|update|edit|collection|sync|serve|status`; `update` (D14), `edit` (D16) and `sync`/`serve` (D18) EXIST) (each maps to a command):

| Subcommand | Purpose |
|---|---|
| `init` / `new <catalogue>` | Create an empty SQLite catalogue |
| `open <catalogue>` | Open/validate and print a summary (and later launch the GUI on it) |
| `import <catalogue> <path>` | EXISTS for raster directories (D13); legacy `.ivc` not built. Import a photo/media directory (creating a source and mirroring its directory tree into folders, grouped under a device in the UI; `--kind local\|optical\|removable\|network`), or a legacy `.ivc`/`.mpcatalog` (via crate `ivc`) |
| `list` / `find <query>` | Browse and search items (name, path, keyword, rating, label, date) |
| `update <catalogue> [--path <dir>]` | EXISTS (D14): rescan folders for added/changed/missing files |
| `edit <catalogue> --id <id> [--rating 0-5] [--label 0-7] [--keyword <names>] [--remove-keyword <names>] [--clear-keywords true]` | EXISTS (D16): set one asset's user annotations (keywords, rating, label); the iView-style editing that used to be sketched under `update` |
| `collection <catalogue> <action>` | EXISTS (D25/D35/D36/D37/D38): manual and Smart Collections; `list`, `create --name <name>`, `create-smart --name <name> <rules>`, `preview-smart <rules>` (read-only count/summary, D37), `rename --id <collection> --name <name>`, `rules --id <collection>`, `set-rules --id <collection> [--name <name>] <rules>`, `show --id <collection>`, `add/remove --id <collection> --asset <asset>` (manual only); `list`/`find --collection <id>` browse members |
| `sync` | EXISTS (D18): pull/push against the NAS catalogue over HTTP (section 5) |
| `serve` | EXISTS (D18): run the NAS-side HTTP sync service |
| `status` / `info` | Catalogue stats, generation, sync state, missing files |

Deployment on a NAS:

- **Bare metal:** a static `icatalog` binary for `x86_64-unknown-linux-musl` (and `aarch64-unknown-linux-musl` for ARM NAS), built with `rusqlite`'s `bundled` feature so there is no system SQLite dependency; copy it and run `icatalog serve`, ideally under systemd.
- **Docker:** a small image holding the musl binary plus a compose example, under `packaging/` (PLANNED), with the catalogue directory as a volume.
- Desktop release automation EXISTS (D34): Windows x64/ARM64 ZIPs, macOS ARM64 app archive (no Intel/universal build), and Linux x86-64 Debian/Fedora/Arch packages. The workflow builds on published releases. Manual dispatch without `release_tag` keeps artifacts only; supplying an existing release tag builds that commit's app code with the dispatched workflow revision's `packaging/` and `Taskfile.yml`, then uploads after checks pass. Go is explicitly installed before Task. Hosted v0.1.0 recovery passed all four platform builds, all three Linux installation checks and final upload; all six assets and checksums are published. Arch constraints are guarded by a generated-metadata regression check. Musl NAS builds and Docker remain PLANNED.

## 5. Local copies + startup sync

Model chosen by the human: **each computer keeps a local SQLite copy; it syncs with the NAS copy at startup.** The transport is an **HTTP(S) API** served by `icatalog serve`; clients (CLI, desktop GUI and the wasm browser build) pull and push over that same API.

- The NAS holds the canonical catalogue, served by `icatalog serve` over HTTP(S). Chosen over a shared-file, single-writer SQLite on SMB/NFS because SQLite locking over network shares is unreliable and corrupts databases; a service owns the file and serialises writes.
- Each client opens only its local copy; the app is fully usable offline. Startup and explicit `icatalog sync` pull newer generations and push local changes. If the NAS is unreachable, sync is deferred and reported, never fatal.
- The catalogue carries a **generation counter**, bumped on every accepted push, plus a per-catalogue id.
- A client keeps a **local change marker** (`name.sqlite.sync-state`) so an unchanged copy is not re-pushed: an idle sync is a no-op, and any local edit is still pushed (D19, `docs/sync.md`).
- **Conflict policy:** a push is refused if the server generation moved since the client's base generation (refuse-and-resync, never silent last-writer-wins). On refusal the client saves its rejected local state as a timestamped backup (e.g. `name.rejected-<UTC timestamp>.sqlite`) before resyncing from the server. Merging local changes (per-item, by stable item id) is a later improvement and must keep the same safety guarantees; until then conflicts are surfaced to the user with the backup path.
- **"Everyone sees the same thing" means sync points, not live co-editing.** Two machines edited between syncs can conflict; the user is told, and nothing is lost.
- **Decided target (D20; not built):** the push direction changes — a client pushes only an **annotation delta** (rating/label/keywords, keyed by stable asset uid), never media, thumbnails or previews; previews come from the server **content-addressed and pull-only** (`GET /preview/<sha>`); and the preview store is chosen per catalogue (**embedded** for a portable single-file `~/Pictures` catalogue, **external** for the NAS). Until that slice is built, the whole-snapshot push and the conflict policy above are what exists. A standalone local catalogue simply never runs `sync`, so it is never affected by any of this.
- Sync is a command (in `icatalog-sync`, D18), so CLI and GUI dispatch the same code. Details (endpoints, auth, TLS) are in `docs/sync.md`.

## 6. Current focus / picking work

Priorities: format knowledge first (tracked in `ivc-file-research/AGENTS.md`), then the data model, then the interfaces. App-side order:

1. SQLite schema (`icatalog-catalog`, `docs/schema.md`).
2. Engine + `icatalog` CLI.
3. Sync service and client.
4. egui browser (desktop shell EXISTS in `icatalog-ui`, D15; wasm later).

Manual Collections EXIST (D25): create/browse/add/remove through the engine, CLI and desktop sidebar on the v1 tables (desktop creation uses a modal, D26); membership does not move assets between folders. Grid/list assets can be dragged onto a collection or edited through a right-click collection menu (D27). Shift-click ranges and Ctrl/⌘-click toggles select multiple collections for a combined asset view (D28). Smart Collections EXIST (D35): the same New Collection modal offers a Smart Collection option with AND rules over format, local capture-date range, camera/lens, minimum rating, label, keyword and filename/path text. They display a (Smart) suffix and live counts, support mixed collection selection/search, and expose Edit Rules… on right-click. Membership is computed, so drops and manual membership checkboxes apply only to manual collections. Schema v6 stores bounded typed rules; existing catalogues migrate automatically.

Smart Collection previews (D37) show a live catalogue-wide match count and plain-language AND summary in Create/Edit Rules before saving. The read-only `collection preview-smart <rules>` action uses the same validation and SQL predicate as saved membership; it creates no collection. Desktop previews debounce edits for 200 ms on a separate worker, clear stale results immediately, and ignore older in-flight responses. Invalid rules and query failures show an error instead of a zero count. Long summaries scroll within the dialog. No schema or sync change.

Every manual and Smart Collection supports `collection rename --id <id> --name <name>` and sidebar right-click Rename… (D36). The modal selects the current name, saves with Rename/Enter and preserves inputs on failure; pending writes disable controls/cancellation. Renaming preserves collection ids, membership, rules, selection and counts. No schema migration.

Smart Collection editing (D38) supports changing Name and rules in the same Save/Enter transaction (`set-rules` accepts optional `--name`; omission preserves the name). Double-clicking a Smart Collection selects it and opens the same dialog as Edit Rules…. Normal Collections open Rename on double-click (D41); busy commands/modals block opening another editor. Invalid names or failed writes preserve both inputs and roll back both changes.

macOS double-click timing follows the system preference (D39), with a 500 ms fallback when AppKit cannot supply it. Headless interaction checks include slower clicks and native-style batched input. Other platforms retain their current egui input settings.

Double-clicking a normal Collection opens Rename with its name focused and selected (D41); Smart Collections open Edit Rules. Both keep the current selection stable (D40/D41). A plain click on an already solely selected Collection waits for the double-click interval before deselecting; another click/action cancels that pending toggle. New selections and modifiers remain immediate. Opening either dialog is blocked during a pending command or another modal.

Asset selections support ranges, toggles and rectangle selection with wheel scrolling. Their Collections context submenu uses cached tri-state membership checkboxes (D29), and each sidebar collection shows a member count with space reserved beside its truncated name (D30).

Thumbnails use the asset's EXIF orientation, including mirrored values (D31). Import/update store oriented pixels; desktop display applies stored metadata to older, unapplied thumbnails without modifying the catalogue or requiring the original media.

List view uses thumbnail rows with filename/rating/dates/filesize, document/camera/lens/focal length, and dimensions/colour/profile columns (D32). It uses the browsing width with an optional Details inspector and horizontal scrolling in narrow windows. Import/update capture supported source colour/ICC descriptions; Update backfills older catalogue entries. Missing metadata stays explicit.

Desktop typography uses 15 pt interface/caption text, 14 pt List metadata and 13 pt small text, with stronger contrast in both themes (D33). Font hinting stays enabled and fractional glyph variants are disabled; eframe's native monitor scale is preserved. Headless layout checks cover 1×/1.5×/2× scales; visual baselines also cover light and Retina List views.

Screenshot comparisons keep a zero failed-pixel allowance. On Linux only, the two 800×600 snapshots with a clipped thumbnail (`catalogue-narrow`, `catalogue-collection-counts`) use a measured YIQ colour tolerance of 1.0 rather than 0.6 for Mesa/Metal rounding (D24). Other snapshots/platforms retain 0.6; baselines are not regenerated to hide CI differences. The adjusted comparisons passed hosted Linux CI.

Native macOS smoke tests isolate the production binary in a temporary app bundle with a unique executable name and bundle identifier, guard the target PID, and wait for menu/picker readiness (D24). Diagnostics are `target/ui-smoke/native-stderr.log` and `native-automation.log`.

`log/devlog.md` EXISTS (gitignored): append a terse entry per task and read its "Still open" bullets when picking work. Create app crates inside `crates/` as they appear.

## 7. Before you finish a task

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

- Don't claim sync, schema or wasm behaviour that isn't built and tested; keep EXISTS vs PLANNED honest.
- **Keep the docs true — same change, same commit.** Docs are part of the definition of done, not an afterthought. If a change alters what EXISTS vs PLANNED, the crate map, the schema, the CLI surface or a design decision, update the matching doc in the same change: `docs/roadmap.md` (milestone status), `docs/architecture.md` (crate map and layers), `docs/schema.md` (SQLite schema), `docs/decisions.md` (add a new D-numbered entry when you choose an approach), the root `Taskfile.yml` (see the next bullet) and this file. A reader must be able to trust that "EXISTS" means built and tested and "PLANNED" means not yet built; if code and docs disagree the doc is the bug — fix it before you finish, and never claim behaviour a doc still lists as PLANNED until a crate and its tests back it.
- Append a terse entry to `log/devlog.md` (what landed, what's still open).
- **Keep `Taskfile.yml` current — same change, same commit.** The root `Taskfile.yml` (go-task: `task --list`) is the canonical command surface, so whenever a change adds, renames or removes a workspace command, crate, or gate step, update the matching task in the same change (and `docs/development.md`). Today it exposes `check`, `build`, `test`, `fmt`, `fmt:check`, `lint`, `layers`, `ci`, `doc`, `clean`, `run`, `gui`, and per-crate `catalog`/`media`/`engine`/`ui`. `task ui:test` runs headless UI checks (also included in workspace tests); `task ui:visual` compares reviewed screenshot baselines, `task ui:visual:update` explicitly regenerates them for review, and `task ui:smoke` runs native macOS checks with desktop permissions (D24). `task scale` is the opt-in 10,000-image workflow and large sync validation (requires a localhost listener). `task ci` is the full gate (`fmt:check`, then `lint`, then `test`, sequentially). Prefer `task <name>` over ad-hoc invocations so the documented surface stays the one people use; if a task's command changes, change the task — don't add a parallel path.
- `task release:build -- --target <triple>` builds the locked release CLI and desktop app; `task release:package -- <triple>` packages them under `target/release-assets`; `task release:test` runs archive/version and Arch metadata regression tests (also included in workspace tests). Linux packaging requires FPM 1.17.0, rpm, bsdtar and zstd; Windows packaging uses PowerShell. See `docs/development.md` for the release matrix and prerequisites.
- Keep the tree building at every step. `cargo xtask layers` joins this list once it exists.

## 8. Parallel agents

- Use your own target dir (`CARGO_TARGET_DIR=target/agent-<name>`) to avoid the Cargo build lock, and edit only files you own. Shared files (root `Cargo.toml`, engine command registry) get small, surgical edits; re-read before editing.
- Put new commands in a new module rather than growing a shared file.
- If someone else's in-progress edit breaks the build, wait and retry; don't fix their files.
- Keep every `Cargo.toml` valid at all times. Create or rewrite manifests atomically: write to a temp file outside the crate directories, then `mv` it into place.
- Delete `target/agent-*` dirs of finished agents.
- **Stay inside the worktree.** Run commands from the repo root with repo-relative paths (use the tool's `workdir` for a crate directory). Do not `cd` to absolute paths, climb out with `..`, or touch files outside the repo (use the pre-approved temp dir for scratch work) unless it is really, really necessary; if it is, say why first.

## 9. Where things are tracked

- Real today: app-side, `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync` (whole-snapshot `serve`/`sync`, D18), the `icatalog` CLI and the `icatalog-ui` desktop shell; `ivc-file-research/docs/ivc-format.md` and `ivc-file-research/crates/ivc/tests/samples.rs` back the import path.
- EXISTS: `log/devlog.md` (local, gitignored; append a terse entry per task).
- Open question: whether the sync service is mandatory or also offers a simple share-file mode. (Samples corpus: resolved, see `ivc-file-research/samples/fetch.sh`.)
