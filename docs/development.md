# iCatalog development guide

Purpose: how to build, test, run and debug. Status vocabulary: **Decided** (owner decision or golden rule), **Proposed** (designed, not built), **Open** (unresolved). The `ivc`, `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli` and `icatalog-ui` crates build today; of the `icatalog` commands below `init/open/import/list/find/status/update/edit/collection/sync/serve` exist.

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
- `task scale`: opt-in 10,000-image workflow and 10,000-asset HTTP sync round trip; prints timings and needs localhost listener permission. Uses generated media in temporary directories; run separately from the normal gate.
- `task ci`: the full gate (`fmt:check`, `lint`, `test`, in that order); also what CI will run once set up.
- `task test`, `task lint`, `task fmt:check`: the individual gate steps.
- `task layers`: the crate layer-rule test.
- `task sync`: test the `icatalog-sync` crate.
- `task run -- <args>`: run the `icatalog` CLI, e.g. `task run -- init my.sqlite` or `task run -- collection my.sqlite list`.
- `task gui -- <catalogue>`: run the `icatalog-ui` desktop shell, e.g. `task gui -- photos.sqlite`.
- `task release:build -- --target <triple>`, `task release:package -- <triple>`: build the locked release CLI + desktop app and package them under `target/release-assets` (section 8).
- `task release:test`: archive, release-tag and Arch package metadata regression tests, also included in the workspace gate.

## 3. Workspace layout

Root `Cargo.toml` today: `members = ["ivc-file-research/crates/ivc", "crates/*"]`; `icatalog-catalog` is the first app crate. App crates go in `crates/` (see `docs/architecture.md`).

Adding a crate (**Proposed** procedure):

1. Create `crates/<name>/` with a valid `Cargo.toml`; write it to a temp file outside the crate dirs and `mv` it into place.
2. The glob `crates/*` is already in the root members, so a new crate under `crates/` is picked up automatically.
3. Start with `unsafe_code = "forbid"` and `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
4. Register the crate in the layer test; respect the layering rule.

## 4. Driving the CLI

EXISTS: `cargo run -p icatalog-cli -- <command> <catalogue>` with `init`, `open`, `import`, `list`, `find`, `status`, `update`, `edit`, `collection`, `sync` and `serve`; also `--help`, `--version` and `commands` (list registered commands). Example: `cargo run -p icatalog-cli -- init photos.sqlite`; import a raster directory with `cargo run -p icatalog-cli -- import photos.sqlite --path <dir>` (add `--kind optical|removable|network` to declare the source kind; default `local`); set annotations with `cargo run -p icatalog-cli -- edit photos.sqlite --id 3 --rating 5 --keyword sunset`.

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
| `collection` | `icatalog collection photos.sqlite create --name "Favourites"` | Manual collections: list/create/show/add/remove (EXISTS, D25) |
| `sync` | `icatalog sync photos.sqlite --url http://nas:8080` | Pull/push against the NAS (EXISTS, D18) |
| `serve` | `icatalog serve photos.sqlite --port 8080` | Run the NAS-side HTTP sync service (EXISTS, D18) |
| `status` / `info` | `icatalog status photos.sqlite` | Stats, generation, sync state |

Each subcommand is a thin wrapper over a command: the engine registry (including `collection`) or the sync registry in `icatalog-sync` (`sync`, `serve`); the shell merges both (D18).

Manual Collections (D25) use `collection <catalogue> <action>`, preserving the catalogue-first CLI convention:

```sh
task run -- collection photos.sqlite create --name "Favourites"
task run -- collection photos.sqlite list
task run -- collection photos.sqlite add --id 1 --asset 3
task run -- collection photos.sqlite add --id 1 --assets 3,4,5
task run -- collection photos.sqlite show --id 1
task run -- find photos.sqlite sunset --collection 1 --rating 4
task run -- list photos.sqlite --collection 1 --limit 20
task run -- collection photos.sqlite remove --id 1 --asset 3
```

