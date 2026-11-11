# AGENTS.md: iCatalog

iCatalog is an open-source, native, Rust-only rebuild of the discontinued iView Media Pro media catalogue/organiser (the lineage that became Microsoft Expression Media and Phase One Media Pro). Our own catalogue format is **SQLite**; the legacy `.ivc` / `.mpcatalog` format is **import-only**. **Rust only: no JavaScript or TypeScript. No Tauri, Electron or webview shells, ever:** the GUI will be native egui/eframe (also compiled to wasm for the browser, see below), and the primary interface is the `icatalog` CLI. Never add Tauri (or any webview/JS UI framework) as a dependency, build step or packaging target. The product name is always written **iCatalog** in user-facing prose; machine names stay lowercase: crates (`ivc`, `icatalog-*`), the binary (`icatalog`), file names, ids. **Clean-room:** we study behaviour and the on-disk format only; never copy iView/Expression/Phase One code or assets. Read this file first, then the docs.

**Architecture origin:** these guides were adapted from PhotoCraft's AGENTS.md (`https://raw.githubusercontent.com/storytold/photocraft/main/AGENTS.md`, repo `github.com/storytold/photocraft`). PhotoCraft also references a shared `../craftrules` standards repo, which is **not present on this machine**. Decision history: `docs/decisions.md`.

Use `icatalog_catalog::APP_NAME` for the user-facing product name in Rust code (window/menu titles and messages). Documentation and Cargo package descriptions spell it **iCatalog**; machine names remain lowercase.

**Conventions:** this repo has two parts: the app (this guide; `docs/`, future `crates/`) and the clean-room format study in `ivc-file-research/` (guide: `ivc-file-research/AGENTS.md`; spec, 16 sample oracles and the `ivc` parser crate). This is the single root guide and stands alone. All paths are repo-root-relative (e.g. `crates/`). Format knowledge lives in the nested guide `ivc-file-research/AGENTS.md`. The repo is under git with a root `.gitignore` (covers `target/`, `log/`, `*.sqlite`, `*.icatalog`, plus `ivc-file-research/samples/.gitignore`). New catalogues prefer `.icatalog`; existing `.sqlite` files remain supported, and opening verifies catalogue contents before migration (D89).

Web/wasm build: **a goal, like PhotoCraft** (trunk + wasm-bindgen, Rust only). The browser build uses the same command engine and model as desktop: every client keeps a local SQLite copy and syncs over the same HTTP(S) API (section 5). The browser storage strategy (e.g. OPFS) is an open design item (see `docs/architecture.md`). Nothing wasm-related is built yet.

## 1. Orientation (5 minutes)

| Read | Status | Why |
|---|---|---|
| `docs/decisions.md` | EXISTS | ADR-lite log of owner decisions (hosting, sync transport, web, naming, layout, format) |
| `ivc-file-research/docs/ivc-format.md` | EXISTS (ivc-file-research) | Byte-level spec of the legacy catalogue format, needed for import; see `ivc-file-research/AGENTS.md` |
| `docs/architecture.md` | EXISTS | Crate map, layers, engine/UI seam |
| `docs/development.md` | EXISTS | Build, test, run, drive the CLI, debug tricks |
| `docs/sync.md` | EXISTS | Full protocol and conflict policy (summary in section 5 below) |
| `docs/sync-user-guide.md` | EXISTS | User setup, everyday sync, offline work, conflict recovery and previews |
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
  icatalog-sync              EXISTS. L3 server-authoritative sync: revision-checked annotation/Collection uploads, catalogue/preview pulls (D20/D44; tiny_http, ureq)
  icatalog-cli               EXISTS. L4 binary `icatalog` (init/open/import/list/find/update/edit/collection/previews/clone/sync/serve/status)
  icatalog-ui                EXISTS. L4 thin native egui desktop shell (browse + thumbnail + metadata + rating/label/keyword edit; init/import/update/edit/collection actions plus Connect/Clone/Sync dialogs, D46); wasm PLANNED
  testkit                    test helpers
