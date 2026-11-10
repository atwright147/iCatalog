# iCatalog architecture

Purpose: crate map, layering, and the engine/UI seam. Status vocabulary: **Decided** (owner decision D1-D18 in `docs/decisions.md` or a golden rule in `AGENTS.md`), **Proposed** (designed here, not accepted or built), **Open** (unresolved). Built today: `ivc`, `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli` and `icatalog-ui` (under `crates/`); everything else app-side is planned.

## 1. Purpose and stack

**Decided.** iCatalog is a Rust-only rebuild of the iView Media Pro media catalogue/organiser: CLI first, egui desktop, wasm browser build as a goal. No JavaScript/TypeScript, no Tauri, Electron or webviews, ever. The own catalogue format is **SQLite**; legacy `.ivc`/`.mpcatalog` is **import-only** through `ivc`. Development is **clean-room**. Machine names are lowercase (`icatalog`, `icatalog-*`, `ivc`); the repo root is `iCatalog/` (D4).

Development tooling (D59): five on-demand Codex role definitions EXIST in
`.codex/agents/`, with scope and coordination in [Development agents](agents.md).
They do not participate in the application runtime, crate graph or sync protocol.

## 2. Crate map

Status column: EXISTS means present in the repo today; PLANNED means not created.

The shared user-facing name is `icatalog_catalog::APP_NAME`. The catalogue crate is the lowest existing app layer shared by the UI, engine and sync code that display it, so window/menu titles and runtime messages use the same constant without another dependency. Executable, crate and file names remain lowercase; documentation and Cargo descriptions use the product spelling directly.

| Crate | Layer | Depends on | Role | Status |
|---|---|---|---|---|
| `ivc` (`ivc-file-research/crates/ivc`) | L0 | nothing in the workspace | Legacy `.ivc` parser | EXISTS |
| `icatalog-catalog` | L1 | `ivc` (for import mapping, Proposed), `rusqlite` (`bundled`, `functions`) | SQLite schema (migrations v1-v8) + model + CRUD + `AssetQuery`/`CatalogStats` (`query_assets`, `stats`) | EXISTS |
| `icatalog-media` | L1 | `image` 0.25, `kamadak-exif`, `sha2` (no workspace dependency) | Raster probe, EXIF read, grid JPEG thumbnails, content hash, RGBA decode (D8, D13); optional EXIF orientation for thumbnails/display (D31); `raw` (embedded JPEG preview of DNG/CR2/NEF/ARW/PEF/RAF/CR3) and `xmp` (packet locate + tolerant parse) modules (D17) | EXISTS |
| `icatalog-engine` | L2 | L0-L1 | Session `Context` + command registry; commands `init`, `open`, `import`, `list`, `find`, `status`, `update`, `edit`, `collection`, `previews`; `dispatch_guarded` panic guard (D12, D14, D16) | EXISTS |
| `icatalog-sync` | L3 | L0-L2, `tiny_http`, `ureq` | Protocol v2 (D20/D44): NAS-side `serve`, verified replica `clone` (D45), revision-checked annotation/Collection uploads, catalogue/preview pulls, embedded replica baseline, per-entity conflict backups, verified atomic replacement; exposes its own command registry | EXISTS |
| `icatalog-cli` | L4 | `icatalog-engine`, `icatalog-sync` | Binary `icatalog`: thin wrapper dispatching engine + sync commands by id (`init/open/import/list/find/status/update/edit/collection/previews/clone/sync/serve`, `--help`, `--version`, `commands`); only `init` creates a missing catalogue (D72) | EXISTS |
| `icatalog-ui` | L4 | `icatalog-catalog`, `icatalog-engine`, `icatalog-media`, `icatalog-sync`, `eframe`/`egui`, `rfd`, `muda` (native macOS menu), `chrono` (date validation/locales), Foundation/Windows native regional date APIs | Binary `icatalog-ui`: thin native egui desktop shell (browse, thumbnail, metadata, `init`/`import`/`update`/`edit`/`collection`); desktop Connect/Clone and explicit Sync dialogs (D46); wasm PLANNED | EXISTS (native) |
| `testkit` | test-only | as needed | Test helpers | PLANNED |
| `xtask/` (repo root) | tooling | none | `cargo xtask layers`, `ci` | PLANNED |
| `packaging/` | tooling | OS archivers, FPM for desktop Linux, musl-tools/binutils for NAS, Docker/Podman for containers | Desktop archives/packages and Linux launcher (D34); native static x86-64/ARM64 musl CLI builds/archives and ELF checks (D58); static CLI scratch image and Compose example (D60) | EXISTS |

