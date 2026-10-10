# iCatalog architecture

Purpose: crate map, layering, and the engine/UI seam. Status vocabulary: **Decided** (owner decision D1-D7 in `docs/decisions.md` or a golden rule in `AGENTS.md`), **Proposed** (designed here, not accepted or built), **Open** (unresolved). Built today: `ivc`, `icatalog-catalog`, `icatalog-media`, `icatalog-engine` and `icatalog-cli` (under `crates/`); everything else app-side is planned.

## 1. Purpose and stack

**Decided.** iCatalog is a Rust-only rebuild of the iView Media Pro media catalogue/organiser: CLI first, egui desktop, wasm browser build as a goal. No JavaScript/TypeScript, no Tauri, Electron or webviews, ever. The own catalogue format is **SQLite**; legacy `.ivc`/`.mpcatalog` is **import-only** through `ivc`. Development is **clean-room**. Machine names are lowercase (`icatalog`, `icatalog-*`, `ivc`); the repo root is `iCatalog/` (D4).

## 2. Crate map

Status column: EXISTS means present in the repo today; PLANNED means not created.

| Crate | Layer | Depends on | Role | Status |
|---|---|---|---|---|
| `ivc` (`ivc-file-research/crates/ivc`) | L0 | nothing in the workspace | Legacy `.ivc` parser | EXISTS |
| `icatalog-catalog` | L1 | `ivc` (for import mapping, Proposed), `rusqlite` (`bundled`) | SQLite schema (migrations v1, v2) + model + CRUD + `AssetQuery`/`CatalogStats` (`query_assets`, `stats`) | EXISTS |
| `icatalog-media` | L1 | `image` 0.25, `kamadak-exif`, `sha2` (no workspace dependency) | Raster probe, EXIF read, grid JPEG thumbnails, content hash (D8, D13) | EXISTS |
| `icatalog-engine` | L2 | L0-L1 | Session `Context` + command registry; commands `init`, `open`, `import`, `list`, `find`, `status`; `dispatch_guarded` panic guard (D12) | EXISTS |
| `icatalog-sync` | L3 | L0-L2 | Sync client + NAS-side HTTP(S) service | PLANNED |
| `icatalog-cli` | L4 | `icatalog-engine` only | Binary `icatalog`: thin wrapper dispatching engine commands by id (`init/open/import/list/find/status`, `--help`, `--version`, `commands`) | EXISTS |
| `icatalog-ui` | L4 | L0-L3 | Thin egui shell, desktop + wasm | PLANNED |
| `testkit` | test-only | as needed | Test helpers | PLANNED |
| `xtask/` (repo root) | tooling | none | `cargo xtask layers`, `ci` | PLANNED |
| `packaging/` | n/a | n/a | Docker image + compose example | PLANNED |

**Decided** layout: one Cargo workspace rooted at the repo top. The workspace members include `ivc` and the `crates/*` glob (check root `Cargo.toml`). The exact dependency edges above (e.g. whether `icatalog-catalog` depends on `ivc` or the engine does the mapping) are **Open**; the layer ordering is Decided.

## 3. Layers

