# iCatalog architecture

Purpose: crate map, layering, and the engine/UI seam. Status vocabulary: **Decided** (owner decision D1-D18 in `docs/decisions.md` or a golden rule in `AGENTS.md`), **Proposed** (designed here, not accepted or built), **Open** (unresolved). Built today: `ivc`, `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli` and `icatalog-ui` (under `crates/`); everything else app-side is planned.

## 1. Purpose and stack

**Decided.** iCatalog is a Rust-only rebuild of the iView Media Pro media catalogue/organiser: CLI first, egui desktop, wasm browser build as a goal. No JavaScript/TypeScript, no Tauri, Electron or webviews, ever. The own catalogue format is **SQLite**; legacy `.ivc`/`.mpcatalog` is **import-only** through `ivc`. Development is **clean-room**. Machine names are lowercase (`icatalog`, `icatalog-*`, `ivc`); the repo root is `iCatalog/` (D4).

## 2. Crate map

Status column: EXISTS means present in the repo today; PLANNED means not created.

The shared user-facing name is `icatalog_catalog::APP_NAME`. The catalogue crate is the lowest existing app layer shared by the UI, engine and sync code that display it, so window/menu titles and runtime messages use the same constant without another dependency. Executable, crate and file names remain lowercase; documentation and Cargo descriptions use the product spelling directly.

| Crate | Layer | Depends on | Role | Status |
|---|---|---|---|---|
| `ivc` (`ivc-file-research/crates/ivc`) | L0 | nothing in the workspace | Legacy `.ivc` parser | EXISTS |
| `icatalog-catalog` | L1 | `ivc` (for import mapping, Proposed), `rusqlite` (`bundled`) | SQLite schema (migrations v1-v5) + model + CRUD + `AssetQuery`/`CatalogStats` (`query_assets`, `stats`) | EXISTS |
| `icatalog-media` | L1 | `image` 0.25, `kamadak-exif`, `sha2` (no workspace dependency) | Raster probe, EXIF read, grid JPEG thumbnails, content hash, RGBA decode (D8, D13); optional EXIF orientation for thumbnails/display (D31); `raw` (embedded JPEG preview of DNG/CR2/NEF/ARW/PEF/RAF/CR3) and `xmp` (packet locate + tolerant parse) modules (D17) | EXISTS |
| `icatalog-engine` | L2 | L0-L1 | Session `Context` + command registry; commands `init`, `open`, `import`, `list`, `find`, `status`, `update`, `edit`, `collection`; `dispatch_guarded` panic guard (D12, D14, D16) | EXISTS |
| `icatalog-sync` | L3 | L0-L2, `tiny_http`, `ureq` | Whole-snapshot sync (D18): NAS-side HTTP `serve` + client `sync` with a generation guard, refuse-and-resync, timestamped backup and atomic swap, plus a local change marker so an idle re-sync is a no-op (D19); exposes its own command registry. Decided target (D20, **not built**): annotation-delta push + content-addressed pull-only previews | EXISTS |
| `icatalog-cli` | L4 | `icatalog-engine`, `icatalog-sync` | Binary `icatalog`: thin wrapper dispatching engine + sync commands by id (`init/open/import/list/find/status/update/edit/collection/sync/serve`, `--help`, `--version`, `commands`) | EXISTS |
| `icatalog-ui` | L4 | `icatalog-catalog`, `icatalog-engine`, `icatalog-media`, `eframe`/`egui`, `rfd`, `muda` (native macOS menu) | Binary `icatalog-ui`: thin native egui desktop shell (browse, thumbnail, metadata, `init`/`import`/`update`/`edit`/`collection`); wasm PLANNED; sync buttons not wired yet | EXISTS (native) |
| `testkit` | test-only | as needed | Test helpers | PLANNED |
| `xtask/` (repo root) | tooling | none | `cargo xtask layers`, `ci` | PLANNED |
| `packaging/` | tooling | OS archivers, FPM for Linux | Desktop release archives/packages and Linux launcher (D34); Docker image + compose example remain planned | EXISTS (desktop); PLANNED (Docker) |

**Decided** layout: one Cargo workspace rooted at the repo top. The workspace members include `ivc` and the `crates/*` glob (check root `Cargo.toml`). The exact dependency edges above (e.g. whether `icatalog-catalog` depends on `ivc` or the engine does the mapping) are **Open**; the layer ordering is Decided.

Release automation (D34) packages the existing L4 binaries without adding an app crate or changing the engine seam. `.github/workflows/release.yml` builds Windows x64/ARM64, macOS ARM64 and Linux x86-64, tests native Linux package installation, then attaches the six assets and SHA-256 checksums to the published release. Manual dispatch keeps artifacts only unless an existing `release_tag` is supplied. Recovery compiles that tag's app code while loading `packaging/` and `Taskfile.yml` from the dispatched workflow revision, so package fixes apply without moving app tags. Every runner explicitly installs Go before compiling Task. Hosted recovery passed all four builds and Debian/Fedora installation; Arch's invalid spaced dependency is corrected with a generated-metadata check. End-to-end upload remains unverified. NAS musl/Docker and wasm remain separate planned work.

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