Use the ids returned by create/list and the asset ids from `list`. Names are trimmed, limited to 1024 UTF-8 bytes and cannot contain control characters; names may repeat, so commands identify collections by id. Add/remove are idempotent. Membership does not move or delete assets, folders or media. Smart Collections are implemented (D35); see below.

Rename either collection kind with `task run -- collection my.sqlite rename --id 1 --name "New name"`. In the desktop sidebar, double-click a normal Collection (D41), or right-click either kind and choose **Rename…**. The current name is selected for replacement; **Rename** or Enter saves, while Cancel/Escape/backdrop dismisses without writing. Blank names disable Rename. Invalid names and failed writes preserve the entered name and show an inline error for retry. Controls and cancellation are disabled while saving. Renaming preserves collection ids, manual membership, smart rules, active selections and counts; the sidebar reorders by the new name (D36). Double-clicking selects the target without briefly deselecting it; a plain click on an already selected Collection waits for the double-click interval before returning to All. New selections and modifier gestures remain immediate, and pending commands/modals block opening another dialog.

Smart Collections evaluate stored rules against the current catalogue automatically:

```sh
task run -- collection my.sqlite create-smart --name "Highly rated JPEGs" --format JPEG --min-rating 4
task run -- collection my.sqlite preview-smart --format JPEG --min-rating 4
task run -- collection my.sqlite rules --id 2
task run -- collection my.sqlite set-rules --id 2 --lens "50 mm" --captured-from 2024-01-01 --captured-to 2024-12-31
task run -- list my.sqlite --collection 2
```

Use the returned id. `collection list` includes a `kind` column (`manual`/`smart`); `rules` displays the active rules. `set-rules` replaces the entire rule set, clearing omitted rules. Add `--name "New name"` to rename the Smart Collection in the same atomic save; omitting it preserves its name (D38). Supported options are `--format` (exact), `--captured-from`/`--captured-to` (inclusive local dates in `YYYY-MM-DD` form), `--camera` (make/model contains), `--lens` (contains), `--min-rating` (0–5), `--label` (0–7), `--keyword` (exact, case-sensitive), and `--text` (filename/path contains). Text matching other than keywords ignores ASCII case; `%` and `_` are literal characters. At least one rule is required, and every supplied rule must match. Missing extracted metadata does not match a corresponding rule; absent annotations count as rating/label zero. Rule text is limited to 1024 UTF-8 bytes without control characters. There are no nested groups or OR rules in this version.

In the desktop's **New Collection…** modal, select **Smart Collection** and fill the desired rule fields. A **(Smart)** suffix identifies smart rows. Double-click a Smart Collection or right-click it and choose **Edit Rules…** to edit its name and rules together. **Save**, or Enter from Name, saves both atomically; Cancel/Escape dismisses without changes. Blank names disable Save, and validation or write failures preserve both inputs for retry. On macOS, double-click timing follows the system preference (D39), with a 500 ms fallback when unavailable. An already selected Smart Collection stays selected between the two clicks (D40). A single plain click deselects it after the double-click interval; new selections and modifier gestures stay immediate. Another pointer/keyboard/scroll action cancels a pending deselection. Double-click selects the targeted Smart Collection; normal Collections open Rename on double-click (D41), and dialogs cannot open during a pending command (D38). Rules use the normal background command worker with disabled pending controls and inline errors that preserve inputs for retry. Membership and counts update after import/update/annotation edits. Smart and manual collections can be selected together using the existing modifiers, and search further narrows the result. Smart rows accept no membership drops and are omitted from the asset membership-checkbox menu. Existing catalogues migrate to schema v6 automatically; older iCatalog versions refuse v6 catalogues.