xtask/                       cargo xtask layers | ci   (PLANNED, at repo root)
packaging/          EXISTS: desktop release scripts/launcher (D34), static x86-64/ARM64 Linux musl CLI tooling (D58), Docker image + Compose example (D60)
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
- **CLI OS arguments (D61).** Read with `args_os`; pass catalogue paths byte-exactly and report a normal open error if the filesystem rejects one. Reject non-UTF-8 command names and text/option values before the string-keyed command engine. Do not use lossy path conversion to choose a different file. The desktop entry point also reads its path with `args_os`; Desktop Import rejects non-UTF-8 selected directories before dispatch (D68).
- **Bound recursion** with depth limits or seen-sets (folder trees and chunk lists can be deep or cyclic).
- **Don't cascade.** Handle lock poisoning (`lock().unwrap_or_else(PoisonError::into_inner)`) and treat thread joins as `Result`s.
- **Last-resort guard.** The shell (the `icatalog` CLI `main` and the GUI shell) catches an escaped panic around command dispatch and import, reports it as an error and leaves the catalogue untouched. It's a safety net, not a licence to panic. Keep `panic = "unwind"`. Engine dispatch now enforces a per-command SQLite transaction (D23): errors, panics and failed commits roll back that command. Outer shell guards must not claim earlier completed commands were undone. Sync retains its separate backup/verify/swap policy.
- **Prove it.** Every crash fix comes with a small synthetic regression test that panicked before the fix.
- **Enforced by clippy.** Clean crates carry `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`, with tests allowed via `clippy.toml` (PLANNED; check whether `ivc-file-research/crates/ivc` already has them before assuming); new crates start with it.

### Other rules

1. **Everything is a command.** New user-visible behaviour = a command with id, label, params doc, `enabled` and `run`, plus tests. Engine commands live in `icatalog-engine` (EXISTS); the sync commands `clone`/`sync`/`serve` live in `icatalog-sync` (L3, D18) because the engine (L2) may not depend on it. The CLI and the GUI both dispatch commands by id; the CLI subcommands are thin wrappers, and the L4 shells merge the engine and sync registries. Only pure view/window state belongs to the shell.
2. **Clean-room.** Implement from observation and `ivc-file-research/docs/ivc-format.md`. Third-party assets need a permissive licence and a row in `ATTRIBUTION.md` (PLANNED).
3. **Tests are the gate.** Every change comes with tests. Add synthetic and round-trip tests for the catalogue and sync. Demo/test images must be public-domain and **never personal photos**; never put personal details in URLs or User-Agents.
4. **The UI is thin and data-driven.** `icatalog-ui` holds no logic; all actions go through engine commands.
5. **Never panic on input** (see *Never crash*). Commands return `Err` for any input. A fuzz-style test feeding adversarial params to every command (and truncated/mutated bytes to import/update) EXISTS (`crates/icatalog-engine/tests/adversarial.rs`) and must stay green; `crates/icatalog-sync/tests/sync.rs` does the same for the `sync`/`serve` params (invalid ports only, so `serve` never blocks).
6. **Never break wasm.** Crates from L0 up (except the CLI/server-only parts of `icatalog-sync` and `icatalog-cli`) must stay compilable for `wasm32-unknown-unknown` once that target is installed. Status: the target is NOT installed in this environment today (`rustup target list --installed` shows only `aarch64-apple-darwin`), so `cargo check -p ivc --target wasm32-unknown-unknown` has not been run and no wasm status is claimed.
7. **Sync safety.** A failed or interrupted sync must never destroy the only copy of a catalogue. SQLite writes are transactional; a rejected local state is kept as a timestamped backup before anything is replaced; downloads land in a temp file and are swapped in atomically after an integrity check; no silent data loss, no silent last-writer-wins.
8. **Docs track code.** When reality changes, update the matching doc in the same change (see §7); a stale "PLANNED"/"EXISTS" claim is a bug, not a note.

## 4. CLI and NAS deployment

Requirements: start and update a catalogue from a CLI, and install that CLI on a NAS (bare metal or Docker). The CLI and the `serve` service EXIST (D18); native static musl build/archive tasks and CI jobs EXIST (D58; new hosted execution/publication unverified). Docker/Compose packaging EXISTS (D60); images are built locally, with no registry publication.

`icatalog` subcommands (`icatalog init|open|import|list|find|update|edit|collection|previews|clone|sync|serve|status`; `update` (D14), `edit` (D16) and `sync`/`serve` (D18) EXIST) (each maps to a command):