**EXISTS** (D12): the engine holds a session `Context` and a static registry (slice of `&dyn Command`) mapping string id to command; `run` takes string-keyed `Params` validated into typed values (bad input is `EngineError::BadParam`) and returns `Outcome::{Text,Table}` or an error, never panics. `dispatch` runs each engine command in a SQLite transaction and rolls back on an error, panic or failed commit (D23). `dispatch_guarded` also catches panics outside command execution. Sync retains its separate file-replacement protocol.

**EXISTS** (D18): the sync commands `sync` and `serve` live in `icatalog-sync` (L3), not the engine (L2 must not depend on L3). `icatalog-sync` exposes the same shape (`commands()`, `find_command`, `dispatch_guarded`); the L4 shells merge the two registries and dispatch engine-first. The sync registry is not part of `icatalog-engine`, so the engine's fuzz test covers only its own commands; `crates/icatalog-sync/tests/sync.rs` covers hostile sync params.

May never leak into the engine: egui/eframe/winit/rfd types, window or widget state, terminal formatting. Only pure view/window state belongs to the shell.

Commands return `Err` for any input (hostile params included). A fuzz-style test over every command **EXISTS** at `crates/icatalog-engine/tests/adversarial.rs` and must stay green.

## 5. UI shell

- **EXISTS (D15):** `icatalog-ui` is a thin, data-driven native egui desktop shell. Browsing reads the local catalogue (`query_assets`, `thumbnail`, `annotation`, `asset_keywords`); every action (`init`, `import`, `update`, per-asset `edit` of rating/label/keywords, and manual `collection` actions) is dispatched as an engine command, so CLI and GUI behave identically. The shell runs import/update/edit/collection on a single background worker using the same engine dispatcher, reports failures, and draws only visible grid/list rows with visible/selected thumbnail caching (D23). Manual Collections (D25) use the existing v1 tables and `AssetQuery::collection_id`/`collection_ids`; the sidebar creates collections through an egui modal (D26) and selects collections and edits the selected asset's membership through the engine. Grid/list drag-and-drop and right-click collection actions use the same command/worker (D27). Modifier selection (D28) stays in shell state: Shift selects a display-order range, Ctrl/⌘ toggles collections, and the catalogue query returns their combined assets without duplicates. Smart Collections remain planned. Asset queries still fetch all matching rows. egui/eframe native desktop; no webview. Display decoding uses `icatalog-media::decode_rgba_with_orientation` (D8, D31).
- **Decided (goal):** wasm browser build via trunk + wasm-bindgen, Rust only, using the same engine and model as desktop (D3).
- **Open:** the exact data-driven mechanism (e.g. menus generated from the command registry) is not designed.

UI verification (D24): `egui_kittest` is a UI-only dev dependency. Headless tests instantiate the same eframe App without installing OS menus; screenshot tests use its real renderer at fixed viewport/theme/scale. The native smoke integration test launches the production binary and operates macOS menus/dialog cancellation through System Events. Test fixtures and snapshot baselines live under `crates/icatalog-ui`; no test logic enters the engine or catalogue layers.

Visual comparisons allow no failed pixels. Two Linux 800×600 screenshots with a clipped thumbnail use a per-pixel YIQ colour tolerance of 1.0, based on inspected Mesa/Metal differences of at most two red-channel levels in two pixels. Other snapshots/platforms keep the library's 0.6 tolerance; reviewed baselines remain unchanged.

List view (D32) renders a thumbnail and three metadata columns per asset: filename/rating/captured and modified dates/filesize; document type/camera/lens/focal length; dimensions/colour model/ICC description. It uses the browsing width by default; its Details toggle opens the existing inspector. Narrow windows and an open inspector use horizontal scrolling. Metadata reads are cached only for visible/selected rows and invalidated after refresh; errors are surfaced without repeating failed reads every frame. List display works from stored catalogue data, including offline media.

Desktop typography (D33) is configured once for both egui themes in the UI-only `typography` module. Interface/caption text is 15 pt, List metadata 14 pt, small text 13 pt and headings 20 pt. Text and selection colours have tested contrast; metadata is no longer dimmed. Font hinting is enabled and fractional glyph variants are disabled, while theme-specific atlas gamma and eframe's native monitor scaling are preserved. No font assets, dependencies or engine commands are added.

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

Source colour metadata (D32) uses `icatalog-media::read_color_info`: bounded header/ICC description reads through the existing image stack, including v2 `desc` and v4 `mluc` names. The engine stores small UTF-8 display values in the existing `metadata_raw` table, under `image_info`. Import and changed-file Update refresh those values, clearing lost profiles; Update also backfills absent values for older entries without regenerating thumbnails when content is unchanged. RAW preview colour is not attributed to the original RAW. No dependency or schema migration is added.

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