Create/Edit Rules shows a live count across the whole catalogue and a plain-language summary joining all active rules with **AND** (D37). Previews update after a short typing pause without saving or changing the catalogue; they ignore the current sidebar/search filter. Invalid rules show a preview error rather than zero matches. Long summaries scroll within the preview area. The CLI exposes the same read-only preview through `collection preview-smart` with the rule options above; no collection name or id is needed.

**Minimum Rating** includes that rating and higher, using the same stored rating displayed in the inspector, whether seeded from embedded XMP on import or assigned in iCatalog. For example, a photo rated 4 with no camera metadata matches a rating-only minimum of 4, but does not match a minimum of 5 or a combined camera rule such as Sony. Leave other fields blank for a rating-only collection; every filled field is an additional requirement.

## 4a. Driving the GUI

EXISTS (D15): the `icatalog-ui` desktop shell. `cargo run -p icatalog-ui -- <catalogue>` (or `task gui -- <catalogue>`). A missing catalogue file is created and initialised automatically. A native macOS **File** menu (in-app fallback on other platforms) provides Import and Update; its **Import…** item picks a folder and dispatches the `import` command, and **Update** dispatches `update` (whole-catalogue rescan). The left panel holds a source-rooted folder tree grouped under device headers such as "This Mac" or a volume/share name (D21), with collapsible device sections; clicking a device name lists every asset on that device, plus an "Unassigned" entry; the centre has a full-width toolbar with the grid/list switch on the left and the search box (name/path, Enter or Go to apply, × to clear) on the right, above the thumbnails; selecting an asset shows its stored grid thumbnail, basic metadata (including the device and source it was imported from), EXIF and an **Annotations** section (rating 0-5, label 0-7, keyword add/remove/clear) in the right panel. Each annotation click/save dispatches the `edit` command. The status bar shows the last command's outcome, including how many assets belong to offline sources, and the catalogue's file name at its right end (full path on hover). Native desktop only; the wasm build is PLANNED.

The **Folders** All and Unassigned buttons fill the sidebar content width (D42). Device headers and folder names fill the remaining width beside their disclosure arrows and indentation, including selection highlights and clickable space at the right edge. Long names truncate; hover to read the full name. Arrows retain their separate expand/collapse behavior.

The **Collections** section below the folder tree lists albums and opens a **New Collection…** modal (D26). Its name field receives focus; **Create** or Enter submits through the engine, and **Cancel**, Escape or a backdrop click dismisses it without saving. Empty names disable Create. While saving, the modal stays open with disabled controls; success closes it, and a failure preserves the name with an inline error for retry. Native Import/Update actions disable while the modal is open. Each collection is a single row filling the available sidebar width, including selection/drop highlights, with its catalogue-wide member count aligned at the right (D30); empty collections show 0. Long names truncate within the row; hover to read the full name. A plain click selects one collection; clicking it again clears the filter and returns to All, preserving the search text. Shift-click selects an inclusive range in displayed name order; Ctrl-click (or ⌘-click on macOS) toggles individual collections. Ctrl/⌘+Shift-click adds a range to the existing selection. With several collections selected, assets from any selected collection appear once and search narrows that combined view (D28). Membership is edited through drag-and-drop or the asset context menu below.

In the grid or list, Ctrl/⌘-click toggles assets, Shift-click selects a range, and dragging from empty space selects a rectangle. The mouse wheel scrolls while rectangle selection is active. Selected assets can be dragged together onto a collection in the sidebar (D27). The target highlights while hovered; dropping adds membership and preserves the assets' folders and other collections. Escape or dropping elsewhere cancels. Right-click an asset for a **Collections** submenu (D29) with a checkbox for each collection. The checkbox summarizes membership across the selected assets: checked for all, unchecked for none, indeterminate for some. Clicking unchecked adds all selected assets; clicking checked removes them; clicking indeterminate adds only the missing assets. Right-clicking an already selected asset preserves the selection; right-clicking an unselected asset targets that asset alone. Both gestures use the `collection` engine command on the same background worker as annotation edits. Drag/drop and menu writes disable while a command is running or the creation modal is open; errors appear in the status bar. Reopen the context menu to retry a failed membership read.

