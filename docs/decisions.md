# Decision log (ADR-lite)

Purpose: records why the repo is shaped as it is. Rules live in `AGENTS.md` and `ivc-file-research/AGENTS.md`; paths are repo-root-relative. All entries Status: Accepted (current).

## D1. Hosting model
Context: options were (a) dedicated NAS server owning the catalogue (recommended), (b) local copies + startup sync, (c) SQLite directly on an SMB/NFS share.
Decision: owner chose **(b)**. (c) rejected: SQLite locking over network shares is unreliable and corrupts databases. (a) remains a viable future alternative. Policy details: `AGENTS.md` section 5.
Status: Accepted.

## D2. Sync transport
Context: clients need to sync with the NAS copy.
Decision: **HTTP(S)** API (`icatalog serve`), an explicit owner choice. API shape undecided; future `docs/sync.md`.
Status: Accepted.

## D3. Web/wasm
Context: dropped from the first AGENTS.md draft as it did not fit the local-DB model.
Decision: owner reinstated it as a **goal, like PhotoCraft** (trunk + wasm-bindgen). The browser keeps a local copy and syncs over the same HTTP(S) API. Browser storage (e.g. OPFS) is an open design item.
Status: Accepted.

## D4. Naming
Context: product was called iView.
Decision: renamed **iView -> iCatalog** (owner, Oct 2026). Repo directory was later flattened/renamed: it is now `iCatalog/` (revised Oct 2026); machine names lowercase (`icatalog`, `icatalog-*`, `ivc`).
Note: before the flatten, the old `iView/` tree (excluding `target/`) was archived to `/var/folders/9q/cpq5z8l51x95cflgy3t2kq_00000gn/T/opencode/iView-backup-20261009-174129.tgz` (temporary; safe to delete once the new root is trusted).
Status: Accepted.

## D5. Folder structure
Decision: split into the app (repo root) and `ivc-file-research/` (format work) by owner instruction. [Superseded Oct 2026: the format work lives inside the app tree at `ivc-file-research/`, and the repo was flattened so the root is iCatalog with a single root `AGENTS.md` guide.] one shared Cargo workspace stays rooted at the repo top.
Status: Accepted.

## D6. Location of the `ivc` crate
Decision: `ivc` parser lives in `ivc-file-research/` (format-knowledge crate), consumed by the app via path dependency. Deliberate, reversible call flagged to the owner.
Status: Accepted.

## D7. Format and stack
Decision: own catalogue format = **SQLite**; legacy `.ivc` import-only. Rust only, no Tauri/webviews, egui desktop (PhotoCraft architecture rules adopted by owner).
Status: Accepted.

## D8. Media decoding and metadata extraction: hybrid, isolated in one crate
Context: iCatalog must extract thumbnails and EXIF/metadata from imported media (legacy `.ivc`, modern photos, RAW, video). Options weighed: hand-roll everything, reuse the ArtCraft sibling projects, or use published crates. Constraints: Rust only, no C/C++ in product code, wasm is a goal, clean-room, permissive licences only (`ATTRIBUTION.md`), never crash, layering.
Decision (owner approved the direction; nothing is built, library choices are planned):
1. **Isolation.** New crate **`icatalog-media`** at **L1** (beside `icatalog-catalog`, no dependency between them) is the **only** crate allowed media/metadata dependencies (`image`, `exif`, `filmcraft-*`, `lightcraft-*`, the optional ffmpeg escape hatch). Bytes or a seekable reader in, plain data out; never a file path (the browser File API yields bytes). Default features must build for `wasm32-unknown-unknown`.
2. **Legacy knowledge is hand-rolled in `ivc`** from public specs and the sample oracles: QuickDraw PICT pixel decode (the classic iView 1.5.7 raw-pixmap case), `ex??` tail blocks, Mac Roman, IPTC-IIM. `ivc` returns plain pixels/bytes and gains no encoder dependency. Embedded JFIF in PICT is already extracted by `ivc`. Flow: `ivc -> icatalog-media -> icatalog-catalog`, wired by the engine (L2).
3. **One pixel stack:** the `image` crate (default features off, explicit codec list, `Limits` required). Never two decoders in the wasm bundle. EXIF read (M2) uses **`kamadak-exif`**. No hand-rolled codecs.
4. **ArtCraft sibling reuse** is approved as a source: study, vendor or git-dep only (nothing is on crates.io), pinned to a **commit** (not a tag) behind feature flags until published.
   - **LightCraft** (`storytold/lightcraft`, MIT OR Apache-2.0, take MIT): planned provider of **XMP** (no permissive pure-Rust XMP crate exists) and **RAW preview + metadata** via its `lightcraft-meta`/`tiff`/`raw` crates only, never its pixel stack (`codecs`/`raster`/`color`).
   - **FilmCraft** (`storytold/filmcraft`, MIT OR Apache-2.0, pure-Rust decoders and containers, `unsafe_code = "forbid"`, no FFmpeg/GPL/LGPL; only `symphonia` MPL-2.0 for audio): planned provider of **video metadata + a single-frame thumbnail** (`filmcraft-media` `MediaSource::video_frame(FrameRequest{time,scale})`, `MediaInfo`).
   - **ffmpeg sidecar is not the default:** an optional, user-installed, native-only escape hatch; never bundled, never auto-downloaded; deferred. Phone/camera H.264/HEVC in MP4/MOV is covered by FilmCraft, keeping us LGPL/GPL-free and wasm-clean.