**Decided** layout: one Cargo workspace rooted at the repo top. The workspace members include `ivc` and the `crates/*` glob (check root `Cargo.toml`). The exact dependency edges above (e.g. whether `icatalog-catalog` depends on `ivc` or the engine does the mapping) are **Open**; the layer ordering is Decided.

Release automation (D34/D58) packages the existing L4 binaries without adding an app crate or changing the engine seam. `.github/workflows/release.yml` builds Windows x64/ARM64, macOS ARM64 and desktop Linux x86-64, and calls `nas.yml` for CLI-only static x86-64/ARM64 musl archives. Publication waits for desktop installation and native NAS checks before attaching eight assets and SHA-256 checksums. Manual dispatch keeps artifacts only unless an existing `release_tag` is supplied. Recovery compiles that tag's app code while loading `packaging/` and `Taskfile.yml` from the dispatched workflow revision. Every runner explicitly installs Go before compiling Task. Hosted v0.1.0 recovery verified the original six desktop assets; new NAS hosted runs/publication remain unverified. NAS builds explicitly select musl-gcc and static linkage for the CLI graph and bundled SQLite, with ELF architecture/interpreter/dependency checks. Wasm remains planned.

Container packaging (D60) reuses those static binaries and validates their ELF before copying only the CLI and empty mount directories into an allowlisted temporary build context. The scratch image runs the CLI as a numeric non-root user. Compose injects init for signal forwarding, persists the whole catalogue directory (including external previews), mounts media and the root filesystem read-only, and defaults host publication to loopback. `serve` selects `Context::open_existing` and `Catalog::open_existing`, retaining migration behavior while disabling SQLite file creation; a wrong mount cannot initialize a catalogue. No new app dependency, schema, protocol or command id. A separate native x86-64/ARM64 workflow exercises container/Compose lifecycle and does not publish images. See [NAS Docker setup](nas-docker.md).

Smart Collection previews (D37) dispatch the read-only `collection preview-smart` action from a separate desktop worker after a 200 ms typing pause. The engine validates the draft and produces its count and readable AND summary; the catalogue counts against the shared saved-membership SQL predicate. The shell holds draft/worker state and discards stale responses. No temporary collection, new layer, dependency or schema is needed.

Preview generation and storage conversion prune blobs no longer referenced by
assets within the command transaction (D67); shared preview hashes remain.

Smart Collection editing (D38) submits the editable Name and full rules through one `collection set-rules --id … --name …` command/transaction. Optional name validation reuses rename/create; omission preserves the existing name. The shell handles double-click selection/opening and the shared modal/worker, including busy guards and retained inputs on errors. No new layer, command id, dependency or schema.

Desktop shell input initialization (D39) reads the macOS double-click interval through the existing safe AppKit binding and sets egui's input option; unavailable intervals fall back to 500 ms. Other platforms keep their current settings. No engine/model behavior or new dependency is involved.

Collection double-click routing and deselection arbitration (D40/D41) live only in shell gesture state: a selected row delays its plain-click toggle for the configured double-click interval. A double-click or intervening action cancels it; committed timers request repaint. Normal Collections open the shared Rename modal and Smart Collections open Edit Rules. Both use existing engine commands/workers; model/command behavior is unchanged.

## 3. Layers

```text
L4  icatalog-cli        icatalog-ui (egui; desktop EXISTS, wasm PLANNED)
        |                   |
L3  icatalog-sync  (client + `serve`)
        |
L2  icatalog-engine (session + command registry)
        |
L1  icatalog-catalog (SQLite schema + model)    icatalog-media (decode, thumbnails, metadata)
        |
L0  ivc (legacy parser, no workspace deps)
```

