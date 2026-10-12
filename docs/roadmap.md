# iCatalog roadmap

Purpose: milestones and current focus. Status vocabulary: **Decided** (owner order from `AGENTS.md` section 6), **Proposed** (breakdown here), **Open** (unresolved). Milestone status is NOT STARTED unless stated; `icatalog-catalog`, `icatalog-engine` and `icatalog-cli` exist app-side (plus `ivc`).

## 1. Priority rule

**Decided:** format knowledge first (tracked in `ivc-file-research/AGENTS.md`), then the data model, then the interfaces. App-side order: schema, engine + CLI, sync, egui (desktop, then wasm). Exit criteria for every milestone are the workspace gate: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

## 2. Milestones (Proposed breakdown)

| Milestone | Deliverables | Exit criteria | Status |
|---|---|---|---|
| M0 housekeeping | `git init`, `.gitignore` (`target/`, `log/`), `log/devlog.md`, first crate scaffold, `crates/*` in root members | tree builds, gate passes | DONE: repo under git, root `.gitignore` (`target/`, `log/`, `*.sqlite`), `crates/*` in root members, `log/devlog.md` started (local, gitignored) |
| M1 schema | `icatalog-catalog`, schema + migrations per `docs/schema.md`, `ivc` import mapping | round-trip and migration tests, gate passes | LARGELY IMPLEMENTED: schema v1+v2, migrations, model + CRUD for every table, tests (round-trip, FK cascade, constraints, raw bytes, migrations, never-panic). Remaining gap: the `ivc` import mapping (M2 work) |
| M2 engine + CLI | `icatalog-engine` command registry, binary `icatalog` (init/open/import/list/find/update/edit/status), panic guard, layer test; `icatalog-media` (L1, D8): legacy thumbnails (embedded JPEG unchanged; PICT decoded in `ivc`), raster ingest (`image`), EXIF read (`kamadak-exif`), thumbnail stored in SQLite | each command tested, adversarial-params test, gate passes | STARTED: engine registry, `dispatch_guarded` panic guard, CLI `icatalog` with `init/open/list/find/status` and the layer test landed with tests. `icatalog-media` (raster ingest, EXIF, thumbnails) and `import <dir>` landed (D13). `update` (rescan for added/changed/missing, D14) and `edit` (keywords/rating/label, D16) landed. The adversarial-params fuzz test landed (`crates/icatalog-engine/tests/adversarial.rs`). Not built: legacy `.ivc` import (deprioritised), the PICT decoder |
| M3 sync | `icatalog-sync` service (`serve`) + client, per `docs/sync.md`; musl build and Docker packaging | generation-race and interrupted-transfer tests, gate passes | NOT STARTED |
| M4 egui | `icatalog-ui` desktop, then wasm via trunk + wasm-bindgen | desktop builds; wasm target installed and `cargo check` passes for L0+ | STARTED (pulled ahead of M3 by owner, D15): native desktop shell `icatalog-ui` browses assets, shows a stored thumbnail + metadata, edits rating/label/keywords, and dispatches `init`/`import`/`update`/`edit` via engine commands. wasm NOT STARTED |
| Media breadth (post-M4, D8) | XMP (LightCraft), RAW preview + metadata (LightCraft), PSD, HEIC/AVIF metadata-only, video metadata + frame (FilmCraft), disk preview cache | workspace gate + wasm check + cargo-deny clean | NOT STARTED |

PICT pixel decoder (QuickDraw raw-pixmap case) is `ivc` work (ivc-file-research), needed by the M2 legacy-thumbnail task; tracked in `ivc-file-research/AGENTS.md`.

## 3. Current focus

**Decided:** milestone 1, the SQLite schema (`icatalog-catalog`, `docs/schema.md`), is largely implemented; encoding, paths and stable ids are settled (D10) and user annotations (D11). M2 has started: engine + CLI core commands (`init/open/list/find/status`) landed with tests. `icatalog-media` and `import <dir>` have landed (D13). `update` has landed (D14) and `edit` (user annotations: keywords, rating, label, D16) has landed. The adversarial-params fuzz test has landed (`tests/adversarial.rs`). Remaining M2/backlog: legacy `.ivc` import (deprioritised) and the PICT decoder. M4 was pulled ahead of M3 by owner request: the native desktop shell `icatalog-ui` has landed (D15; browse, thumbnail, metadata, rating/label/keyword edit, `init`/`import`/`update`/`edit`); wasm is still to come.

## 4. Backlog (Open)

- Samples corpus: resolved (committed, CC0; `ivc-file-research/samples/fetch.sh` is an optional re-fetch/verifier; `docs/decisions.md` D9).
- Sync service mandatory vs share-file mode.
- Browser storage (e.g. OPFS) and SQLite on wasm.
- Merge strategy for conflicting syncs.
- Sync API shape, auth, TLS.
- `ATTRIBUTION.md` if third-party assets ever land.
- Per-crate `clippy.toml`.
- `xtask` (`layers`, `ci`).
- Install `wasm32-unknown-unknown` and run the first wasm check.

## 5. How priority is decided

The owner sets order (recorded in `docs/decisions.md` and `AGENTS.md`). Crash-safety fixes outrank feature work (**Decided** golden rule). New decisions get a numbered entry in `docs/decisions.md`; this file then follows.