5. **RAW: preview + metadata only, no demosaic**, deferred past M2. `rawler`/`rawloader` are LGPL-2.1: **banned**. Fallback if LightCraft is not ready (proposed): hand-rolled preview locators for DNG/CR2/NEF/ARW/PEF/RAF/CR3.
6. **HEIC/AVIF: metadata only**, placeholder thumbnail; no libheif/libde265 (LGPL).
7. **Thumbnails:** canonical grid thumbnails live in **SQLite** (the wasm client and sync cannot regenerate them). A LightCraft-style disk preview cache is derived, external, desktop-only, deferred to M4+. Decoded RGBA textures live only in an in-memory UI cache.
8. **Metadata is stored raw-first** plus normalised columns; **user annotations are authoritative** and separate. Display/merge order (iCatalog policy): **EXIF -> IPTC fills gaps -> XMP overrides -> the user's edit wins**.
9. **Enforcement (planned):** cargo-deny licence allow-list + bans (`rawler`, `rawloader`, `ac-ffmpeg`, `ffmpeg-sys*`, libheif, libvips); `ATTRIBUTION.md` generated and diff-checked in CI; a fuzz target per public entry point in `icatalog-media`/`ivc`; `cargo check -p ivc -p icatalog-media --target wasm32-unknown-unknown` joins the gate once the target is installed.
Status: **Accepted (owner, Oct 2026).** Open: exact crate dependency edges; whether the org publishes reusable `lightcraft-*`/`filmcraft-*` crates; `nom-exif` licence (reports `non-standard`) if ever used for video container metadata; RAW fallback effort; whether to replace `kamadak-exif` with a narrow in-house catalogue-EXIF reader (standard IFDs only, no maker notes, fuzzed, with `kamadak-exif` retained as a differential test oracle), only worth it if a TIFF/IFD reader is needed anyway for the legacy `ex??` blocks or RAW preview locators, or if dependency purity / wasm size becomes a concrete problem.

## D9. Legacy sample corpus strategy (revised Oct 2026: now committed)

Context: the 16 oracle samples come from a public research repo; whether to commit them or fetch them was open.

Decision (revised Oct 2026; originally "do not commit, fetch only"): the 16 CC0 sample files (~183 KB) are committed. `ivc-file-research/samples/fetch.sh` plus `SHA256SUMS` are retained as an optional re-fetch/verifier; run `fetch.sh` if the corpus is missing. Samples courtesy of Tyler Thorsted's PRONOM_Research project (https://github.com/thorsted/PRONOM_Research/tree/main/Submissions/iView/Samples), licensed **CC0 1.0** (public domain); fetch.sh pins to commit `2c26686a0577028783b2f24977cdaf78e6ca39fb`.