## 5. Debugging tricks

Thumbnail orientation (D31): import/update apply the asset's EXIF orientation, including mirror transforms, to stored JPEGs. Existing thumbnails with `orientation_applied = false` are oriented from `asset_meta.orientation` when displayed in the grid or details panel; originals can be offline and no re-import is required. Already oriented thumbnails are never transformed twice. Missing/invalid orientation leaves pixels unchanged. Asset dimensions remain the source dimensions. A changed file that loses EXIF clears stale extracted metadata during update, preserving user annotations.

- **Debug builds and breakpoints (Decided):** `cargo build`/`cargo run` use the dev profile (symbols, `debug_assertions`, no optimisation), so an ordinary debugger attaches with no extra flags. There is no in-process inspector like a browser's devtools; you drive a debugger instead.
- **VS Code (recommended):** install the CodeLLDB extension (`vadimcn.vscode-lldb`, recommended in `.vscode/extensions.json`). `.vscode/launch.json` (committed) provides: *Debug icatalog-ui (GUI)*, *Debug icatalog CLI (edit)*, *Debug engine tests*, *Debug edit integration tests* and *Attach to icatalog-ui*. Pick one, set a breakpoint (e.g. in `crates/icatalog-engine/src/commands/edit.rs`) and press F5. Edit the `args` in the config to point at your catalogue. `dbg!(x)` and `eprintln!` work as console-style output and appear in the Debug Console.
- **Terminal:** `rust-lldb target/debug/icatalog-ui -- catalogue.sqlite`, then `breakpoint set --file edit.rs --line <n>` (or by symbol), `run`, `bt`, `frame variable`. `rust-gdb` is the same for gdb users.
- **GUI workers (D23/D25):** import/update/edit/collection run on a background thread. A busy indicator is shown; further edits and window close wait for completion. Debugger breakpoints may suspend all threads. There is no per-file progress or cancellation yet. Grid/list rendering visits visible rows, and thumbnails are cached only for those rows and the selection. All matching asset rows are still queried together.
- **Panic backtraces:** run with `RUST_BACKTRACE=1`. Because `dispatch_guarded` turns an escaped panic into an error, a panicking command shows an error rather than a backtrace; to see the backtrace, call the engine directly (e.g. `cargo test`) instead of through the guard.
- **Logging:** nothing wires a logger yet, so there is no `RUST_LOG`; use `dbg!`/`eprintln!` or the debugger for now.
- **Legacy files:** the `ivc` crate has `examples/dump.rs` for inspecting `.ivc` files; check `ivc-file-research/AGENTS.md` for current usage. Format spec: `ivc-file-research/docs/ivc-format.md`; sample oracles: `ivc-file-research/crates/ivc/tests/samples.rs`.
- **Parallel agents:** use `CARGO_TARGET_DIR=target/agent-<name>` to avoid the Cargo build lock; delete finished `target/agent-*` dirs. Edit only files you own; shared files get small surgical edits.
- **Devlog:** `log/devlog.md` (local, gitignored) records a terse entry per task; read its "Still open" bullets when picking work.
- **Panics/errors:** engine commands commit atomically; errors, panics and failed commits roll back writes from that command (D23). Outer shell guards report errors without claiming that earlier successful commands were undone. Sync keeps its backup/verify/swap policy. Never use the guard as a licence to panic.

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

## 8. Release builds and packages (D34)

`.github/workflows/release.yml` runs when a release is **published**, including prereleases published from drafts. Saving a draft does not build. This follows GitHub's [release event guidance](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#release). The workflow builds the release's tagged commit, using `rust-toolchain.toml` and `Cargo.lock`. Manually dispatch **Release builds** on a selected ref with `release_tag` empty to validate the matrix without uploading assets to a release.

