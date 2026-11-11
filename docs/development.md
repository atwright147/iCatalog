# iCatalog development guide

Purpose: how to build, test, run and debug. Status vocabulary: **Decided** (owner decision or golden rule), **Proposed** (designed, not built), **Open** (unresolved). The `ivc`, `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli` and `icatalog-ui` crates build today; of the `icatalog` commands below `init/open/import/list/find/status/update/edit/collection/previews/clone/sync/serve` exist.

## 1. Prerequisites

- **Decided:** Rust toolchain, one Cargo workspace at the repo root.
- **Decided:** the repo pins Rust `1.99.0` via `rust-toolchain.toml` (components rustfmt, clippy, rust-src, rust-analyzer), so the terminal and rust-analyzer (VS Code) use the same compiler regardless of the machine's default `stable`. rustup installs it on first use.
- The `wasm32-unknown-unknown` target is **NOT installed** here (`rustup target list --installed` shows only `aarch64-apple-darwin`). To add it: `rustup target add wasm32-unknown-unknown`. No wasm build has been run.
- NAS CLI builds use native Linux hosts with musl-tools/binutils and the matching musl Rust target (section 8a). Later, **Proposed**: trunk + wasm-bindgen for the browser build.

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
- `task ci`: the full gate (`fmt:check`, `lint`, `test`, in that order). `task test`, `task lint`, `task fmt:check` and `task layers` run individual checks.
- `task test:catalog`, `task test:media`, `task test:engine`, `task test:sync` and `task ui:test`: focused crate tests. Sync and UI tests need localhost listener access.
- `task test:scale`: opt-in 10,000-image workflow and 10,000-asset HTTP sync round trip; prints timings and needs localhost listener permission. Uses generated media in temporary directories; run separately from the normal gate.
- `task cli:run -- <args>`: run the `icatalog` CLI, e.g. `task cli:run -- init my.sqlite` or `task cli:run -- collection my.sqlite list`.
- `task cli:clone -- local.sqlite --url http://nas:8080`: initialise a new local replica from the server.
- `task cli:previews -- <catalogue> status|generate|external|embed`: generate and manage preview storage.
- `task ui:run -- <catalogue>`: run the `icatalog-ui` desktop shell, e.g. `task ui:run -- photos.sqlite`. `task ui:visual` and `task ui:smoke` run optional visual and native checks (see below).
- `task release:build -- --target <triple>`, `task release:package -- <triple>`: build the locked release CLI + desktop app and package them under `target/release-assets` (section 8).
- `task release:test`: archive, release-tag and Arch package metadata regression tests, also included in the workspace gate.
- `task docker:build -- <musl-triple> [image]`: package a verified static CLI into a local image; requires Docker/Podman and `readelf`.
- `task docker:test -- [image]`: disposable container permissions, preview, clone/edit/sync and restart checks; `ICATALOG_TEST_COMPOSE=1` also exercises Docker Compose. See [NAS Docker setup](nas-docker.md).

## 2b. Codex development agents (D59)

Five project-scoped roles EXIST in `.codex/agents/`: `legacy_format`,
`catalogue_sync`, `native_ui`, `release_deployment` and `reviewer`. The main session
coordinates their bounded tasks; they inherit its model choices and permissions.
The reviewer is read-only by instruction. See [Development agents](agents.md) for
invocation examples, file ownership, isolated build directories and verification.
These are on-demand development tools; no application dependency or Task command is added.

## 3. Workspace layout

Root `Cargo.toml` today: `members = ["ivc-file-research/crates/ivc", "crates/*"]`; `icatalog-catalog` is the first app crate. App crates go in `crates/` (see `docs/architecture.md`).

Adding a crate (**Proposed** procedure):