| Subcommand | Purpose |
|---|---|
| `init` / `new <catalogue>` | Create an empty SQLite catalogue |
| `open <catalogue>` | Open/validate an existing catalogue and print a summary (and later launch the GUI on it); only `init` creates a missing CLI catalogue |
| `import <catalogue> <path>` | EXISTS for raster directories (D13); legacy `.ivc` not built. Import a photo/media directory (creating a source and mirroring its directory tree into folders, grouped under a device in the UI; `--kind local\|optical\|removable\|network`), or a legacy `.ivc`/`.mpcatalog` (via crate `ivc`) |
| `list` / `find <query>` | Browse and search items (name, path, keyword, rating, label, date); name/path substring searches handle Unicode case variants (D83); `find --rating 0` includes all assets and `--label 0` includes assets without annotations |
| `update <catalogue> [--path <dir>]` | EXISTS (D14): rescan folders for added/changed/missing files |
| `edit <catalogue> --id <id> [--rating 0-5] [--label 0-7] [--keyword <names>] [--keyword-exact <name>] [--remove-keyword <names>] [--remove-keyword-exact <name>] [--clear-keywords true]` | EXISTS (D16): set one asset's user annotations (keywords, rating, label); exact options preserve commas in one keyword; unchanged rating/label values do not write (D82) |
| `collection <catalogue> <action>` | EXISTS (D25/D35/D36/D37/D38): manual and Smart Collections; `list`, `create --name <name>`, `create-smart --name <name> <rules>`, `preview-smart <rules>` (read-only count/summary, D37), `rename --id <collection> --name <name>`, `rules --id <collection>`, `set-rules --id <collection> [--name <name>] <rules>`, `show --id <collection>`, `add/remove --id <collection> --asset <asset>` (manual only); `list`/`find --collection <id>` browse members |
| `previews <catalogue> status\|generate\|external\|embed` | EXISTS (D20/D44): generate oriented 1920-pixel JPEG previews and convert embedded/external storage |
| `clone <catalogue> --url <server>` | EXISTS (D45): download and validate a new local replica, ready for edits; refuses existing destinations |
| `sync` | EXISTS (D20/D44): push user-edit deltas and pull the NAS catalogue; `--preview <sha>` fetches one referenced preview |
| `serve` | EXISTS (D18): run the NAS-side HTTP sync service |
| `status` / `info` | Catalogue stats, generation, sync state, missing files |

Deployment on a NAS:

- **Bare metal:** `task nas:build -- <triple>` builds a static CLI for `x86_64-unknown-linux-musl` or `aarch64-unknown-linux-musl` on native Linux of the same architecture, using musl-gcc and bundled SQLite. `task release:package -- <triple>` creates a CLI-only tar archive; `task nas:test -- <triple>` runs native release CLI tests and checks the extracted ELF/version/catalogue creation/reopen. No dynamic interpreter/shared libraries are allowed. `nas.yml` provides native PR/main/manual jobs, reused by the release workflow. Copy the binary and run `icatalog serve`; service installation remains manual.
- **Docker (D60):** `task docker:build -- <musl-triple> [image]` packages an existing verified static CLI into a scratch image, using a minimal generated context. `task docker:test -- [image]` verifies generated-media import, external previews, clone/edit/Collection sync, permissions, stop signals and persistence across container replacement. `packaging/docker/compose.yml` uses a configurable non-root UID/GID, init process, persistent catalogue-directory bind mount, read-only media/rootfs and loopback publication by default. Host directories and the served catalogue must already exist; `serve` opens without SQLite's create flag. Native x86-64/ARM64 CI adds actual Compose lifecycle checks. No registry publication or systemd installer. Setup: `docs/nas-docker.md`.
- Desktop release automation EXISTS (D34): Windows x64/ARM64 ZIPs, macOS ARM64 app archive (no Intel/universal build), and Linux x86-64 Debian/Fedora/Arch packages. The workflow builds on published releases. Manual dispatch without `release_tag` keeps artifacts only; supplying an existing release tag builds that commit's app code with the dispatched workflow revision's `packaging/` and `Taskfile.yml`, then uploads after checks pass. Go is explicitly installed before Task. Hosted v0.1.0 recovery passed all four desktop platform builds, all three Linux installation checks and final upload; the original six assets and checksums are published. Arch constraints are guarded by a generated-metadata regression check. D58 adds two NAS CLI archives and makes publication wait for both native musl jobs (eight assets on future runs); new hosted execution/publication remains unverified. D60 container builds/tests are separate and do not publish images.