To recover an existing release after fixing the workflow or packaging, push the fix, select **Actions → Release builds → Run workflow** on `main`, and enter the existing tag (e.g. `v0.1.0`) in `release_tag`. The workflow checks that the tag matches the binary versions and that the GitHub release exists, then pins the application source to that tag's resolved commit. Build/install jobs overlay only `packaging/` and `Taskfile.yml` from the dispatched workflow revision, keeping package fixes independent of tagged application code. Successful checks upload to that existing release. There is no need to move the tag or delete the release. **Re-run failed jobs** on the original run uses its original workflow and cannot pick up the setup fix.

Before tagging, set the versions in both `crates/icatalog-cli/Cargo.toml` and `crates/icatalog-ui/Cargo.toml` to the release version and update `Cargo.lock`. Tags must be `vX.Y.Z` or `X.Y.Z`, optionally with a prerelease suffix (e.g. `v0.2.0-rc.1`); build metadata (`+...`) is not supported. Mismatches fail before compilation. Other crates can retain their own versions.

| Platform | Rust target | Release asset |
|---|---|---|
| Windows 64-bit | `x86_64-pc-windows-msvc` | `icatalog-<version>-windows-x64.zip` |
| Windows ARM64 | `aarch64-pc-windows-msvc` | `icatalog-<version>-windows-arm64.zip` |
| macOS ARM64 | `aarch64-apple-darwin` | `icatalog-<version>-macos-arm64.tar.gz` |
| Debian 12+ x86-64 | `x86_64-unknown-linux-gnu` | `icatalog-<version>-linux-amd64.deb` |
| Fedora x86-64 | `x86_64-unknown-linux-gnu` | `icatalog-<version>-linux-x86_64.rpm` |
| Arch x86-64 | `x86_64-unknown-linux-gnu` | `icatalog-<version>-linux-x86_64.pkg.tar.zst` |

Both binaries are included. Windows ZIPs contain `icatalog.exe` and `icatalog-ui.exe`; the release build statically links the MSVC runtime. The macOS archive contains `iCatalog.app` and the standalone `icatalog` CLI; the app is ad-hoc signed for Apple Silicon execution, without Developer ID signing or notarization. Windows publisher signing is not configured. There is no macOS Intel or universal artifact.