**Decided rules:**

- A crate may depend only on lower layers. `ivc` depends on nothing in the workspace.
- Nothing below `icatalog-ui` may use egui, eframe, winit, rfd or muda.

**Enforcement.** EXISTS: a layer test at `crates/icatalog-cli/tests/layers.rs` parses each `Cargo.toml` and fails on an upward workspace dependency or on egui/eframe/winit/rfd below `icatalog-ui`. **Proposed:** later replaced or joined by `cargo xtask layers` (PLANNED). New crates must be registered in the test.

## 4. Engine/command seam

**Decided** (golden rule: everything is a command). A command has: an **id**, a **label**, a params doc, an `enabled` check and a `run` function, plus tests. The CLI and the GUI both dispatch commands by id. CLI subcommands are thin wrappers over commands. Sync is a command too, so CLI and GUI behave identically.

**EXISTS** (D12): the engine holds a session `Context` and a static registry (slice of `&dyn Command`) mapping string id to command; `run` takes string-keyed `Params` validated into typed values (bad input is `EngineError::BadParam`) and returns `Outcome::{Text,Table}` or an error, never panics. `dispatch` runs each engine command in a SQLite transaction and rolls back on an error, panic or failed commit (D23). During `update`, catalogue errors propagate to that transaction; media that passes header probing but fails full decode is skipped before asset fields change. The rescan checks thumbnail derivation and streams a bounded content hash before treating matching size and mtime as unchanged (D70). Import and Update also bound the later full-file read to the observed size plus one byte; a source that changes size during scanning is not ingested and does not change its existing asset (D73). Image bytes are decoded when content changes or a thumbnail needs repair. `dispatch_guarded` also catches panics outside command execution. Sync retains its separate file-replacement protocol.

Import uses exact path and current content-hash checks for duplicates (D66).
Historical UIDs remain stable after Update and do not suppress reimport of
their old bytes.

**EXISTS** (D18): the sync commands `clone`, `sync` and `serve` live in `icatalog-sync` (L3), not the engine (L2 must not depend on L3). `icatalog-sync` exposes the same shape (`commands()`, `find_command`, `dispatch_guarded`); the L4 shells merge the two registries and dispatch engine-first. The sync registry is not part of `icatalog-engine`, so the engine's fuzz test covers only its own commands; `crates/icatalog-sync/tests/sync.rs` covers hostile sync params.

The L4 CLI and desktop entry points use `args_os` (D61). CLI parsing preserves catalogue path bytes, validates text arguments and rejects long options absent from that command's registry `ParamSpec` before opening the catalogue (D71). Filesystems that refuse a byte-exact catalogue path produce an ordinary open error; no lossy substitute is used. The engine's `Params` and catalogue asset paths remain UTF-8 strings; accepting arbitrary filesystem bytes for imported media would require a separate model design.

CLI `init` uses the create-capable session; `clone` prepares a destination
without opening it. Every other CLI command opens an existing file without
SQLite's create flag, so a mistyped path cannot leave an empty catalogue
(D72). The desktop's separate startup behavior remains as documented below.

The desktop validates a selected import directory before placing it in the
string-keyed command parameter, reporting a normal error for a non-UTF-8 path
(D68).

Catalogue opening treats `file:` as a literal relative filename and rejects
special in-memory disk paths (D65); explicit test sessions use
`Catalog::open_in_memory` or `Context::open_in_memory`.

Replica cloning (D45) uses `Context::for_destination`, an in-memory session carrying the intended output path. The CLI selects this constructor for `clone`, then uses normal registry dispatch, so merely invoking the command cannot create/open/migrate the destination file. The L3 clone command and ordinary sync share staged download validation and baseline preparation. Clone publishes the verified file using a same-filesystem hard link, which atomically refuses any existing destination, then reopens the session on the completed replica. Existing files, symlinks, SQLite sidecars, and the bare `:memory:` path are rejected before transfer (D69). No schema, dependency or HTTP endpoint change; Desktop clone/connect controls are implemented in D46.