## 5. Local copies + sync

Each computer keeps a local SQLite copy; `icatalog serve` owns the canonical NAS catalogue. HTTP protocol v2 (D20/D44) pushes only rating/label/keyword and Collection edits keyed by stable UIDs, never media, thumbnails or previews. Catalogue and SHA-256-addressed previews are pull-only. Plain HTTP exists; Desktop Connect/Clone and explicit Sync dialogs exist (D46); TLS/auth and automatic startup sync are not built.

- Generation advances transactionally on catalogue-data writes, including server imports/updates and direct CRUD. Per-asset annotation revisions and per-Collection revisions prevent silent last-writer-wins; independent entities sync independently. Conflicting drafts survive in a timestamped backup and conflict keys are reported.
- `clone <new-local-file> --url <server>` creates a replica directly from a live server with its baseline established (D45). No init/manual copy/extra first sync is needed. Existing destinations, SQLite sidecars and the bare `:memory:` path are refused before transfer (D69); verified staged downloads are published without overwriting a racing destination. Parent directories must exist.
- A replica baseline is stored inside SQLite. For manually copied catalogues, first v2 sync downloads server state and backs up unknown local edits without uploading them; this also applies to D18/D19 copies whose old fingerprint sidecars cannot supply per-item history. Subsequent sync pushes pending edits before pulling. Idle sync is a no-op; offline catalogue sync is deferred.
- Collection deltas include creation, names, Smart rules and manual membership using stable member UIDs. Deletion/manual-to-Smart conversion remain unsupported. Established replicas reject media-derived writes; desktop Import/Update disable while annotation/Collection edits remain available. Rating/label edits preserve existing server-owned captions and credits on replicas; clients cannot change that text.
- Downloaded files undergo integrity, foreign-key, ID, generation and user-state checks, then receive a new baseline. The local SQLite writer is reserved during sync. Sync refuses catalogue symlinks and multi-link files on Unix and Windows before transfer, since replacement would detach the replica from another path (D74/D81). A flushed, exclusively created backup precedes atomic replacement; failed transfers/backups leave local work intact. The server uses consistent SQLite snapshots and reads up to four upload bodies concurrently; another concurrent upload receives retryable HTTP 503 (D88). Whole-snapshot uploads are retired (405).
- `previews status|generate|external|embed` exists. Explicit generation uses original media on the server/standalone machine, creating oriented JPEGs (quality 80, long edge at most 1920, no upscaling). Imported TIFFs render as raster images; RAW files use embedded JPEG previews. Storage is per-catalogue: embedded by default for a portable single file, external in `previews/<sha>.jpg` for NAS use. Grid thumbnails always stay embedded. Conversion verifies every blob before committing. `sync --preview <sha>` lazily fetches verified bytes using GET only; desktop full-size preview viewing remains unbuilt.
- Desktop File → Connect to Server… clones into a new local file; File → Sync… runs explicit sync on a worker and refreshes only an existing, initialised catalogue after transfer (D75). The URL is remembered in a local `<catalogue>.sync-url` preference file; CLI commands still require `--url`. First sync of an untracked copy requires acknowledging backup-first replacement. Offline/conflict/backup results stay visible; automatic sync remains unbuilt (D46).
- Standalone catalogues simply never run sync. SQLite stays in DELETE journal mode, without WAL sidecars. Protocol details, bounds and recovery behavior are in `docs/sync.md`.

## 6. Current focus / picking work

Priorities: format knowledge first (tracked in `ivc-file-research/AGENTS.md`), then the data model, then the interfaces. App-side order:

1. SQLite schema (`icatalog-catalog`, `docs/schema.md`).
2. Engine + `icatalog` CLI.
3. Sync service and client.
4. egui browser (desktop shell EXISTS in `icatalog-ui`, D15; wasm later).