Status: Accepted (owner, Oct 2026).

## D10. Catalogue encoding, paths and stable ids (Oct 2026)

- **Encoding:** `asset` stores decoded `path`/`name` TEXT plus the original legacy bytes in nullable `path_raw`/`name_raw` BLOBs (raw-first, like `metadata_raw`). `ivc` must expose raw bytes for import (M2).
- **Paths:** `path` is the normalised POSIX form for queries (not UNIQUE); `path_raw` keeps provenance. `folder` + `name` is the canonical location. `asset.mtime` (epoch seconds) supports rescan and `derived_from`.
- **Stable ids:** `id` is the local rowid; `uid TEXT NOT NULL UNIQUE` is assigned by the caller at first import (not generated by `icatalog-catalog`); `content_hash TEXT` is reserved for dedupe/merge.

## D11. User annotations and migration v2 (Oct 2026)

- **Model:** keywords are many-to-many (`keyword` unique by `name`, optional `name_raw` bytes; `asset_keyword` join). `annotation` is one row per asset holding `rating` (0-5), `label` (0-7), and `caption`, `headline`, `creator`, `copyright`, `updated_utc`; CHECK constraints enforce the ranges.
- **Separation:** user annotations are authoritative and stored apart from extracted metadata (`metadata_raw`, `asset_meta`), per D8 merge order (the user's edit wins).
- **Migration:** added as forward-only migration v2 (`v2.sql`) together with `find`/browse indexes on asset folder/name/path/content_hash, folder parent, keyword, rating and label.
Status: Accepted.

## D12. Command registry design (2026-10-10)

Context: AGENTS golden rule 1 says every action is a command; the API shape for the engine (L2) was open.
Decision:
- Every user-visible action is a command registered in `icatalog-engine` with `id`, `label`, params doc, `enabled` and `run`. The registry is a static slice of `&dyn Command`; dispatch is by string id.
- Parameters are a string-keyed `Params` map, validated into typed values per command; invalid or missing input returns `EngineError::BadParam`, never a panic.
- The result is a small `Outcome::{Text, Table}` enum.
- `dispatch_guarded` wraps `dispatch` in `catch_unwind` as the last-resort crash guard.
- Filtered asset queries and aggregate stats live in the L1 `icatalog-catalog` crate (`AssetQuery`, `CatalogStats`, `query_assets`, `stats`), not in the engine.
- The CLI (`icatalog-cli`, binary `icatalog`) is a zero-logic wrapper that dispatches commands by id. Built commands: `init`, `open`, `list`, `find`, `status`.
Consequences: CLI and the future GUI/wasm shell dispatch the same commands. Importing, `sync` and `serve` are not implemented yet.
Status: Accepted.

## D13. First media slice: raster ingest, EXIF and thumbnails (2026-10-10)

Context: M2 needed the first real ingest path; several schema items (thumbnail sizing, provenance, hash form) were Open.
Decision:
- **`icatalog-media` (L1)** has no workspace dependency (bytes in, plain data out, D8). API: `probe(&[u8])`, `read_exif<R: BufRead+Seek>(R)`, `thumbnail_jpeg(&[u8], u32)`, `content_hash(&[u8])`; consts `GRID_LONG_EDGE = 256`, `GENERATOR_VERSION = 1`; types `ImageInfo`, `ExifData`, `Thumbnail`, `MediaError`.
- **Dependencies (D8):** `image` 0.25 (default-features off, explicit codecs jpeg/png/gif/webp/bmp/tiff, decode `Limits`), `kamadak-exif` 0.6.1, `sha2` 0.10.9. Licences are permissive (image MIT OR Apache-2.0, kamadak-exif MIT, sha2 MIT OR Apache-2.0); `ATTRIBUTION.md` and cargo-deny remain backlog.
- **Resolved schema items:** `thumbnail.size_class` = long edge in pixels (grid thumb 256); `source` = `"rendered"` for raster-generated thumbs; `derived_from` = `"<file_size>:<mtime>"`; `orientation_applied` = false for now (pixels not rotated yet); `content_hash` = `"sha256:<lowercase hex>"`; `asset.uid` = that content hash (caller-assigned, D10), giving content-based dedupe across imports/machines.
- **Engine command `import <dir> [--recursive false]`:** bounded walk (max depth 64, symlinks not followed), per-file failures counted and never fatal, extensions jpg/jpeg/png/gif/webp/bmp/tif/tiff, files over 512 MiB skipped, dedupe by `uid` (`Catalog::asset_by_uid`); inserts `asset`, `asset_meta` (from EXIF) and a grid `thumbnail`. Skipped files are reported with per-reason counts in the outcome (`skipped: unsupported file type`, `skipped: larger than 512 MiB`, `skipped: not a recognised image`), so the CLI and GUI say *why* something wasn't imported (e.g. RAW/DNG is off the whitelist today).
Consequences: not built: legacy `.ivc` import (deprioritised), PICT decode, RAW/video/HEIC, disk preview cache, `update`, `sync`, `serve`, UI, the adversarial-params fuzz test; wasm is unchecked (target not installed).
Status: Accepted.

## D14. `update`: rescan for added, changed and missing files (2026-10-10)

Context: M2 needed a way to bring a catalogue back in line with disk.
Decision:
- **Missing is a flag, never a delete:** migration v3 adds `asset.missing INTEGER NOT NULL DEFAULT 0` (+ index). `update` sets it for catalogued files that are gone from disk and clears it when they reappear.
- **`uid` stays stable on content change (D10):** a file at a known path whose bytes changed keeps its `uid`; `content_hash`, size, mtime, dimensions, EXIF metadata and the grid thumbnail are refreshed in place (`insert_thumbnail` now replaces the same `(asset_id, size_class)`). Unchanged size+mtime skips reading the file.
- **Command `update [--path <dir>] [--recursive false]`:** with `--path`, walk that tree (shared bounded walk with `import`), ingest new files, refresh changed ones, and flag catalogued assets inside the directory that are absent. Without `--path`, the roots are the distinct parent directories of catalogued assets, scanned non-recursively; any catalogued path that no longer exists is flagged. Output: scanned/added/changed/unchanged/skipped/missing/errors plus per-reason skip counts, same as `import`. Per-file failures are counted, never fatal; invalid params return `Err`.
- Catalog API: `asset_by_path`, `update_asset`, `set_asset_missing`. Walk and ingest are shared with `import` in `commands/scan.rs`.
Consequences: a moved file is flagged missing at its old path and is not re-added at the new path (its content hash already exists, so it counts as unchanged); move detection is not built. A new path whose content duplicates an existing asset is likewise left un-added.
Status: Accepted.

## D15. First GUI slice: thin native egui shell (`icatalog-ui`) (2026-10-11)

Context: the owner asked to try the app in a UI while sync (M3) was still unbuilt; the engine, catalogue and media crates were already usable. This pulls M4 ahead of M3 by owner request.

Decision:
- New **`icatalog-ui`** crate at **L4** (binary `icatalog-ui <catalogue>`), **native desktop only for now**; the wasm build stays a later goal (D3). Deps: `eframe`/`egui` and `rfd`, GUI-only and allowed for `icatalog-ui` by the layer rule; no media dependency.
- **Thin and data-driven** (golden rule 4): browsing reads the local catalogue directly (`query_assets`, `thumbnail`), while every action (`init` on a fresh path, `import` via a folder picker, `update`) is dispatched as an engine command through `dispatch_guarded`, so CLI and GUI behave identically.
- To keep media deps isolated (D8), the only media addition is **`icatalog-media::decode_rgba(&[u8]) -> DecodedImage`** (bytes in, RGBA8 out, `Limits`-bounded); the shell turns that into an egui texture.
- `icatalog-ui` is compiled natively only; the wasm target is not installed and no wasm status is claimed.

Status: Accepted (owner requested the slice, Oct 2026).

## Open questions
Share-file mode: see the guides. (Samples licensing/commit strategy is resolved: D9.)