```text
L4  icatalog-cli        icatalog-ui (egui, desktop + wasm)
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
- Nothing below `icatalog-ui` may use egui, eframe, winit or rfd.

**Enforcement.** EXISTS: a layer test at `crates/icatalog-cli/tests/layers.rs` parses each `Cargo.toml` and fails on an upward workspace dependency or on egui/eframe/winit/rfd below `icatalog-ui`. **Proposed:** later replaced or joined by `cargo xtask layers` (PLANNED). New crates must be registered in the test.

## 4. Engine/command seam

**Decided** (golden rule: everything is a command). A command in `icatalog-engine` has: an **id**, a **label**, a params doc, an `enabled` check and a `run` function, plus tests. The CLI and the GUI both dispatch commands by id. CLI subcommands are thin wrappers over commands. Sync is a command too, so CLI and GUI behave identically.

**EXISTS** (D12): the engine holds a session `Context` and a static registry (slice of `&dyn Command`) mapping string id to command; `run` takes string-keyed `Params` validated into typed values (bad input is `EngineError::BadParam`) and returns `Outcome::{Text,Table}` or an error, never panics. `dispatch_guarded` wraps `dispatch` in `catch_unwind` as the last-resort guard.

May never leak into the engine: egui/eframe/winit/rfd types, window or widget state, terminal formatting. Only pure view/window state belongs to the shell.

Commands return `Err` for any input (hostile params included). A fuzz-style test over every command is planned and must stay green once it exists (**Open**: not written).

## 5. UI shell

- **Decided:** `icatalog-ui` is thin and data-driven; it holds no logic and every action goes through engine commands. egui/eframe native desktop. No webview.
- **Decided (goal):** wasm browser build via trunk + wasm-bindgen, Rust only, using the same engine and model as desktop (D3).
- **Open:** the exact data-driven mechanism (e.g. menus generated from the command registry) is not designed.

## 6. `ivc` seam and clean-room boundary

`ivc` lives in `ivc-file-research/` by design (D6) and is consumed through a **path dependency**; it stays standalone. Existing API (EXISTS): `parse(&[u8]) -> Result<Catalog>`; `Catalog { header, items, chunks, trailer }`; `Item` with `path`, `name`, `format` (4cc), `file_size`, `width`, `height`, `color_space`, `pict`, `jpeg: Option<&[u8]>`, `tail_blocks`, `iptc: Vec<IptcRecord>`; `Chunk { kind, offset, payload }` including the `FSM!` folder tree. Format spec: `ivc-file-research/docs/ivc-format.md`. Strings are likely Mac Roman (an open RE item); currently decoded lossy.

Boundary: app code reads `ivc` output only; it never copies iView/Expression/Phase One code or assets. Import is one-way into SQLite; the app never writes `.ivc`.

## 7. Media decoding seam

**Decided** (D8; nothing built, library choices are planned):

- Only `icatalog-media` (L1) may hold media/metadata dependencies. Bytes or a seekable reader in, plain data out; never a file path.
- Flow: `ivc -> icatalog-media -> icatalog-catalog`, wired by the engine (L2). `icatalog-media` and `icatalog-catalog` do not depend on each other.
- Legacy-format knowledge (PICT pixel decode, `ex??` tail blocks, Mac Roman, IPTC-IIM) is hand-rolled in `ivc`, which returns plain pixels/bytes and takes no encoder dependency.
- `image` (default features off, explicit codecs, `Limits`) is the single pixel stack; never two decoders in the wasm bundle. EXIF via `kamadak-exif`.
- ArtCraft siblings are sources only (study, vendor or git-dep; commit-pinned; feature-flagged): LightCraft for photo XMP and RAW bytes + metadata, FilmCraft for video metadata + frames; never their pixel stacks.
- wasm: `icatalog-media` default features must build for `wasm32-unknown-unknown`; the ffmpeg escape hatch is native-only, user-installed, deferred.

## 8. wasm constraints

**Decided:** crates from L0 up, except the CLI/server-only parts of `icatalog-sync` and `icatalog-cli`, must stay compilable for `wasm32-unknown-unknown`. **Status:** the target is NOT installed here, so no wasm check has been run and no wasm status is claimed. Consequence (**Proposed**): server/HTTP-listener code in `icatalog-sync` sits behind a feature or module split so the client half compiles for wasm.

## 9. Browser storage

**Open** (D3). The browser keeps a local copy and syncs over the same HTTP(S) API. Which storage holds the SQLite data (e.g. OPFS), and how `rusqlite` `bundled` (C code) maps to wasm, are undecided. See `docs/sync.md`.

## 10. Open questions

- Exact crate dependency edges (section 2), including `icatalog-media`.
- Command-registry API and param typing: RESOLVED (D12).
- Data-driven UI mechanism.
- Browser storage and SQLite-on-wasm approach.
- Whether the sync service is mandatory or also a share-file mode (see `docs/sync.md`).