Manual Collections EXIST (D25): create/browse/add/remove through the engine, CLI and desktop sidebar on the v1 tables (desktop creation uses a modal, D26); membership does not move assets between folders. Grid/list assets can be dragged onto a collection or edited through a right-click collection menu (D27). Shift-click ranges and Ctrl/⌘-click toggles select multiple collections for a combined asset view (D28). Smart Collections EXIST (D35): the same New Collection modal offers a Smart Collection option with AND rules over format, local capture-date range, camera/lens, minimum/maximum rating, label, keyword and filename/path text. They display a (Smart) suffix and live counts, support mixed collection selection/search, and expose Edit Rules… on right-click. Membership is computed, so drops and manual membership checkboxes apply only to manual collections. Schema v8 stores bounded typed rules, including an optional inclusive maximum rating (D57); existing catalogues migrate automatically.

Smart Collection previews (D37) show a live catalogue-wide match count and plain-language AND summary in Create/Edit Rules before saving. The read-only `collection preview-smart <rules>` action uses the same validation and SQL predicate as saved membership; it creates no collection. Desktop previews debounce edits for 200 ms on a separate worker, clear stale results immediately, and ignore older in-flight responses. Invalid rules and query failures show an error instead of a zero count. Long summaries scroll within the dialog. No schema or sync change.

Every manual and Smart Collection supports `collection rename --id <id> --name <name>` and sidebar right-click Rename… (D36). The modal selects the current name, saves with Rename/Enter and preserves inputs on failure; pending writes disable controls/cancellation. Renaming preserves collection ids, membership, rules, selection and counts. No schema migration.

Smart Collection editing (D38) supports changing Name and rules in the same Save/Enter transaction (`set-rules` accepts optional `--name`; omission preserves the name). Double-clicking a Smart Collection selects it and opens the same dialog as Edit Rules…. Normal Collections open Rename on double-click (D41); busy commands/modals block opening another editor. Invalid names or failed writes preserve both inputs and roll back both changes.

macOS double-click timing follows the system preference (D39), with a 500 ms fallback when AppKit cannot supply it. Headless interaction checks include slower clicks and native-style batched input. Other platforms retain their current egui input settings.

Double-clicking a normal Collection opens Rename with its name focused and selected (D41); Smart Collections open Edit Rules. Both keep the current selection stable (D40/D41). A plain click on an already solely selected Collection waits for the double-click interval before deselecting; another click/action cancels that pending toggle. New selections and modifiers remain immediate. Opening either dialog is blocked during a pending command or another modal.

Asset selections support ranges, toggles and rectangle selection with wheel scrolling. Their Collections context submenu uses cached tri-state membership checkboxes (D29), and each sidebar collection shows a member count with space reserved beside its truncated name (D30).

Collections use all remaining sidebar height below the Folders tree, heading and New Collection button (D49). The standalone asset-total footer and separator are removed; long lists scroll and each Collection retains its own member count.

Folders navigation fills the available sidebar width (D42): All and Unassigned span the content area; device headers and folder names extend to its right edge while retaining disclosure arrows and tree indentation. Long names truncate with full-name hover text.

Long device/folder trees scroll independently below fixed All and Unassigned controls (D62), using at most 45% of the sidebar height left after those controls. Collections remains reachable below and scrolls separately; short trees use only their content height.

Selecting a folder includes assets in that folder and all descendants in grid/list (D43), even when the branch is collapsed. Search narrows the entire selected subtree. The shared `AssetQuery::folder_recursive` option follows catalogue parent links with cycle-safe SQL before paging; exact-folder queries remain available by default. No schema or CLI change.

Thumbnails use the asset's EXIF orientation, including mirrored values (D31). Import/update store oriented pixels; desktop display applies stored metadata to older, unapplied thumbnails without modifying the catalogue or requiring the original media.

List view uses thumbnail rows with filename/rating/dates/filesize, document/camera/lens/focal length, and dimensions/colour/profile columns (D32). It uses the browsing width with an optional Details inspector and horizontal scrolling in narrow windows. Import/update capture supported source colour/ICC descriptions; Update backfills older catalogue entries. Missing metadata stays explicit.

Desktop typography uses 15 pt interface/caption text, 14 pt List metadata and 13 pt small text, with stronger contrast in both themes (D33). Font hinting stays enabled and fractional glyph variants are disabled; eframe's native monitor scale is preserved. Headless layout checks cover 1×/1.5×/2× scales; visual baselines also cover light and Retina List views.