1. Create `crates/<name>/` with a valid `Cargo.toml`; write it to a temp file outside the crate dirs and `mv` it into place.
2. The glob `crates/*` is already in the root members, so a new crate under `crates/` is picked up automatically.
3. Start with `unsafe_code = "forbid"` and `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
4. Register the crate in the layer test; respect the layering rule.

## 4. Driving the CLI

For the server/client setup, first sync, everyday desktop workflow and conflict recovery, see [Using iCatalog sync](sync-user-guide.md). The [protocol reference](sync.md) covers wire details and limits.

EXISTS: `cargo run -p icatalog-cli -- <command> <catalogue>` with `init`, `open`, `import`, `list`, `find`, `status`, `update`, `edit`, `collection`, `previews`, `clone`, `sync` and `serve`; also `--help`, `--version` and `commands` (list registered commands). Example: `cargo run -p icatalog-cli -- init photos.sqlite`; import a raster directory with `cargo run -p icatalog-cli -- import photos.sqlite --path <dir>` (add `--kind optical|removable|network` to declare the source kind; default `local`); set annotations with `cargo run -p icatalog-cli -- edit photos.sqlite --id 3 --rating 5 --keyword sunset`. Options use `--name value`; a following `--option` is treated as a missing value and rejected (D78).

Catalogue paths are opened as literal filenames (D65), including names that
begin `file:`. Empty and `:memory:` paths are rejected for disk commands.
`init --id` accepts 1–1024 printable ASCII bytes without leading or trailing
spaces; omit it for a generated ID. Only CLI `init` creates a missing catalogue; `clone` publishes a verified new
destination, and every other CLI command requires an existing file (D72).

Import skips a path already catalogued and content currently present at another
path. After Update changes an asset's bytes, its old UID remains stable but no
longer makes the old bytes count as a duplicate (D66).
Update hashes files even when size and stored modification time match, so a
same-size replacement with the original timestamp refreshes the asset and
thumbnail (D70). An unchanged file is read in bounded chunks without decoding.
Import and Update stop a full-file read if a source grows past its observed
size or 512 MiB cap; a size change is counted as a per-file failure (or a
size skip at the cap), leaving the previous asset untouched (D73).
Paths containing non-UTF-8 bytes are reported as skipped during import/update
until the catalogue path model can represent them byte-exactly.
Desktop Import rejects a selected non-UTF-8 directory before dispatch and
shows the error in the status area (D68).

CLI and desktop startup read OS arguments without assuming UTF-8. The CLI passes a catalogue filename through byte-for-byte on Unix: a filesystem that supports it can open the catalogue, while one that rejects it returns an ordinary error. The CLI never substitutes replacement characters into a path. Command names, option values and positional text must be UTF-8 because the current command parameters and stored imported-media paths are strings; malformed values return a usage error. Choose a UTF-8 source path before `import`/`update` if that option is rejected.
The CLI also rejects unknown long options for each command with a usage error
before opening a catalogue or applying any requested edit (D71).

Sync (D20/D44) is two processes. On the NAS run `task cli:run -- serve nas.sqlite --port 8080` (optional `--bind`, default `127.0.0.1`). `serve` requires an existing file and opens without SQLite's create flag (D60); initialize new catalogues explicitly. Use `task cli:clone -- local.sqlite --url http://nas:8080` with a new destination filename to download a verified replica and establish its baseline before editing. Parent directories must exist; existing files/sidecars are refused. Clone makes no uploads and fails if the server is unreachable. Alternatively, manually copy the canonical catalogue to a local path and run `task cli:run -- sync local.sqlite --url http://nas:8080` before editing. That first sync backs up unknown local changes before establishing the server baseline, including upgrades from D18/D19. Both peers must support protocol v2. Thereafter ratings, labels, keywords and Collection creation/name/rules/membership are uploaded by stable UID and revision; media, metadata, thumbnails and previews are never uploaded. Independent edits merge; conflicting entities are reported with a backup containing the local drafts. Idle sync uploads/downloads nothing. Replica Import/Update are disabled; make media changes on the server or a standalone catalogue. Whole-snapshot uploads and `.sync-state` fingerprints are retired. Plain HTTP only; TLS/auth and automatic desktop sync remain open; D46 supplies explicit desktop Connect/Clone and Sync dialogs. A stalled upload body does not block other sync clients; the server reads at most four bodies concurrently and returns retryable HTTP 503 for another concurrent upload (D88). Full recovery/bounds/wire details: `docs/sync.md`.

Preview generation is explicit: `task cli:previews -- photos.sqlite generate` makes oriented JPEG previews with long edge at most 1920 pixels, using original media and existing raster/RAW-preview decoding. Imported TIFFs are decoded as raster images; RAW formats use their embedded JPEG previews. Embedded storage is the default for a portable standalone file. For a NAS run `task cli:previews -- nas.sqlite external`, then Generate; `embed` converts referenced bytes back into SQLite and `status` reports mode/count. Conversion verifies all referenced blobs and commits atomically; external files are retained for recovery. Generate rebuilds corrupt cached bytes from unchanged original media (D86). `task cli:run -- sync local.sqlite --url http://nas:8080 --preview <sha>` fetches one referenced preview using GET only, verifies SHA-256, reuses valid cached bytes and refetches corrupt local bytes. Desktop full-size preview viewing is not wired yet.

The table below mirrors `AGENTS.md` section 4; rows marked EXISTS are built. The `info` alias for `status` is **Open**.

| Subcommand | Example | Purpose |
|---|---|---|
| `init` / `new` | `icatalog init photos.sqlite` | Create an empty catalogue |
| `open` | `icatalog open photos.sqlite` | Validate an existing catalogue and print a summary; a missing file is an error |
| `import` | `icatalog import photos.sqlite --path ~/Pictures --kind removable` | Import a directory (EXISTS, raster only; builds a source + folder tree, D21) or a `.ivc`/`.mpcatalog` (not built). `--recursive` accepts `true`/`1`/`yes` or `false`/`0`/`no` (default true); other values fail before import (D79). |
| `list` / `find` | `icatalog find photos.sqlite beach` | Browse and search; name/path substring matching handles Unicode case variants such as `CAFÉ`/`café` (D83) |
| `update` | `icatalog update photos.sqlite` | Rescan folders for added/changed/missing files (EXISTS, D14); an absent source root is reported `offline` and never marked missing (D21). A path is marked missing only after a `NotFound` probe; access errors are counted and preserve its prior state (D77). Unreadable files are counted/skipped; a catalogue write failure aborts and rolls back the rescan. A decodable replacement is required before its asset and thumbnail advance. Changed media refreshes or removes stored raw XMP without changing user edits (D80). |
| `edit` | `icatalog edit photos.sqlite --id 3 --rating 5 --keyword sunset` | Set one asset's keywords, rating and label (EXISTS, D16). Repeating an unchanged rating/label does not write or advance generation (D82). Use `--keyword-exact` or `--remove-keyword-exact` for one name containing commas. |
| `collection` | `icatalog collection photos.sqlite create --name "Favourites"` | Manual collections: list/create/show/add/remove (EXISTS, D25) |
| `previews` | `icatalog previews photos.sqlite generate` | Generate or convert embedded/external previews (EXISTS, D20/D44) |
| `clone` | `icatalog clone photos.sqlite --url http://nas:8080` | Create a new verified local replica; no prior init/copy needed (EXISTS, D45) |
| `sync` | `icatalog sync photos.sqlite --url http://nas:8080` | Push user edits and pull the catalogue; optional `--preview <sha>` fetch (EXISTS, D20/D44) |
| `serve` | `icatalog serve photos.sqlite --port 8080` | Run the NAS-side HTTP sync service (EXISTS, D18) |
| `status` / `info` | `icatalog status photos.sqlite` | Stats, generation, sync state |