Ordinary sync and preview fetches require a regular catalogue pathname before
any network work. Catalogue sync rechecks immediately before replacement
(D74/D81). Symlinks, and multi-link files on Unix and Windows, are refused
because replacing one pathname would split the replica from its other name.
Windows link counts come from a safe handle-information wrapper in a
Windows-only engine dependency.

Desktop sync workers and recovery reopening use an existing, initialised
catalogue (D75). Only the explicit desktop startup/Init path creates one;
a missing or empty file during sync keeps the recovery error visible.

May never leak into the engine: egui/eframe/winit/rfd types, window or widget state, terminal formatting. Only pure view/window state belongs to the shell.

Commands return `Err` for any input (hostile params included). A fuzz-style test over every command **EXISTS** at `crates/icatalog-engine/tests/adversarial.rs` and must stay green.

## 5. UI shell

- **EXISTS (D15):** `icatalog-ui` is a thin, data-driven native egui desktop shell. Browsing reads the local catalogue (`query_assets`, `thumbnail`, `annotation`, `asset_keywords`); every action (`init`, `import`, `update`, per-asset `edit` of rating/label/keywords, and manual/Smart `collection` actions) is dispatched as an engine command, so CLI and GUI behave identically. Desktop single-keyword Add/Remove uses the exact-name edit options (D64), preserving names containing commas. The shell runs import/update/edit/collection on a single background worker using the same engine dispatcher, reports failures, and draws only visible grid/list rows with visible/selected thumbnail caching (D23). Manual Collections (D25) use the existing v1 tables and `AssetQuery::collection_id`/`collection_ids`; the sidebar creates collections through an egui modal (D26) and selects collections and edits the selected asset's membership through the engine. Grid/list drag-and-drop and right-click collection actions use the same command/worker (D27). Modifier selection (D28) stays in shell state: Shift selects a display-order range, Ctrl/⌘ toggles collections, and the catalogue query returns their combined assets without duplicates. Smart Collections (D35) store typed AND rules in v6; catalogue SQL evaluates membership, counts and mixed unions. The creation modal offers a Smart Collection option and fields; right-clicking a smart row opens Edit Rules… through the same worker. Smart rows cannot receive membership drops or checkboxes. Asset queries still fetch all matching rows. egui/eframe native desktop; no webview. Display decoding uses `icatalog-media::decode_rgba_with_orientation` (D8, D31).
- **Decided (goal):** wasm browser build via trunk + wasm-bindgen, Rust only, using the same engine and model as desktop (D3).
- **Open:** the exact data-driven mechanism (e.g. menus generated from the command registry) is not designed.

UI verification (D24): `egui_kittest` is a UI-only dev dependency. Headless tests instantiate the same eframe App without installing OS menus; screenshot tests use its real renderer at fixed viewport/theme/scale. The native smoke integration test launches the production binary and operates macOS menus/dialog cancellation through System Events. Test fixtures and snapshot baselines live under `crates/icatalog-ui`; no test logic enters the engine or catalogue layers.

Folders rows (D42) use selectable buttons filling the remaining sidebar width, with left-aligned names, truncation and full-name hover text. All/Unassigned fill the content area; device and folder rows retain separate disclosure arrows and indentation. The device/folder tree scrolls independently within 45% of the sidebar height remaining after the fixed Folders/All/Unassigned controls (D62), leaving Collections reachable. Selection and collapse behavior remain shell state.

Folder browsing (D43) sets `AssetQuery::folder_recursive` for desktop selections. The catalogue query follows `folder.parent_id` with a recursive CTE using `UNION` to deduplicate visited ids and terminate cycles. It filters the selected folder and descendants before ordering/paging, composing with search and other filters. Expansion state does not affect membership. Exact-folder queries remain the API default; the shell holds only selection/expansion state. No schema, command or dependency change.

Asset name/path substring filters and Smart Collection text/camera/lens rules
use a Rust Unicode-lowercase SQLite function registered on each catalogue
connection (D83). Their predicates remain in SQL before ordering and paging.
The engine `edit` command skips annotation writes when a requested rating or
label already has that value (D82), preserving timestamps and generation.

