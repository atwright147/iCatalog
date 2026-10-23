# iCatalog roadmap

Purpose: what is built, what is in progress, and what comes next, app-side. Crate map and layering live in `docs/architecture.md`; numbered decisions in `docs/decisions.md`; the format study in `ivc-file-research/AGENTS.md`. Status vocabulary: **Decided** (owner order), **Proposed** (breakdown), **Open** (unresolved). The exit criteria for every milestone are the workspace gate: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

App-side crates that exist today: `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli`, `icatalog-ui` (plus `ivc`, in `ivc-file-research/`).

## 1. Priority rule

**Decided:** format knowledge first, then the data model, then the interfaces. App-side order: schema, engine + CLI, sync, egui (desktop, then wasm); M4 was pulled ahead of M3 by owner request (D15). Crash-safety fixes outrank feature work (golden rule).

## 2. Done

- **M0 housekeeping** — repo under git; root `.gitignore` (`target/`, `log/`, `*.sqlite`); `crates/*` workspace members; `log/devlog.md` (local, gitignored).
- **M1 schema** — `icatalog-catalog`: schema v1-v5 with migrations, the model + CRUD for every table (including `source` with device provenance and source-rooted folders, D21), `AssetQuery`/`CatalogStats`; round-trip, FK-cascade, constraint, raw-bytes, migration and never-panic tests. The one open piece — the `ivc` import mapping — is M2 work, not schema work.

## 3. In progress

- **M2 engine + CLI** — *landed:* the `icatalog-engine` command registry + `dispatch_guarded` panic guard; the `icatalog` commands `init`, `open`, `import` (D13, now source-rooted folders with `--kind`, D21), `list`, `find`, `status`, `update` (D14, offline sources are not marked missing, D21) and `edit` (D16); the layer test; `icatalog-media` raster ingest (`image`), EXIF (`kamadak-exif`) and grid thumbnails; per-reason skip reporting in `import`/`update`; the adversarial-params fuzz test (`crates/icatalog-engine/tests/adversarial.rs`). *Open:* a `source`/`folder` command surface, Collections (manual and smart), legacy `.ivc`/`.mpcatalog` import (deprioritised) and the PICT pixel decoder (`ivc` work, tracked in `ivc-file-research/AGENTS.md`).
- **M4 egui (desktop)** — *landed:* `icatalog-ui`, a thin native egui shell (D15) that browses assets, shows a stored thumbnail + metadata, edits rating/label/keywords, and dispatches `init`/`import`/`update`/`edit` as engine commands. The left panel has a source-rooted folder tree grouped under collapsible device headers (D21; selecting a device shows all its sources' assets), the centre a grid/list view with search, and the right a details panel. *Open:* Collections in the UI, recursive folder selection, and the wasm build (trunk + wasm-bindgen; the target is not installed and no wasm status is claimed).
- **M3 sync** — *landed (D18):* the `icatalog-sync` crate with the NAS-side HTTP service (`serve`: `GET /info`, `GET /catalogue`, `POST /catalogue?expected=`) and client (`sync`: generation-guarded push/pull, refuse-and-resync, timestamped backup before any overwrite, temp-file + integrity check + atomic swap); offline is deferred, never fatal. A local change marker (D19) makes an idle re-sync a no-op (no push, no generation bump); any local edit is still pushed. `sync`/`serve` are wired into the `icatalog` CLI (the shell merges the engine and sync registries) with tests for a round trip, a refused push (backup + resync), an invalid snapshot, offline deferral, hostile params, an idle re-sync and a pushed local change. *Decided target (D20, **not built**):* the model moves from bidirectional whole-snapshot to **server-authoritative with an annotation-delta push** (rating/label/keywords keyed by stable asset uid; never media or previews) and **content-addressed, pull-only previews** (`GET /preview/<sha>`), with the preview store chosen per catalogue — embedded for a portable single-file `~/Pictures` catalogue, external for the NAS. See `docs/sync.md` (Target model) and D20. *Open:* the D20 slice below; TLS and auth; delta/incremental transfer; conflict merge.
- **Media breadth (D8/D17)** — *landed:* RAW embedded-preview thumbnails, RAW container format + true dimensions, and XMP packet storage with seed-on-first-ingest of rating/label/keywords — all hand-rolled with no new dependency (D17). *Open:* sidecar `.xmp`; PSD; HEIC/AVIF (metadata only); video metadata + frame (FilmCraft); the preview store (D8's derived disk preview cache, now scoped by D20 as the per-catalogue embedded-or-external preview store with a 1080p+ tier — not built). No demosaic, by design.

## 4. Next

- **Collections (D21)** — manual sets/albums on the existing `collection`/`collection_asset` tables, then Smart Collections storing `AssetQuery`-shaped rules over format, capture date, lens and other metadata. Needs a command surface (`collection create/add/remove`, `smart create`) and UI. Not started.
- **M3 sync, D20 slice** (decided, not built) — implement the target model: generation bump on any catalogue write; annotation-delta push with a per-item conflict policy; preview generation with the per-catalogue store (embedded vs external); `GET /preview/<sha>`; retire the whole-snapshot push. Testable on this machine. See D20 and `docs/sync.md`.
- **M3 sync, deployment half** — the musl build and Docker packaging (`packaging/`) for `icatalog serve` on a NAS (D18 built the service itself). Exit criteria: a static musl build, a compose example, gate passes. Not verifiable on this machine (no musl target, no cross linker, no Docker).
- Then close the M2/M3 gaps above; `wasm32-unknown-unknown` install and the first wasm check join the gate once the target is available.

## 5. Open backlog

- Sync: the D20 target model (annotation-delta push, preview store, content-addressed previews) and its open sub-points (preview sizes/quality, per-item conflict granularity, the `previews --external|--embed` command, whether the write endpoint survives); share-file mode vs a mandatory service; auth/TLS; delta transfer; conflict merge (must keep the same safety guarantees).
- `ATTRIBUTION.md` and cargo-deny, if third-party assets ever land (D17 added no new dependency).
- Per-crate `clippy.toml`.
- `xtask` (`layers`, `ci`).
- Install `wasm32-unknown-unknown` and run the first wasm check.

## 6. How priority is decided

The owner sets order (recorded in `docs/decisions.md` and `AGENTS.md`); crash-safety fixes outrank feature work (**Decided** golden rule). New decisions get a numbered entry in `docs/decisions.md`; this file then follows.