Each subcommand is a thin wrapper over a command: the engine registry (including `collection`) or the sync registry in `icatalog-sync` (`clone`, `sync`, `serve`); the shell merges both (D18).

Manual Collections (D25) use `collection <catalogue> <action>`, preserving the catalogue-first CLI convention:

```sh
task cli:run -- collection photos.sqlite create --name "Favourites"
task cli:run -- collection photos.sqlite list
task cli:run -- collection photos.sqlite add --id 1 --asset 3
task cli:run -- collection photos.sqlite add --id 1 --assets 3,4,5
task cli:run -- collection photos.sqlite show --id 1
task cli:run -- find photos.sqlite sunset --collection 1 --rating 4
task cli:run -- list photos.sqlite --collection 1 --limit 20
task cli:run -- collection photos.sqlite remove --id 1 --asset 3
```

For `find`, `--rating 0` means a minimum rating of zero and includes all assets. `--label 0` finds unlabelled assets, including those with no annotation row. Higher rating thresholds and nonzero labels retain their usual filtering.

Use the ids returned by create/list and the asset ids from `list`. Names are trimmed, limited to 1024 UTF-8 bytes and cannot contain control characters; names may repeat, so commands identify collections by id. Add/remove are idempotent. Membership does not move or delete assets, folders or media. Smart Collections are implemented (D35); see below.

Rename either collection kind with `task cli:run -- collection my.sqlite rename --id 1 --name "New name"`. In the desktop sidebar, double-click a normal Collection (D41), or right-click either kind and choose **Rename…**. The current name is selected for replacement; **Rename** or Enter saves, while Cancel/Escape/backdrop dismisses without writing. Blank names disable Rename. Invalid names and failed writes preserve the entered name and show an inline error for retry. Controls and cancellation are disabled while saving. Renaming preserves collection ids, manual membership, smart rules, active selections and counts; the sidebar reorders by the new name (D36). Double-clicking selects the target without briefly deselecting it; a plain click on an already selected Collection waits for the double-click interval before returning to All. New selections and modifier gestures remain immediate, and pending commands/modals block opening another dialog.

Smart Collections evaluate stored rules against the current catalogue automatically:

```sh
task cli:run -- collection my.sqlite create-smart --name "Highly rated JPEGs" --format JPEG --min-rating 4
task cli:run -- collection my.sqlite preview-smart --format JPEG --min-rating 4
task cli:run -- collection my.sqlite rules --id 2
task cli:run -- collection my.sqlite set-rules --id 2 --lens "50 mm" --captured-from 2024-01-01 --captured-to 2024-12-31
task cli:run -- list my.sqlite --collection 2
```

Exact Smart keyword rules preserve surrounding spaces in CLI and desktop entry (D85). A keyword added with `--keyword-exact " travel "` matches a Smart rule entered as `--keyword " travel "`.

Use the returned id. `collection list` includes a `kind` column (`manual`/`smart`); `rules` displays the active rules. `set-rules` replaces the entire rule set, clearing omitted rules. Add `--name "New name"` to rename the Smart Collection in the same atomic save; omitting it preserves its name (D38). Supported options are `--format` (exact), `--captured-from`/`--captured-to` (inclusive local dates in `YYYY-MM-DD` form), `--camera` (make/model contains), `--lens` (contains), `--min-rating`/`--max-rating` (inclusive 0–5 bounds), `--label` (0–7), `--keyword` (exact, case-sensitive), and `--text` (filename/path contains). Text matching other than keywords ignores ASCII case; `%` and `_` are literal characters. At least one rule is required, and every supplied rule must match. Missing extracted metadata does not match a corresponding rule; absent annotations count as rating/label zero. Rule text is limited to 1024 UTF-8 bytes without control characters. There are no nested groups or OR rules in this version.