The Collections scroll area fills the remaining sidebar height (D49), replacing its former 180-point cap. The standalone asset-total footer/separator is removed. The New Collection button remains above the scrolling rows, which retain per-Collection counts and selection behavior. This is shell layout only.

Collection renaming (D36) uses `collection rename --id … --name …` for both manual and Smart Collections. Every sidebar row offers Rename…; the shared collection modal and worker submit the same engine action as the CLI. Catalogue CRUD updates only `collection.name`; collection ids, membership and rules remain stable. No migration or dependency is needed.

Visual comparisons allow no failed pixels. Two Linux 800×600 screenshots with a clipped thumbnail use a per-pixel YIQ colour tolerance of 1.0, based on inspected Mesa/Metal differences of at most two red-channel levels in two pixels. Other snapshots/platforms keep the library's 0.6 tolerance; reviewed baselines remain unchanged.

List view (D32) renders a thumbnail and three metadata columns per asset: filename/rating/captured and modified dates/filesize; document type/camera/lens/focal length; dimensions/colour model/ICC description. It uses the browsing width by default; its Details toggle opens the existing inspector. Narrow windows and an open inspector use horizontal scrolling. Metadata reads are cached only for visible/selected rows and invalidated after refresh; errors are surfaced without repeating failed reads every frame. List display works from stored catalogue data, including offline media.

Desktop typography (D33) is configured once for both egui themes in the UI-only `typography` module. Interface/caption text is 15 pt, List metadata 14 pt, small text 13 pt and headings 20 pt. Text and selection colours have tested contrast; metadata is no longer dimmed. Font hinting is enabled and fractional glyph variants are disabled, while theme-specific atlas gamma and eframe's native monitor scaling are preserved. No font assets, dependencies or engine commands are added.

Inspector/dialog spacing (D47) is scoped to those UI children through the UI-only `spacing` module. The inspector uses an inset scrolling region and wrapping annotation rows; collection and sync modals set their own frame margins and field/button spacing. Smart rule grids use explicit column/row gaps to preserve the 800×600 form layout. Sidebar, grid and List spacing stay independent. Native OS pickers are unchanged; no new command, layer, dependency or schema is involved.

The UI-only `fieldset::show` component (D48) groups related controls with inset content, a theme-styled outline with a legend gap, a real label and an accessible Group. Edit Rules wraps only its instructions/rule inputs; Name, preview and actions remain outside. The editor reserves the inset when aligning Name and widens its content to 464 logical points. No layer, command, dependency or schema change.

App modal headings use the UI-only `dialog::header` component (D50): the title and a right-aligned, accessible X button. Collection and sync shells merge its click with their existing Cancel/Close signal; pending writes/transfers disable the button. Dismissal does not dispatch a command or cancel a running worker. Native picker controls are unchanged.

The shared UI-only `middle_ellipsis::label` component (D51) fits a single line to its available width using measured text layout, retaining the start and end around an ellipsis. The Details inspector uses it for the asset path; hover and accessibility expose the full value. UTF-8 character boundaries are preserved, and very narrow layouts cannot force panel growth. No engine command, model or dependency change.

The UI-only `formatting::file_size` helper (D52) is shared by List rows and the Details inspector. It formats stored byte counts with binary units and keeps unknown/invalid values explicit. This is display formatting; stored sizes and command output are unchanged.

The UI-only `rating` module (D56) shares five-star display/editing across List, Details and Smart rule forms. List calls its display-only variant inside the existing selectable/draggable row; Details and Create/Edit Rules call its editor with five accessible 24-point star buttons in a labelled Group. The editor returns a changed response and updates only local `Option<i64>` view state: Details maps clearing to rating 0 and dispatches `edit`; Smart rule inputs preserve None versus Some(0) and serialize through existing collection commands/previews. Disabled parent UIs suppress edits. D56 added no engine command, dependency or schema change. D57 adds Maximum Rating to the same shared rule form and existing collection commands. The L1 model validates inclusive optional 0–5 bounds and their ordering, and all live queries/previews share the same predicate. Schema v8 rebuilds the rules table to permit maximum-only rules, preserving existing rules, Collection identities/revisions, membership and replica baseline bytes. Rule rows use 4-point gaps so all ten inputs/actions fit 800×600.

