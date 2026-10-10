# iCatalog roadmap

Purpose: milestones and current focus. Status vocabulary: **Decided** (owner order from `AGENTS.md` section 6), **Proposed** (breakdown here), **Open** (unresolved). Milestone status is NOT STARTED unless stated; nothing app-side is built.

## 1. Priority rule

**Decided:** format knowledge first (tracked in `ivc-file-research/AGENTS.md`), then the data model, then the interfaces. App-side order: schema, engine + CLI, sync, egui (desktop, then wasm). Exit criteria for every milestone are the workspace gate: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

## 2. Milestones (Proposed breakdown)

| Milestone | Deliverables | Exit criteria | Status |
|---|---|---|---|
| M0 housekeeping | `git init`, `.gitignore` (`target/`, `log/`), `log/devlog.md`, first crate scaffold, `crates/*` in root members | tree builds, gate passes | NOT STARTED |
| M1 schema | `icatalog-catalog`, schema + migrations per `docs/schema.md`, `ivc` import mapping | round-trip and migration tests, gate passes | NOT STARTED (current focus) |
| M2 engine + CLI | `icatalog-engine` command registry, binary `icatalog` (init/open/import/list/find/update/status), panic guard, layer test; `icatalog-media` (L1, D8): legacy thumbnails (embedded JPEG unchanged; PICT decoded in `ivc`), raster ingest (`image`), EXIF read (`kamadak-exif`), thumbnail stored in SQLite | each command tested, adversarial-params test, gate passes | NOT STARTED |
| M3 sync | `icatalog-sync` service (`serve`) + client, per `docs/sync.md`; musl build and Docker packaging | generation-race and interrupted-transfer tests, gate passes | NOT STARTED |
| M4 egui | `icatalog-ui` desktop, then wasm via trunk + wasm-bindgen | desktop builds; wasm target installed and `cargo check` passes for L0+ | NOT STARTED |
| Media breadth (post-M4, D8) | XMP (LightCraft), RAW preview + metadata (LightCraft), PSD, HEIC/AVIF metadata-only, video metadata + frame (FilmCraft), disk preview cache | workspace gate + wasm check + cargo-deny clean | NOT STARTED |

PICT pixel decoder (QuickDraw raw-pixmap case) is `ivc` work (ivc-file-research), needed by the M2 legacy-thumbnail task; tracked in `ivc-file-research/AGENTS.md`.

## 3. Current focus

**Decided:** milestone 1, the SQLite schema (`icatalog-catalog`, `docs/schema.md`). M0 housekeeping may run alongside. Before M1 can finish, the schema's open items (encoding, path normalisation, stable asset ids) need decisions.

## 4. Backlog (Open)

- Samples corpus: resolved (committed, CC0; `ivc-file-research/samples/fetch.sh` is an optional re-fetch/verifier; `docs/decisions.md` D9).
- Sync service mandatory vs share-file mode.
- Browser storage (e.g. OPFS) and SQLite on wasm.
- Merge strategy for conflicting syncs.
- Sync API shape, auth, TLS.
- Fuzz-style adversarial tests for commands and import.
- `ATTRIBUTION.md` if third-party assets ever land.
- Per-crate `clippy.toml`.
- `xtask` (`layers`, `ci`).
- Install `wasm32-unknown-unknown` and run the first wasm check.

## 5. How priority is decided

The owner sets order (recorded in `docs/decisions.md` and `AGENTS.md`). Crash-safety fixes outrank feature work (**Decided** golden rule). New decisions get a numbered entry in `docs/decisions.md`; this file then follows.