In the desktop's **New Collection…** modal, select **Smart Collection** and fill the desired rule fields. A **(Smart)** suffix identifies smart rows. Double-click a Smart Collection or right-click it and choose **Edit Rules…** to edit its name and rules together. **Save**, or Enter from Name, saves both atomically; Cancel/Escape dismisses without changes. Blank names disable Save, and validation or write failures preserve both inputs for retry. On macOS, double-click timing follows the system preference (D39), with a 500 ms fallback when unavailable. An already selected Smart Collection stays selected between the two clicks (D40). A single plain click deselects it after the double-click interval; new selections and modifier gestures stay immediate. Another pointer/keyboard/scroll action cancels a pending deselection. Double-click selects the targeted Smart Collection; normal Collections open Rename on double-click (D41), and dialogs cannot open during a pending command (D38). Rules use the normal background command worker with disabled pending controls and inline errors that preserve inputs for retry. Membership and counts update after import/update/annotation edits. Smart and manual collections can be selected together using the existing modifiers, and search further narrows the result. Smart rows accept no membership drops and are omitted from the asset membership-checkbox menu. Existing catalogues migrate to schema v8 automatically (Smart rules originated in v6); older iCatalog versions refuse v8 catalogues. Update both server and clients before using the new schema with sync.

Create/Edit Rules shows a live count across the whole catalogue and a plain-language summary joining all active rules with **AND** (D37). Previews update after a short typing pause without saving or changing the catalogue; they ignore the current sidebar/search filter. Invalid rules show a preview error rather than zero matches. Long summaries scroll within the preview area. The CLI exposes the same read-only preview through `collection preview-smart` with the rule options above; no collection name or id is needed.

**Minimum Rating** includes that rating and higher; **Maximum Rating** includes that rating and lower (D57). Either may stand alone, equal bounds select an exact rating, and a minimum above the maximum is rejected. A maximum of 0 matches unrated assets, including those without annotations. Clearing a bound removes its restriction. CLI examples: `task cli:run -- collection my.sqlite preview-smart --max-rating 2` or `task cli:run -- collection my.sqlite set-rules --id 1 --min-rating 2 --max-rating 4`. Both use the same stored rating displayed in the inspector. Ratings may be seeded from embedded XMP on import or assigned in iCatalog. For example, a photo rated 4 with no camera metadata matches a rating-only minimum of 4, but does not match a minimum of 5 or a combined camera rule such as Sony. Leave other fields blank for a rating-only collection; every filled field is an additional requirement.

Ratings (D56/D57) share five-star components across List, Details and the Minimum/Maximum Rating fields in Create/Edit Smart Collection. List stars remain read-only so row clicks/drags retain their existing behavior. In the editors, click a star to set 1–5; click the current star again or press Delete/Backspace while a star is focused to clear. Details clearing means unrated (0); clearing a rating bound removes that rule. Keys 0–5 set a value, arrows adjust within bounds, and Home/End select 0/5. An explicit bound of 0 remains distinct from no rule and survives save/reopen. Each star has a 24-point target and accessible name/value grouping; pending saves disable editing. Existing command/preview validation, persistence and cancellation paths are unchanged. Headless checks cover pointer/keyboard/clear/disabled behavior in both themes at 1×/1.5×/2×, all persisted asset ratings, optional/zero Smart rules, cancellation, live counts and existing write-failure guards. No Taskfile command/gate change.

## 4a. Driving the GUI

EXISTS (D15): the `icatalog-ui` desktop shell. `cargo run -p icatalog-ui -- <catalogue>` (or `task ui:run -- <catalogue>`). A missing catalogue file is created and initialised automatically. A native macOS **File** menu (in-app fallback on other platforms) provides Import and Update; its **Import…** item picks a folder and dispatches the `import` command, and **Update** dispatches `update` (whole-catalogue rescan). The left panel holds a source-rooted folder tree grouped under device headers such as "This Mac" or a volume/share name (D21), with collapsible device sections; clicking a device name lists every asset on that device, plus an "Unassigned" entry; the centre has a full-width toolbar with the grid/list switch on the left and the search box (name/path, Enter or Go to apply, × to clear) on the right, above the thumbnails; selecting an asset shows its stored grid thumbnail, basic metadata (including the device and source it was imported from), EXIF and an **Annotations** section (rating 0-5, label 0-7, keyword add/remove/clear) in the right panel. Each annotation click/save dispatches the `edit` command. The status bar shows the last command's outcome, including how many assets belong to offline sources, and the catalogue's file name at its right end (full path on hover). Native desktop only; the wasm build is PLANNED.

The **Folders** All and Unassigned buttons fill the sidebar content width (D42). Device headers and folder names fill the remaining width beside their disclosure arrows and indentation, including selection highlights and clickable space at the right edge. Long names truncate; hover to read the full name. Arrows retain their separate expand/collapse behavior. Long device/folder trees scroll in their own area below All and Unassigned (D62); the tree uses at most 45% of the remaining sidebar height so Collections stays visible and scrolls independently. Short trees take only the space they need.

Selecting a folder shows assets in that folder and all its descendants in both grid and list (D43). Collapsed branches are included, and a folder with no direct assets can still show its descendants' assets. Search applies within the whole selected subtree. Expanding/collapsing a branch does not change the asset view or selection; selecting a child narrows the view to that child's subtree. All, Unassigned, devices and Collections keep their existing scopes. Browsing uses stored catalogue relationships and works without accessing original media.

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

The repo is under git with a root `.gitignore` covering `target/`, `log/`, `*.sqlite` and `*.icatalog` (in addition to `ivc-file-research/samples/.gitignore`). `log/devlog.md` (local, gitignored) records a terse entry per task.