The same UI-only formatting module exposes `exposure_time` for Details (D55). It trims decimal seconds to two fractional places, or two significant digits below 0.01 seconds, removing trailing zeros. Nonfinite/nonpositive values show —. It shortens the decimal representation without input-derived indexing, float-to-integer casts or multiplication overflow; stored exposure metadata stays unchanged.

The UI-only `dates::DateFormatter` (D53/D54) is shared by List and Details. It presents numeric regional dates with padded day/month and four-digit years, joined to the system-format time with at. Foundation uses a localized ddMMyyyy template; Windows normalizes native short-date pattern fields; Linux normalizes regional date tokens without changing order/separators. It validates epoch timestamps and ISO local capture values with Chrono, then formats through Foundation on macOS, Windows.Globalization on Windows, or Chrono's regional locale data on Linux (LC_ALL, LC_TIME, LANG precedence; system local zone). Modified is an instant rendered in local time; Captured remains a wall-clock value even in a daylight-saving gap. A 512-entry cache expires after 30 seconds of use so native preferences are re-read. Native failures use an explicit ISO fallback (UTC marked for Modified); invalid input shows —. Test fixtures inject fixed locale/offset settings without process-global changes. No engine, storage or command change.

D20/D44 adds v7 catalogue revision/replica/preview tables without a new crate or dependency. Catalogue triggers track generations and entity revisions; the sync layer owns the bounded binary wire codec and baseline comparisons. Collection UIDs and asset UIDs identify wire state; local row IDs remain local. Preview byte decoding/hashing stays in `icatalog-media`; the engine's shared `previews` module manages embedded/external files and verification, and the `previews` command generates/converts them. Generation uses the imported asset format to distinguish raster TIFF decoding from embedded JPEG extraction for RAW files, because TIFF signatures alone do not distinguish them. Sync calls that same storage seam to serve/cache referenced previews using GET only. D57 keeps the v2 envelope and legacy records intact, adding Collection kind 3 only for rules with a maximum; new readers still decode kind 2 baselines with no maximum. Older readers reject kind 3 before applying a batch, preserving the client draft. Both server and clients need schema-v8 support to exchange these catalogues. GUI full-size viewing and automatic startup sync remain unbuilt. Desktop media commands disable on established replicas; Collection and annotation commands remain editable.

## 6. `ivc` seam and clean-room boundary

`ivc` lives in `ivc-file-research/` by design (D6) and is consumed through a **path dependency**; it stays standalone. Existing API (EXISTS): `parse(&[u8]) -> Result<Catalog>`; `Catalog { header, items, chunks, trailer }`; `Item` with `path`, `name`, `format` (4cc), `file_size`, `width`, `height`, `color_space`, `pict`, `jpeg: Option<&[u8]>`, `tail_blocks`, `iptc: Vec<IptcRecord>`; `Chunk { kind, offset, payload }` including the `FSM!` folder tree. Format spec: `ivc-file-research/docs/ivc-format.md`. Strings are likely Mac Roman (an open RE item); currently decoded lossy.

Boundary: app code reads `ivc` output only; it never copies iView/Expression/Phase One code or assets. Import is one-way into SQLite; the app never writes `.ivc`.

## 7. Media decoding seam

EXIF orientation transforms stay in `icatalog-media` (D31): `thumbnail_jpeg_with_orientation` generates oriented JPEG pixels, while `decode_rgba_with_orientation` supports older stored thumbnails. Import/update pass the same orientation written to `asset_meta`, including for RAW previews. The desktop supplies stored orientation only when `thumbnail.orientation_applied` is false, so grid and detail textures share the result without double rotation. No original media access or catalogue writes are needed for this display fallback.

**Decided** (D8; built today: the raster slice of D13 and the RAW embedded-preview + embedded XMP read slice of D17, hand-rolled with no new dependency):