Details inspector and app dialogs use local, roomier text spacing (D47), leaving sidebar and grid/List browsing density unchanged. The inspector has inset content, larger section gaps, wrapping annotation controls and vertical scrolling for short windows. Collection/Smart rules/Connect/Sync dialogs have wider margins and field/button gaps; Smart rule fields retain a compact 4-point row gap so all fields and actions fit at 800×600. Rename/Edit Rules place Name beside its field, aligned with rule inputs in Edit Rules; creation keeps its stacked Name layout. Native OS pickers keep system spacing.

Edit Rules groups its instructions and ten rule inputs in a reusable, accessible fieldset with the legend Rules (D48). Name, live match preview and actions stay outside. The editor preserves Name/input alignment and fits at 800×600; creation and Rename keep their existing layouts.

App dialogs have a shared top-right X close button (D50), exposed as Close dialog for accessibility. It follows Cancel/Close behavior and discards unsaved inputs; it is disabled during pending collection saves or clone/sync, matching existing cancellation guards. Native OS pickers keep their own controls.

The Details inspector path uses the shared UI-only `middle_ellipsis::label` component (D51). It stays on one line, measures the available width and replaces the middle with … while retaining both ends. Short values stay complete; hover and accessibility retain the full path. Unicode paths are shortened at character boundaries.

Details and List share human-readable file sizes through `formatting::file_size` (D52): bytes below 1024, then one decimal place with KiB/MiB/GiB and larger binary units. Unknown or invalid sizes show —; a known empty file shows 0 B.

Details exposure times use shared `formatting::exposure_time` (D55): trim to at most two decimal places (0.1666…s → 0.16s), retaining two significant digits below 0.01s so short exposures never become 0s. Redundant trailing zeros are removed; invalid/nonpositive values show —. Stored metadata keeps its original precision.

Ratings use shared `rating::display`/`rating::editor` components (D56), always rendering five filled/empty stars. List remains display-only; Details and the Minimum/Maximum Rating fields in Smart Collection creation/Edit Rules use five accessible star buttons. Click 1–5 to set, click the current star again or press Delete/Backspace to clear, and use focused-star 0–5/arrows/Home/End for keyboard entry. Clearing Details saves rating 0; clearing either rating bound removes that rule. An explicit zero bound is preserved and can be entered with 0/Home. Maximum Rating is inclusive, may be used alone, and cannot be below Minimum Rating; zero matches unrated assets (D57). Schema v8 preserves existing rules, identities, revisions and replica baselines. Sync retains protocol v2 with a distinct extended Collection record for maximum rules; older servers reject it safely. Existing edit/collection commands and pending-write guards remain in charge of persistence.