Use `.icatalog` for new catalogue filenames (D89). The desktop creates
`catalogue.icatalog` when launched without a path, but reopens an existing
`catalogue.sqlite` default when the new name is absent. An unbundled desktop
uses the working directory; the macOS app bundle uses the user's home
directory. Connect proposes
`Library.icatalog`. The CLI accepts explicit filenames without changing their
suffix; existing `.sqlite` catalogues remain valid. Every existing file is
checked for the initialized iCatalog schema and identity before any migration
can write to it. Linux desktop packages and the macOS bundle associate
`.icatalog` with the native app. Finder launches and running-app open events
open verified existing catalogues; a busy command or unsaved dialog/draft
leaves the current catalogue in place and reports a retryable message (D97).
Windows ZIPs include `associate.ps1` for optional per-user registration.

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

Both binaries are included. Windows ZIPs contain `icatalog.exe`, `icatalog-ui.exe`, `associate.ps1` and `README.txt`; the release build statically links the MSVC runtime. After extracting the ZIP to a permanent folder, run `powershell -NoProfile -File .\associate.ps1 Register` there, then choose iCatalog for `.icatalog` in Settings → Apps → Default apps or Open with. Run `powershell -NoProfile -File .\associate.ps1 Unregister` before moving/removing the folder. The helper registers only for the current user and does not change Windows' protected default-app choice. The macOS archive contains `iCatalog.app` and the standalone `icatalog` CLI; move the app to Applications, then select it in Finder's Open with if needed. The app is ad-hoc signed for Apple Silicon execution, without Developer ID signing or notarization. Windows publisher signing is not configured. There is no macOS Intel or universal artifact.