Linux builds on Ubuntu 22.04 (glibc 2.35 baseline). [FPM 1.17.0](https://fpm.readthedocs.io/en/v1.17.0/cli-reference.html) packages the same binaries in all three native formats, including runtime dependencies and an applications-menu launcher. Packages install `/usr/bin/icatalog`, `/usr/bin/icatalog-ui` and `/usr/share/applications/icatalog.desktop`. Arch constraints must contain no spaces (`glibc>=2.35`); a post-package `.PKGINFO` check rejects malformed dependencies and a missing glibc minimum. Debian/RPM prerelease versions use `~` in place of `-`; Arch uses `_`. Files retain the Cargo version in their names. Install with `sudo apt install ./<asset>.deb`, `sudo dnf install ./<asset>.rpm` or `sudo pacman -U ./<asset>.pkg.tar.zst`.

The build matrix uses native [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners): `windows-2022`, `windows-11-arm`, `macos-14` (ARM64), and `ubuntu-22.04`. Each job explicitly installs Go 1.25.10 with [setup-go](https://github.com/actions/setup-go) before building Task 3.53.1, rather than relying on runner software. Go caching is disabled because this is a Rust repository with no `go.sum`. Each job runs CLI/packaging tests. Extracted Windows/macOS archives must create and reopen a synthetic catalogue; the macOS executable must contain only ARM64. Disposable Debian 12, Fedora 43 and current Arch containers install the packages with dependency resolution, check ELF dependencies for both binaries, and create/reopen a catalogue with the installed CLI. These checks do not exercise a graphical desktop.

Only after every build and Linux install test passes does the upload job get `contents: write`, generate `SHA256SUMS` and attach all six packages plus checksums to the existing release. Rerunning replaces same-named assets. Builds retain workflow artifacts for 14 days, including manual runs. Build jobs have read-only repository permissions and require no signing secrets.

Local packaging uses the same tasks as the workflow:

```sh
task release:build -- --target aarch64-apple-darwin
task release:package -- aarch64-apple-darwin
task release:test
```

Use the matching target on Windows/Linux. Windows needs PowerShell (`pwsh`); set `$env:RUSTFLAGS='-C target-feature=+crt-static'` before the Windows build to match CI. Linux needs the existing UI development libraries plus Ruby/FPM 1.17.0, `rpm`, `bsdtar` (`libarchive-tools` on Ubuntu) and `zstd`; the workflow records the exact install commands. Packaging reads `target/<triple>/release` and writes `target/release-assets`. `ICATALOG_RELEASE_BINARY_DIR` and `ICATALOG_RELEASE_OUTPUT_DIR` override those directories for artifact tests or inspection. Packaging requires both executables, performs no downloads, and cleans its temporary staging directory.

Status: local workspace, visual and archive/version checks pass. After correcting missing Go and Arch's spaced glibc constraint, [hosted v0.1.0 recovery](https://github.com/atwright147/iCatalog/actions/runs/40751654218) passed all four platform builds, all three Linux installation checks and final upload. The [existing release](https://github.com/atwright147/iCatalog/releases/tag/v0.1.0) contains all six assets plus SHA256SUMS; manifest entries were checked against GitHub's recorded asset digests. NAS musl builds, Docker/compose and wasm remain planned.

## 9. Open questions

- Final CLI argument syntax.
- Per-crate `clippy.toml` (allow panics in tests); check whether `ivc` already has the deny attributes before assuming.
- Samples corpus: resolved (CC0, committed; `ivc-file-research/samples/fetch.sh` is an optional re-fetch/verifier; see `docs/decisions.md` D9).

## Scale validation

`task scale` generates 10,000 unique 8×8 PNGs across 100 folders, imports thumbnails, browses/searches, edits annotations, rescans unchanged and added/changed/missing files, reopens the catalogue and checks SQLite integrity. The separate sync case transfers 10,000 assets with 1 KiB synthetic thumbnail blobs and checks push, pull and idle sync. These are functional scale checks, not representative RAW/photo decode benchmarks. The normal UI tests also render a headless grid over 10,000 rows and assert that fewer than 100 thumbnails are requested in a 900×600 viewport.

## List view

Desktop text (D33): interface and grid captions use 15 pt, List metadata 14 pt, small text 13 pt and headings 20 pt. Both themes use stronger text/selection contrast; metadata retains full text contrast. Hinting is on and fractional glyph variants are off. The bundled egui fonts and theme-specific atlas gamma are retained. eframe supplies the native monitor scale; the app does not override pixels per point. Headless tests cover both themes at 1×/1.5×/2× and check row/caption bounds and contrast. List rows reserve 136 logical points vertically and a minimum 840-point content width before horizontal scrolling.

List displays each asset as a thumbnail row with three columns: filename, rating, captured/modified dates and file size; document type, camera, lens and focal length; dimensions, colour model and ICC profile description (D32). Modified dates are displayed in UTC; capture dates retain their stored local time. Unavailable values show an em dash, and truncated values have tooltips. List uses the browsing width; **Details** opens the existing inspector for editing. Narrow windows scroll horizontally with a visible scrollbar. Selection, rectangle selection, collection drag/drop and context actions work across the row.

Import and Update capture colour/profile data for supported images. Run **File → Update** (or `task run -- update my.sqlite`) to fill these fields in an older catalogue. An unchanged asset keeps its thumbnail bytes and identity. Lost profiles clear on changed files; RAW preview colour is not used as RAW source metadata. Browsing itself reads stored metadata and works with offline originals.

## Automated UI checks

- `task ui:test`: headless interaction tests against the real eframe App using `egui_kittest` 0.36.2. Included in `task ci` through the workspace test suite. Exercises folder selection, search/Enter/clear, grid selection, rating/label/keyword edits and reopened database state, disabled controls during a blocked write, failed-edit recovery, collection modal submission/cancellation/input blocking/error retry, membership/filtering/reopen and failed-write recovery, drag/drop and context-menu targeting/cancellation/busy/error behaviour, collection Shift ranges and Ctrl/⌘ toggles with combined search in grid/list, and scrolling through 10,000 assets with bounded thumbnail caching.
- `task ui:visual`: GPU-backed screenshot comparisons for grid, selected details, list, an 800×600 narrow window and populated collections, the New Collection modal, a collection context menu, an active drop target and multiple selected collections. Baselines live in `crates/icatalog-ui/tests/snapshots/`. Theme (dark), fonts (egui bundled), scale (1), operating-system conventions and fixtures are fixed, with additional List baselines for light theme and 2× Retina rendering (D33). No native window is needed, but a graphics adapter or software Vulkan renderer is required. These tests are opt-in locally; the PR workflow runs them with Mesa on Linux.
- `task ui:visual:update`: deliberately regenerates baselines. Inspect every changed PNG before committing; do not use update mode to make CI green. Missing/different baselines fail normal comparisons and produce `.new.png`/`.diff.png` evidence. Generated comparison/backup images are gitignored. Initial baselines were inspected on macOS. Linux CI evidence for `catalogue-narrow` and `catalogue-collection-counts` showed two thumbnail pixels with a red-channel difference of 2/255; all other changed channels differed by at most 1/255. Only these two snapshots on Linux use a per-pixel weighted squared YIQ colour tolerance of 1.0; every other snapshot/platform retains 0.6. No failing pixels are allowed and no baselines were replaced. Local comparisons and [hosted Linux checks](https://github.com/atwright147/iCatalog/actions/runs/40751633405) passed.
- `task ui:smoke`: opt-in native macOS test. Copies the actual binary into a temporary app bundle with a unique executable name and bundle identifier, so System Events cannot confuse it with an already running iCatalog. It uses generated BMPs and a temporary catalogue, checks that the application menu is named **iCatalog**, invokes File → Update, verifies the new asset in SQLite, opens/cancels Import's native folder picker with Escape, and exits using Command-Q. The catalogue is reopened to verify persistence. The Rust driver guards its PID target, waits for foreground/menu readiness, opens File before choosing items, and retries Escape until the picker dismisses (including its opening animation). It polls observable state with deadlines and terminates only its own child processes on failure. Diagnostics: `target/ui-smoke/native-stderr.log` and `target/ui-smoke/native-automation.log`; automation timeouts include the last completed stage.

Native prerequisites: a logged-in graphical macOS session; Accessibility permission for the runner/terminal app under System Settings → Privacy & Security → Accessibility, and permission to control System Events under Automation when prompted. Missing permission fails with the OS diagnostic rather than skipping. The test brings its temporary window to the front, so run it on an idle/dedicated desktop. It does not test choosing a directory through the picker, every platform's dialogs, or closing during an active import.

`.github/workflows/ui.yml` runs the full gate and screenshot comparisons on PRs and main pushes, uploading image differences on failure. Hosted workspace checks passed; the initial two Linux colour-rounding snapshot failures were inspected and addressed as above. `.github/workflows/ui-native.yml` is manually dispatched on a dedicated self-hosted Mac labelled `ui` with Rust/Task installed and desktop permissions pre-authorized; its hosted execution remains unverified. The self-hosted job is not triggered automatically for untrusted PR code.

All images are generated checkerboards; no personal catalogues/photos or third-party image assets are used. UI controls carry accessibility names so tests locate widgets by meaning, not hard-coded screen coordinates (the scroll test only positions the pointer within the centre viewport).