- Only `icatalog-media` (L1) may hold media/metadata dependencies. Bytes or a seekable reader in, plain data out; never a file path.
- Flow: `ivc -> icatalog-media -> icatalog-catalog`, wired by the engine (L2). `icatalog-media` and `icatalog-catalog` do not depend on each other.
- Legacy-format knowledge (PICT pixel decode, `ex??` tail blocks, Mac Roman, IPTC-IIM) is hand-rolled in `ivc`, which returns plain pixels/bytes and takes no encoder dependency.
- `image` (default features off, explicit codecs, `Limits`) is the single pixel stack; never two decoders in the wasm bundle. EXIF via `kamadak-exif`.
- ArtCraft siblings are sources only (study, vendor or git-dep; commit-pinned; feature-flagged): FilmCraft for video metadata + frames; never their pixel stacks. D17 **superseded** the planned LightCraft route for photo XMP and RAW: `icatalog-media` reads embedded RAW previews and XMP packets by hand, with no new dependency (see section 7 and D17).
- wasm: `icatalog-media` default features must build for `wasm32-unknown-unknown`; the ffmpeg escape hatch is native-only, user-installed, deferred.

Source colour metadata (D32) uses `icatalog-media::read_color_info`: bounded header/ICC description reads through the existing image stack, including v2 `desc` and v4 `mluc` names. The engine stores small UTF-8 display values in the existing `metadata_raw` table, under `image_info`. Import and changed-file Update refresh those values, clearing lost profiles; Update also backfills absent values for older entries without regenerating thumbnails when content is unchanged. Changed-file Update also replaces the `xmp_packet` scheme, clearing it when the new bytes have no XMP (D80), while preserving user annotations. RAW preview colour is not attributed to the original RAW. No dependency or schema migration is added.

## 8. wasm constraints

**Decided:** crates from L0 up, except the CLI/server-only parts of `icatalog-sync` and `icatalog-cli`, must stay compilable for `wasm32-unknown-unknown`. **Status:** the target is NOT installed here, so no wasm check has been run and no wasm status is claimed. **EXISTS** (D18): the sync client is written against a small `Transport` trait so a wasm `fetch` implementation can slot in; `ureq` and `tiny_http` are native-only and the server half (`server.rs`) is the part excluded from a future wasm build.

## 9. Browser storage

**Open** (D3). The browser keeps a local copy and syncs over the same HTTP(S) API. Which storage holds the SQLite data (e.g. OPFS), and how `rusqlite` `bundled` (C code) maps to wasm, are undecided. See `docs/sync.md`.

## 10. Open questions

- Exact crate dependency edges (section 2), including `icatalog-media`.
- Command-registry API and param typing: RESOLVED (D12).
- Data-driven UI mechanism.
- Browser storage and SQLite-on-wasm approach.
- Whether the sync service is mandatory or also a share-file mode (see `docs/sync.md`).

## 11. Desktop client sync (D46)

File → Connect to Server… selects a new local destination and dispatches the L3 `clone` command with `Context::for_destination`; a successful clone becomes the active catalogue. File → Sync… dispatches the L3 `sync` command on the background worker. The UI depends downward on `icatalog-sync` and routes engine-first through both registries. It implements no transfer or conflict policy. Before sync the shell drops its old SQLite connection, replacing it with a temporary in-memory destination context; it blocks catalogue reads and editing while the worker runs. It reopens the on-disk catalogue afterward on success or error, invalidates caches and refreshes browsing/inspector state. A reopen failure offers a retry instead of continuing against the placeholder. Transfers prevent window closing until completion.

The modal retains inputs on errors and shows the complete command output, including deferred/offline results, backups and conflict keys. First sync of an untracked copy requires acknowledging the backup-first behaviour. Local URL preferences are an optional bounded UTF-8 `<catalogue>.sync-url` file: read failures are visible, writes stage/flush/rename atomically, and failed preference writes are separate from transfer results. Successful clones and ordinary sync attempts remember their address; failed clones leave the current catalogue and its preferences untouched. CLI `--url` semantics are unchanged. No automatic sync, schema, protocol, endpoint or new third-party dependency.
