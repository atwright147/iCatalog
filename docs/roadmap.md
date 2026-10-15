# iCatalog roadmap

Purpose: what is built, what is in progress, and what comes next, app-side. Crate map and layering live in `docs/architecture.md`; numbered decisions in `docs/decisions.md`; the format study in `ivc-file-research/AGENTS.md`. Status vocabulary: **Decided** (owner order), **Proposed** (breakdown), **Open** (unresolved). The exit criteria for every milestone are the workspace gate: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

App-side crates that exist today: `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli`, `icatalog-ui` (plus `ivc`, in `ivc-file-research/`).

## 1. Priority rule

**Decided:** format knowledge first, then the data model, then the interfaces. App-side order: schema, engine + CLI, sync, egui (desktop, then wasm); M4 was pulled ahead of M3 by owner request (D15). Crash-safety fixes outrank feature work (golden rule).

## 2. Done

- **M0 housekeeping** — repo under git; root `.gitignore` (`target/`, `log/`, `*.sqlite`); `crates/*` workspace members; `log/devlog.md` (local, gitignored).
- **M1 schema** — `icatalog-catalog`: schema v1+v2+v3 with migrations, the model + CRUD for every table, `AssetQuery`/`CatalogStats`; round-trip, FK-cascade, constraint, raw-bytes, migration and never-panic tests. The one open piece — the `ivc` import mapping — is M2 work, not schema work.

## 3. In progress

- **M2 engine + CLI** — *landed:* the `icatalog-engine` command registry + `dispatch_guarded` panic guard; the `icatalog` commands `init`, `open`, `import` (D13), `list`, `find`, `status`, `update` (D14) and `edit` (D16); the layer test; `icatalog-media` raster ingest (`image`), EXIF (`kamadak-exif`) and grid thumbnails; per-reason skip reporting in `import`/`update`; the adversarial-params fuzz test (`crates/icatalog-engine/tests/adversarial.rs`). *Open:* legacy `.ivc`/`.mpcatalog` import (deprioritised) and the PICT pixel decoder (`ivc` work, tracked in `ivc-file-research/AGENTS.md`).
- **M4 egui (desktop)** — *landed:* `icatalog-ui`, a thin native egui shell (D15) that browses assets, shows a stored thumbnail + metadata, edits rating/label/keywords, and dispatches `init`/`import`/`update`/`edit` as engine commands. *Open:* the wasm build (trunk + wasm-bindgen; the target is not installed and no wasm status is claimed).
- **M3 sync** — *landed (D18):* the `icatalog-sync` crate with the NAS-side HTTP service (`serve`: `GET /info`, `GET /catalogue`, `POST /catalogue?expected=`) and client (`sync`: generation-guarded push/pull, refuse-and-resync, timestamped backup before any overwrite, temp-file + integrity check + atomic swap); offline is deferred, never fatal. `sync`/`serve` are wired into the `icatalog` CLI (the shell merges the engine and sync registries) with tests for a round trip, a refused push (backup + resync), an invalid snapshot, offline deferral and hostile params. *Open:* TLS and auth; delta/incremental transfer; conflict merge; no dirty flag, so an idle re-sync re-pushes and bumps the generation.
- **Media breadth (D8/D17)** — *landed:* RAW embedded-preview thumbnails, RAW container format + true dimensions, and XMP packet storage with seed-on-first-ingest of rating/label/keywords — all hand-rolled with no new dependency (D17). *Open:* sidecar `.xmp`; PSD; HEIC/AVIF (metadata only); video metadata + frame (FilmCraft); the derived desktop-only disk preview cache (D8, deferred to M4+). No demosaic, by design.

## 4. Next

- **M3 sync, deployment half** — the musl build and Docker packaging (`packaging/`) for `icatalog serve` on a NAS (D18 built the service itself). Exit criteria: a static musl build, a compose example, gate passes.
- Then close the M2/M3 gaps above; `wasm32-unknown-unknown` install and the first wasm check join the gate once the target is available.

## 5. Open backlog

- Sync: share-file mode vs a mandatory service; auth/TLS; delta transfer; conflict merge (must keep the same safety guarantees).
- `ATTRIBUTION.md` and cargo-deny, if third-party assets ever land (D17 added no new dependency).
- Per-crate `clippy.toml`.
- `xtask` (`layers`, `ci`).
- Install `wasm32-unknown-unknown` and run the first wasm check.

## 6. How priority is decided

The owner sets order (recorded in `docs/decisions.md` and `AGENTS.md`); crash-safety fixes outrank feature work (**Decided** golden rule). New decisions get a numbered entry in `docs/decisions.md`; this file then follows.