Linux builds on Ubuntu 22.04 (glibc 2.35 baseline). [FPM 1.17.0](https://fpm.readthedocs.io/en/v1.17.0/cli-reference.html) packages the same binaries in all three native formats, including runtime dependencies and an applications-menu launcher. Packages install `/usr/bin/icatalog`, `/usr/bin/icatalog-ui` and `/usr/share/applications/icatalog.desktop`. Arch constraints must contain no spaces (`glibc>=2.35`); a post-package `.PKGINFO` check rejects malformed dependencies and a missing glibc minimum. Debian/RPM prerelease versions use `~` in place of `-`; Arch uses `_`. Files retain the Cargo version in their names. Install with `sudo apt install ./<asset>.deb`, `sudo dnf install ./<asset>.rpm` or `sudo pacman -U ./<asset>.pkg.tar.zst`.

The build matrix uses native [GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners): `windows-2022`, `windows-11-arm`, `macos-14` (ARM64), and `ubuntu-22.04`. Each job explicitly installs Go 1.25.10 with [setup-go](https://github.com/actions/setup-go) before building Task 3.53.1, rather than relying on runner software. Go caching is disabled because this is a Rust repository with no `go.sum`. Each job runs CLI/packaging tests. Extracted Windows/macOS archives must create and reopen a synthetic catalogue, include their association metadata/helper, and the macOS executable must contain only ARM64. The Windows UI workflow runs the helper against the disposable runner's user registry and checks its scoped cleanup. Disposable Debian 12, Fedora 43 and current Arch containers install the packages with dependency resolution, check ELF dependencies for both binaries, and create/reopen a catalogue with the installed CLI. The release checks do not exercise a graphical desktop; `task ui:smoke` covers Finder delivery on a logged-in Mac.

Only after every desktop build, Linux install test and NAS musl job passes does the upload job get `contents: write`, generate `SHA256SUMS` and attach all eight packages plus checksums to the existing release (six desktop packages and two CLI-only NAS archives). Rerunning replaces same-named assets. Builds retain workflow artifacts for 14 days, including manual runs. Build jobs have read-only repository permissions and require no signing secrets.

Local packaging uses the same tasks as the workflow:

```sh
task release:build -- --target aarch64-apple-darwin
task release:package -- aarch64-apple-darwin
task release:test
```

Use the matching target on Windows/Linux. Windows needs PowerShell (`pwsh`); set `$env:RUSTFLAGS='-C target-feature=+crt-static'` before the Windows build to match CI. Desktop Linux needs the existing UI development libraries plus Ruby/FPM 1.17.0, `rpm`, `bsdtar` (`libarchive-tools` on Ubuntu) and `zstd`; the workflow records the exact install commands. Packaging reads `target/<triple>/release` and writes `target/release-assets`. `ICATALOG_RELEASE_BINARY_DIR` and `ICATALOG_RELEASE_OUTPUT_DIR` override those directories for artifact tests or inspection. Desktop packaging requires both executables; NAS packaging requires only the CLI. Both perform no downloads and clean their temporary staging directories.

Status: local workspace, visual and archive/version checks pass. After correcting missing Go and Arch's spaced glibc constraint, [hosted v0.1.0 recovery](https://github.com/atwright147/iCatalog/actions/runs/40751654218) passed all four desktop platform builds, all three Linux installation checks and final upload. The [existing release](https://github.com/atwright147/iCatalog/releases/tag/v0.1.0) contains the original six assets plus SHA256SUMS; manifest entries were checked against GitHub's recorded asset digests. D58 adds NAS builds for future workflow runs; no NAS asset publication is claimed. Container packaging is covered in section 8b; wasm remains planned.

## 8a. Static NAS CLI builds (D58)

The CLI-only NAS targets are `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`. Build on a native Linux host of the same architecture; macOS, Windows and cross-architecture invocations return an actionable error. On Ubuntu install `musl-tools` and `binutils`, then the matching Rust target. For x86-64:

```sh
sudo apt-get install musl-tools binutils
rustup target add x86_64-unknown-linux-musl
task nas:build -- x86_64-unknown-linux-musl
task release:package -- x86_64-unknown-linux-musl
task nas:test -- x86_64-unknown-linux-musl
```

On ARM64 Linux replace the triple with `aarch64-unknown-linux-musl`. `nas:build` uses the locked release CLI dependency graph, sets target-specific `musl-gcc` C compiler/linker settings for bundled SQLite, and explicitly enables the static C runtime ([Rust linkage reference](https://doc.rust-lang.org/stable/reference/linkage.html#static-and-dynamic-c-runtimes)). It also passes the GNU linker's `--no-dynamic-linker` option: older musl-gcc specs can otherwise retain an interpreter request in static PIE executables, even with no shared-library dependencies ([GNU ld options](https://sourceware.org/binutils/docs/ld/Options.html)). No GUI libraries, system SQLite, FPM or container runtime are needed. The ELF check rejects wrong architectures, non-executables, dynamic interpreters and shared-library dependencies. `nas:test` runs the CLI integration/packaging suite for that release target, then extracts the archive, repeats ELF verification and creates/reopens a synthetic catalogue with the packaged CLI.

Archives are `target/release-assets/icatalog-<version>-<triple>.tar.gz`, containing only an executable `icatalog`. Extract on the matching NAS architecture, place it on PATH and follow [the sync setup guide](sync-user-guide.md). NAS firmware must support running Linux executables for that CPU; no systemd unit or service installer is supplied.

`.github/workflows/nas.yml` runs both native targets on [GitHub-hosted Linux runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) (`ubuntu-24.04`, `ubuntu-24.04-arm`) for PRs, main pushes and manual dispatch, retaining checked archives as workflow artifacts. The release workflow calls the same workflow with its pinned application commit and, for recovery, the selected tooling revision; publication waits for both NAS jobs. Local verification passed native ARM64 builds/tests and an x86-64 cross-build with emulated CLI/archive tests. The latter skipped two native-host/compiler fixture tests already covered on ARM64; native x86-64 builds and hosted execution/publication remain unverified.

## 8b. NAS Docker/Compose packaging (D60)

`task docker:build -- <musl-triple> [image]` validates and packages an existing
static CLI into a non-root scratch image. `task docker:test -- [image]` runs the
ignored real-container regression with generated media and disposable data.
`ICATALOG_CONTAINER_ENGINE=podman` selects Podman; `ICATALOG_TEST_COMPOSE=1` adds
actual Docker Compose stop/down/up and persistence checks. The normal workspace
gate includes fast build-input regressions without requiring a container engine.

The separate `docker.yml` workflow builds/tests on native x86-64 and ARM64 Linux,
including the supplied Compose lifecycle, without publishing images. Hosted runs
remain unverified. Local runtime verification passed native ARM64 and emulated
x86-64 applications, plus official Compose v2 against Podman's Docker API, using
Linux-local fixture storage. Native Docker-backend execution remains unverified.
Read [NAS Docker setup](nas-docker.md) for prerequisites,
ownership, mounts, explicit init/import, desktop connection and upgrades.

## 9. Open questions

- Final CLI argument syntax.
- Per-crate `clippy.toml` (allow panics in tests); check whether `ivc` already has the deny attributes before assuming.
- Samples corpus: resolved (CC0, committed; `ivc-file-research/samples/fetch.sh` is an optional re-fetch/verifier; see `docs/decisions.md` D9).

## Scale validation

`task test:scale` generates 10,000 unique 8×8 PNGs across 100 folders, imports thumbnails, browses/searches, edits annotations, rescans unchanged and added/changed/missing files, reopens the catalogue and checks SQLite integrity. The separate sync case transfers 10,000 assets with 1 KiB synthetic thumbnail blobs and checks catalogue pulls, a small annotation upload and idle sync. These are functional scale checks, not representative RAW/photo decode benchmarks. The normal UI tests also render a headless grid over 10,000 rows and assert that fewer than 100 thumbnails are requested in a 900×600 viewport.

## List view

Desktop text (D33): interface and grid captions use 15 pt, List metadata 14 pt, small text 13 pt and headings 20 pt. Both themes use stronger text/selection contrast; metadata retains full text contrast. Hinting is on and fractional glyph variants are off. The bundled egui fonts and theme-specific atlas gamma are retained. eframe supplies the native monitor scale; the app does not override pixels per point. Headless tests cover both themes at 1×/1.5×/2× and check row/caption bounds and contrast. List rows reserve 136 logical points vertically and a minimum 840-point content width before horizontal scrolling.

List displays each asset as a thumbnail row with three columns: filename, rating, captured/modified dates and file size; document type, camera, lens and focal length; dimensions, colour model and ICC profile description (D32). Modified dates display in the system's local time zone; capture dates retain their stored local time. Both use regional date/time formatting (D53). Unavailable values show an em dash, and truncated values have tooltips. List uses the browsing width; **Details** opens the existing inspector for editing. Narrow windows scroll horizontally with a visible scrollbar. Selection, rectangle selection, collection drag/drop and context actions work across the row.

Import and Update capture colour/profile data for supported images. Run **File → Update** (or `task cli:run -- update my.sqlite`) to fill these fields in an older catalogue. An unchanged asset keeps its thumbnail bytes and identity. Lost profiles clear on changed files; RAW preview colour is not used as RAW source metadata. Browsing itself reads stored metadata and works with offline originals.

Details inspector and app dialogs (D47): `spacing.rs` applies local layout settings without changing the sidebar or grid/List browsing density. The inspector adds a 6-point inset, 8-point row gaps, extra section separation, wrapping annotation controls and vertical scrolling. App dialogs use 12-point inner margins, 8-point vertical/12-point horizontal item gaps and 8×4-point button padding; Smart rule grids use 16-point column/4-point row gaps (D57) to fit 800×600. Rename and Edit Rules show Name beside the input, aligned with the rule columns in Edit Rules; New Collection keeps its stacked Name layout. Native file/folder pickers keep OS spacing. Real-widget checks cover both themes at 1×/1.5×/2×, a short inspector with scrolled keyword editing, Smart rule columns/actions and long sync diagnostics. Screenshot baselines include metadata and a scrolled light inspector.

Edit Rules (D48) uses the reusable `fieldset::show` component for the instructions and ten rule fields, with a Rules legend and an accessible Group. Name, preview and action buttons remain outside. Editor content is 464 logical points wide so fieldset padding preserves input widths/alignment; all fields/actions fit 800×600. Creation and Rename layouts are unchanged. Headless checks cover group membership, alignment and bounds in both themes/scales; visual checks include a light editor.

Collections sidebar (D49): the scroll area uses all remaining sidebar height below the Folders tree, Collections heading and New Collection button. There is no standalone asset-total footer or 180-point list cap; per-Collection member counts remain. Headless checks cover rows near the bottom of short/tall windows and scrolling/selecting later Collections; screenshots include an expanded populated list.

App dialog close controls (D50): `dialog::header` adds a 24-point X at the top right of collection creation/rename/rule editing and Connect/Sync. Close dialog is its accessible name/tooltip. Clicking discards drafts via the existing Cancel/Close path; pending saves/transfers disable it. Headless checks cover placement, activation, draft/result/error dismissal, reopening and blocked workers. Native file/folder pickers keep system controls.

Inspector paths (D51): the shared `middle_ellipsis::label` component keeps paths on one line, retaining both ends with … in the middle when the measured text exceeds the available width. Short paths remain complete. Hover shows the full value and screen readers receive the full label. Tests cover narrow/empty layouts, Unix/Windows/Unicode strings, both themes at 1×/1.5×/2× and the real Grid/List inspector with its tooltip; screenshots cover long paths in both themes. No command or Taskfile change.

File sizes (D52): Details and List use the shared `formatting::file_size` helper, displaying B below 1024 and one decimal place with KiB/MiB/GiB and larger binary units (for example, 1572864 bytes becomes 1.5 MiB). Missing/invalid sizes display — and a known empty file displays 0 B. Unit tests cover boundaries through EiB and hostile values; inspector checks cover both themes at 1×/1.5×/2×. Stored byte counts, CLI output and Taskfile commands are unchanged.

Exposure display (D55): Details uses shared `formatting::exposure_time` beneath Lens. Fractional seconds are trimmed to two decimal places to match the owner's example (0.16666666666666666s → 0.16s). Below 0.01s, retain two significant digits (0.004s remains 0.004s); remove redundant trailing zeros. Nonfinite/nonpositive values display —. Unit checks cover long fractions, short/long exposures and extreme floats; real-widget checks verify the complete exposure line in both themes at 1×/1.5×/2× and unchanged stored precision. Stored metadata, CLI output and Taskfile commands are unchanged.

Regional dates (D53/D54): shared `dates::DateFormatter` formats Modified and Captured in Details/List as numeric dates with padded day/month, four-digit years and an at separator: 06/10/2026 at 22:30:05 in Britain, or 10/06/2026 at 10:30:05 PM with US/12-hour settings. macOS uses Foundation's current locale with a localized ddMMyyyy date template and medium time style, preserving regional/calendar/12/24-hour preferences. Windows normalizes the numeric fields of its user short-date pattern and retains the native long-time formatter. Linux reads LC_ALL, LC_TIME, then LANG (including encoding suffixes) and uses Chrono's matching locale data and the system local zone, normalizing date fields to padded numeric day/month and full year while preserving regional order/separators. Unknown Linux locale identifiers use POSIX formatting; unavailable native formatters fall back to ISO text with an explicit UTC suffix for Modified. Invalid/missing values show —. Captured is parsed as a camera-local wall-clock value and is never shifted to the viewer's time zone; Modified is an epoch instant and is shifted, including DST. Display caching is bounded to 512 entries and expires after 30 seconds of use. Stored dates, sorting/filtering and CLI output are unchanged.

Headless date checks cover British/American/German date order and clocks, time-zone/date rollover, daylight-saving capture gaps, malformed/extreme input, cache bounds/expiry and real Grid/List display. macOS tests compare against the current native formatter and exercise native regions and London DST. Screenshot fixtures explicitly inject British locale/UTC offset so host region/time-zone changes cannot rewrite baselines. Native Windows/Linux runtime checks still require those platforms; the wasm target remains uninstalled and no wasm status is claimed. Existing Taskfile commands/gates are unchanged.

## Automated UI checks

- `task ui:test`: headless interaction tests against the real eframe App using `egui_kittest` 0.36.2. Included in `task ci` through the workspace test suite. Exercises folder selection, search/Enter/clear, grid selection, rating/label/keyword edits and reopened database state, disabled controls during a blocked write, failed-edit recovery, collection modal submission/cancellation/input blocking/error retry, membership/filtering/reopen and failed-write recovery, drag/drop and context-menu targeting/cancellation/busy/error behaviour, collection Shift ranges and Ctrl/⌘ toggles with combined search in grid/list, and scrolling through 10,000 assets with bounded thumbnail caching.
- `task ui:visual`: GPU-backed screenshot comparisons for grid, selected details, list, an 800×600 narrow window and populated collections, the New Collection modal, a collection context menu, an active drop target and multiple selected collections. Baselines live in `crates/icatalog-ui/tests/snapshots/`. Theme (dark), fonts (egui bundled), scale (1), operating-system conventions and fixtures are fixed, with additional List baselines for light theme and 2× Retina rendering (D33). No native window is needed, but a graphics adapter or software Vulkan renderer is required. These tests are opt-in locally; the PR workflow runs them with Mesa on Linux.
- `task ui:visual:update`: deliberately regenerates baselines. Inspect every changed PNG before committing; do not use update mode to make CI green. Missing/different baselines fail normal comparisons and produce `.new.png`/`.diff.png` evidence. Generated comparison/backup images are gitignored. Initial baselines were inspected on macOS. Earlier Linux CI evidence for `catalogue-narrow` and `catalogue-collection-counts` showed thumbnail rounding of at most 2/255, so those snapshots use weighted squared YIQ tolerance 1.0 on Linux (D24). The [2026-11-11 hosted failure](https://github.com/atwright147/iCatalog/actions/runs/43104237092) exposed six more snapshots with only isolated thumbnail/glyph differences of at most 2/255 per channel. D96 gives three of them Linux tolerance 1.0 and the other three 3.0, as measured from the uploaded `.new.png` and `.diff.png` artifacts. Other snapshots/platforms retain 0.6, no failing pixels are allowed, and no baselines were replaced. [Hosted validation passed](https://github.com/atwright147/iCatalog/actions/runs/43106450737).
- `task ui:smoke`: opt-in native macOS test. Copies the actual binary into a temporary app bundle with a unique executable name and bundle identifier, so System Events cannot confuse it with an already running iCatalog. It uses generated BMPs and a temporary catalogue, checks that the application menu is named **iCatalog**, invokes File → Update, verifies the new asset in SQLite, opens/cancels Import's native folder picker with Escape, and exits using Command-Q. The catalogue is reopened to verify persistence. The Rust driver guards its PID target, waits for foreground/menu readiness, opens File before choosing items, and retries Escape until the picker dismisses (including its opening animation). It polls observable state with deadlines and terminates only its own child processes on failure. Diagnostics: `target/ui-smoke/native-stderr.log` and `target/ui-smoke/native-automation.log`; automation timeouts include the last completed stage.

Native prerequisites: a logged-in graphical macOS session; Accessibility permission for the runner/terminal app under System Settings → Privacy & Security → Accessibility, and permission to control System Events under Automation when prompted. Missing permission fails with the OS diagnostic rather than skipping. The test brings its temporary window to the front, so run it on an idle/dedicated desktop. It does not test choosing a directory through the picker, every platform's dialogs, or closing during an active import.

`.github/workflows/ui.yml` runs the full gate and screenshot comparisons on PRs and main pushes, uploading image differences on failure. A separate Windows job runs the sync hard-link regression (D81). The 2026-11-11 D96 hosted run passed both jobs: Windows hard-link, Linux `task ci` and Linux `task ui:visual`. `.github/workflows/ui-native.yml` is manually dispatched on a dedicated self-hosted Mac labelled `ui` with Rust/Task installed and desktop permissions pre-authorized; its hosted execution remains unverified. The self-hosted job is not triggered automatically for untrusted PR code.

All images are generated checkerboards; no personal catalogues/photos or third-party image assets are used. UI controls carry accessibility names so tests locate widgets by meaning, not hard-coded screen coordinates (the scroll test only positions the pointer within the centre viewport).

### Desktop sync controls (D46)

Run `task ui:run -- local.sqlite`, then use File → Connect to Server… to clone into a new local file, or File → Sync… → Sync now for an existing replica. Desktop operations use the L3 command registry and run off-thread; no CLI sync/restart is needed. The server address is remembered beside each catalogue in `.sync-url` preferences. First sync of a manual copy requires acknowledging backup-first replacement. The result area includes offline deferrals, backups and conflict keys; automatic/startup sync remains unbuilt. `task ui:test` now also needs localhost listener access for real desktop clone/edit/upload/pull/offline/conflict tests. `task ui:visual` includes the new dialogs; `task ui:smoke` checks the native File entries and cancelled Connect dialog. No new task or command id.

If clone fails with a catalogue-download HTTP 500, inspect the `serve` terminal and verify its configured catalogue path still exists. Moving/trashing a served folder does not close the process's SQLite handle: `/info` can continue to respond while snapshots fail. Stop `serve`, restore the source to a writable location and restart with that path. Sync regression tests cover a moved server folder, a descriptive error response, absent destination/staging files and a successful retry after restoration.