Details and List share regional date/time formatting through `dates::DateFormatter` (D53/D54). Dates use numeric regional order/separators, padded day/month and four-digit years, followed by at and the system-format time (for example, 06/10/2026 at 22:30:05 in Britain). macOS uses a localized numeric date template and native time style; Windows normalizes its user short-date pattern while retaining native time formatting; Linux follows LC_ALL/LC_TIME/LANG and the local time zone. Modified timestamps display in local time; Captured keeps the camera's stored wall-clock time, without inventing a time zone. Invalid values show —. A bounded display cache refreshes after 30 seconds of use; stored values and CLI output are unchanged. Deterministic UI fixtures inject explicit regional settings without changing the machine's preferences.

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
- **Keep `Taskfile.yml` current — same change, same commit.** The root `Taskfile.yml` (go-task: `task --list`) is the canonical command surface, so whenever a change adds, renames or removes a workspace command, crate, or gate step, update the matching task in the same change (and `docs/development.md`). Workspace commands are `check`, `build`, `test`, `fmt`, `fmt:check`, `lint`, `layers`, `ci`, `doc` and `clean`. CLI entry points are `cli:run`, `cli:clone` and `cli:previews`; the desktop entry point is `ui:run`. Focused crate checks are `test:catalog`, `test:media`, `test:engine`, `test:sync` and `ui:test`. `task ui:test` runs headless UI checks (also included in workspace tests); `task ui:visual` compares reviewed screenshot baselines, `task ui:visual:update` explicitly regenerates them for review, and `task ui:smoke` runs native macOS checks with desktop permissions (D24). `task test:scale` is the opt-in 10,000-image workflow and large sync validation (requires a localhost listener). `task ci` is the full gate (`fmt:check`, then `lint`, then `test`, sequentially). Prefer `task <name>` over ad-hoc invocations so the documented surface stays the one people use; if a task's command changes, change the task — don't add a parallel path.
- **Use the existing command approvals.** Run allowlisted tasks such as `task ci`, `task ui:test`, `task test:sync` and `task test:scale` by their canonical names as plain commands from the repository root, with no shell redirection, environment assignment or wrapper script. Capture output through the command tool. Shell syntax and aliases can prevent a command from matching its narrow approval rule and cause another prompt. When a task needs localhost access, use its existing rule; request a new, narrowly scoped approval only for an action the rules do not cover. Do not use broad shell, Python or Git prefixes to avoid a prompt.
- `task release:build -- --target <triple>` builds the locked release CLI and desktop app; `task release:package -- <triple>` packages them under `target/release-assets`; `task release:test` runs archive/version and Arch metadata regression tests (also included in workspace tests). Linux packaging requires FPM 1.17.0, rpm, bsdtar and zstd; Windows packaging uses PowerShell. See `docs/development.md` for the release matrix and prerequisites.
- For NAS musl targets, use `task nas:build -- <triple>` on matching native Linux with musl-tools/binutils, then `task release:package -- <triple>` and `task nas:test -- <triple>`. These build/package only the CLI; desktop packaging dependencies are unnecessary. No local macOS or cross-architecture NAS build is supported by these tasks.
- For container deployment, use `task docker:build -- <musl-triple> [image]` after building the static CLI, then `task docker:test -- [image]`. Docker/Podman and `readelf` are required for packaging; tests require a container engine. `ICATALOG_TEST_COMPOSE=1` adds real Docker Compose lifecycle checks. Setup and maintenance: `docs/nas-docker.md`.
- Keep the tree building at every step. `cargo xtask layers` joins this list once it exists.

## 8. Parallel agents

Five reusable project agents EXIST in `.codex/agents/`: `legacy_format`, `catalogue_sync`, `native_ui`, `release_deployment` and `reviewer` (D59). Read `docs/agents.md` for their scope, invocation examples and handoff rules. The main conversation coordinates; delegate independent work when it materially helps, usually to one or two specialists. Keep small fixes in the main conversation. Give each worker explicit file ownership; workers do not delegate further. Obtain an independent read-only `reviewer` pass for migrations, imports, catalogue replacement and sync recovery changes. The coordinator owns integration, final `task ci`, shared docs and devlog unless explicitly assigned. Definitions are on-demand roles, not scheduled/background jobs; omitted model settings inherit the session's choices. Reviewer read-only behaviour is an instruction, not a separate sandbox.

- Use your own target dir (`CARGO_TARGET_DIR=target/agent-<name>`) to avoid the Cargo build lock, and edit only files you own. Shared files (root `Cargo.toml`, engine command registry) get small, surgical edits; re-read before editing.
- Put new commands in a new module rather than growing a shared file.
- If someone else's in-progress edit breaks the build, wait and retry; don't fix their files.
- Keep every `Cargo.toml` valid at all times. Create or rewrite manifests atomically: write to a temp file outside the crate directories, then `mv` it into place.
- Delete `target/agent-*` dirs of finished agents.
- **Stay inside the worktree.** Run commands from the repo root with repo-relative paths (use the tool's `workdir` for a crate directory). Do not `cd` to absolute paths, climb out with `..`, or touch files outside the repo (use the pre-approved temp dir for scratch work) unless it is really, really necessary; if it is, say why first.

## 9. Where things are tracked

- Real today: app-side, `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync` (revision-checked user-edit uploads and catalogue/preview pulls, D20/D44), the `icatalog` CLI and the `icatalog-ui` desktop shell; `ivc-file-research/docs/ivc-format.md` and `ivc-file-research/crates/ivc/tests/samples.rs` back the import path.
- EXISTS: `log/devlog.md` (local, gitignored; append a terse entry per task).
- Open question: whether the sync service is mandatory or also offers a simple share-file mode. (Samples corpus: resolved, see `ivc-file-research/samples/fetch.sh`.)
