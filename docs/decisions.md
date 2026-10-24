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
- New **`icatalog-ui`** crate at **L4** (binary `icatalog-ui <catalogue>`), **native desktop only for now**; the wasm build stays a later goal (D3). Deps: `eframe`/`egui`, `rfd` and `muda` (the native macOS menu bar), GUI-only and allowed for `icatalog-ui` by the layer rule; no media dependency.
- **Thin and data-driven** (golden rule 4): browsing reads the local catalogue directly (`query_assets`, `thumbnail`, `annotation`, `asset_keywords`), while every action (`init` on a fresh path, `import` via a folder picker, `update`, and per-asset `edit`) is dispatched as an engine command through `dispatch_guarded`, so CLI and GUI behave identically.
- To keep media deps isolated (D8), the only media addition is **`icatalog-media::decode_rgba(&[u8]) -> DecodedImage`** (bytes in, RGBA8 out, `Limits`-bounded); the shell turns that into an egui texture.
- `icatalog-ui` is compiled natively only; the wasm target is not installed and no wasm status is claimed.

Status: Accepted (owner requested the slice, Oct 2026).

## D16. Metadata editing: a dedicated `edit` command (2026-10-12)

Context: the organiser needs iView-style annotations (keywords, rating, label) that the user sets, stored authoritatively in the v2 tables (D11). AGENTS §4 once sketched metadata editing under `update`; since `update` became the rescan command (D14), folding edits into it would mix two unrelated concerns in one command.

Decision:
- New **`edit`** engine command: `edit <catalogue> --id <asset id> [--rating 0-5] [--label 0-7] [--keyword a[,b..]] [--remove-keyword a[,b..]] [--clear-keywords true]`, dispatched by CLI and GUI like every other command (golden rule 1). Scope: keywords, rating and label; caption/headline/creator/copyright are a later slice. It is deliberately separate from `update` (rescan).
- **One asset per invocation, addressed by row id** (ids shown by `list`). Batch edits against a query match set are a later feature.
- **Edit semantics:** keywords and asset links use `INSERT OR IGNORE` (idempotent); removal deletes only the asset link (keyword rows survive). Annotation rows are upserted on `asset_id`, patching touched fields while preserving others, and `updated_utc` is refreshed when a rating/label change lands. User-set values stay authoritative (D11).
- Validation: `rating` 0-5 and `label` 0-7 must parse and be in range; no edits requested, an unknown/absent `id`, or `--clear-keywords` other than `true`/`false` is a `BadParam`; an uninitialised file is `Unsupported`.
Status: Accepted.

## D17. RAW preview + XMP read with zero new dependencies (2026-10-12)

Context: media breadth (D8) promised RAW preview + metadata and XMP, planned via the LightCraft sibling crates (`storytold/lightcraft`). Those are not on crates.io and are not present in this workspace, and the only mature pure-Rust RAW decoders (`rawler`/`rawloader`) are LGPL-2.1, which D8 bans. D8 already anticipated a hand-rolled fallback ("preview locators for DNG/CR2/NEF/ARW/PEF/RAF/CR3"). The schema already reserves the slots this needs: `thumbnail.source = 'raw_preview'` and `metadata_raw.scheme = 'xmp_packet'`. The owner chose to add **no new dependency** rather than a crate or a git dependency, and to cover CR3/RAF in the first slice.

Decision:
- **RAW is embedded preview + metadata only, never demosaic** (D8 stands). `icatalog-media` gains a `raw` module: content sniffing, then extraction of the largest embedded JPEG preview, bounded and hostile-byte-safe. Containers: TIFF-based DNG/CR2/NEF/ARW/PEF (bounded IFD walk to the preview tags), Fuji RAF (header magic) and Canon CR3 (ISO-BMFF box walk to the `PRVW` box), with a bounded JPEG-segment walk as fallback. No demosaic, no RAW pixel decode.
- **The preview bytes are the pivot:** the existing `probe`, `read_exif` and `thumbnail_jpeg` run on them, so RAW metadata comes from the preview's own EXIF and the grid thumbnail is stored with `source = 'raw_preview'` (`GENERATOR_VERSION` unchanged). No new decoder and no new dependency; wasm-safe by construction.
- **XMP read:** locate the embedded packet (JPEG APP1 header `http://ns.adobe.com/xap/1.0/\0`, PNG `iTXt` with keyword `XML:com.adobe.xmp`, TIFF/DNG tag 700) with a bounded, hand-rolled reader (no XML crate). Store the packet verbatim in `metadata_raw` with `scheme = 'xmp_packet'`; a small tolerant parser extracts `dc:subject` (keywords), `xmp:Rating`, `xmp:Label` and `dc:description`.
- **Merge policy (D8): EXIF -> IPTC -> XMP -> the user's edit wins.** XMP-derived rating/label/keywords seed the `annotation`/`keyword` tables only when the asset has no user value, implemented as **seed-on-first-ingest** (a freshly inserted asset); an already-known asset (uid dedupe on re-import or `update`) is never re-seeded, so a user's `edit` (D16) is never overwritten. Per-field provenance is deferred.
- Engine: `EXTS` gains the RAW extensions and a new `SkipReason` reports RAW files that yield no usable preview; an absent XMP packet is normal, not a skip. Sidecar `.xmp` files are out of scope for now (the media crate is path-free, D8).
- **Recorded format and size (RAW):** the asset's `format` is the container (lowercased file extension, e.g. `dng`, `arw`). `width`/`height` are recorded from the raw sub-IFD (`raw_dimensions`: NewSubfileType bit 0 clear or absent, largest by area), else from the embedded EXIF ExifIFD `PixelXDimension`/`PixelYDimension`, and are left UNKNOWN (NULL) when neither is available. They are explicitly never the embedded preview's dimensions (owner decision: that would be misleading); the thumbnail still derives from the preview. The UI shows "dimensions unknown". Open: CR3/RAF dimension extraction (currently NULL).
- Still **banned**: `rawler`/`rawloader`/libraw and any C/C++ RAW stack (D8). No new dependency means no `ATTRIBUTION.md` or cargo-deny row is added by this decision.
Status: Accepted (owner chose zero new dependencies and the CR3/RAF scope, Oct 2026). Implementation: the RAW-preview + XMP read/store/seed slice is IMPLEMENTED (`icatalog-media` `raw`/`xmp` modules wired into the engine ingest path; `metadata_raw` writes are idempotent via `INSERT OR REPLACE`; XMP seeds rating/caption/keywords on first ingest only; the `xmp:Label` colour name is mapped to the numeric label 0-7 in the engine ingest). Open: sidecar `.xmp` is still out of scope.

## D18. Sync transport and where sync commands live

Context: M3 needs the sync client and the NAS-side service (`docs/sync.md`). Two live choices: the HTTP stack, and which crate owns the `sync`/`serve` commands — the engine (`icatalog-engine`, L2) may not depend on `icatalog-sync` (L3), so the commands cannot simply join the engine registry.

Decision:
- **Commands live in `icatalog-sync` (L3)**, not the engine. `icatalog-sync` exposes its own registry (`commands()`, `find_command`, `dispatch_guarded`, with the same panic guard as the engine); the L4 shells (`icatalog-cli`, later `icatalog-ui`) merge it with `icatalog_engine::commands()` and dispatch engine-first. This keeps the layer rule (L2 never depends on L3) while every action stays a command (golden rule 1).
- **Transport: `tiny_http` (server) + `ureq` (client)**, both `MIT OR Apache-2.0`. Chosen over hyper/tokio for a small, blocking, native-only footprint, avoiding a new async runtime in the workspace. The client sits behind a small `Transport` trait so a wasm `fetch` implementation can slot in later (wasm rule). `ureq` is built with `default-features = false`, so this slice is **plain HTTP only**; TLS is Open.
- **Wire protocol (first slice):** `GET /info` (`id`, `generation` as `key=value` lines), `GET /catalogue` (raw SQLite bytes; `X-Catalogue-Id` + `X-Generation` headers), `POST /catalogue?expected=<gen>` (raw bytes; `200` + new `X-Generation` on accept, `409` + current `X-Generation` on refusal). A snapshot body is capped (4 GiB) and verified before it replaces anything (opens as an iCatalog catalogue with the expected id; `PRAGMA integrity_check`).
- **Policy:** server ahead of the client -> back up and pull; level -> push (a refusal is a race: back up, resync, report); client ahead of the server -> refuse. A local copy is backed up (`name.rejected-<UTC>.sqlite`) before any overwrite; downloads land in a temp file and are swapped in atomically after verification.
- **Limit of this slice:** as first landed there was no change marker, so an idle re-sync re-pushed and bumped the generation (fixed by D19). Delta transfer and merge are later work.

Status: Accepted (owner chose tiny_http + ureq and the `icatalog-sync` command registry, Oct 2026). Implementation: the whole-snapshot `serve`/`sync` slice EXISTS (`crates/icatalog-sync`; CLI `sync`/`serve` wired; help and `commands` list them). Tests cover a real push/pull round trip, a refused push (backup + resync), an invalid snapshot that leaves the local copy untouched, offline deferral and hostile params. Open: TLS, auth, incremental transfer, conflict merge (the D1/D2 policy is unchanged). **Superseded in part by D20:** the push direction (whole-snapshot `POST /catalogue`) and the generation-guard conflict policy of this entry are replaced by the annotation-delta model; the transport (`tiny_http`+`ureq`), the command location and the download/atomic-swap machinery still stand.

## D19. An idle re-sync is a no-op (local change marker)

Context: D18's first slice pushed whenever the client and server generations were level, whether or not anything had changed locally, so every startup sync bumped the server generation — noise, needless whole-file uploads and needless conflict windows. Local edits do not move the generation counter (D18, `docs/sync.md` section 2), so the client needs another way to tell "unchanged since I last synced". Rejected: setting a dirty flag from every local write (many call sites; forgetting one silently loses a push) and storing the marker in the catalogue itself (writing the marker changes the file, so a fingerprint of the file could never include it without being circular).

Decision: a **local-only change marker** beside the local copy.
- After every successful push or pull the client writes `<catalogue>.sync-state` (e.g. `photos.sqlite.sync-state`) holding `fingerprint=<value>`: a fast, non-cryptographic fingerprint (an FNV-1a-style hash over 8-byte words plus the length) of the local file's bytes at that moment. The marker is never transferred and never read by the server.
- At sync time, with the generations level, the client fingerprints its local file and compares it with the marker. Equal -> **up to date**: nothing is sent and the generation does not move. Different (or no marker at all) -> push, the behaviour of D18.
- The marker is best-effort: missing, unreadable or malformed means "unknown", which is always treated as *changed*, so the failure mode is a redundant push, never a skipped one. Any local writer — CLI, GUI, another tool — marks the copy changed simply by changing its bytes.
- Fingerprint scope: it detects local change only. It is never a security or integrity guarantee; integrity is still the snapshot verification in D18, and cross-client consistency still rests on the generation counter.

Status: Accepted. Implemented in `crates/icatalog-sync` (`util::fingerprint`/`read_state`/`write_state`, `SyncAction::UpToDate`); tests: `idle_resync_is_a_no_op`, `a_local_change_is_pushed`, and unit tests for the fingerprint and marker. **Repurposed by D20:** under annotation-delta push the marker answers "has this replica got unpushed edits?" rather than "should the whole file be pushed?"; the best-effort rule (unknown means changed) is unchanged.

## D20. Sync model: server-authoritative catalogue, annotation-delta push, previews pulled not pushed

Context: D18/D19 shipped a **bidirectional whole-snapshot** sync — a client uploads or downloads the entire SQLite file, guarded by a generation compare, with refuse-and-resync and a rejected-state backup on a lost race. Two owner requirements break that shape. (a) The real model is one writer, many readers: the NAS holds the catalogue and the media; clients (laptop, Surface) consume it — but the owner does want locally made annotations to reach the server (ratings/labels/keywords, never previews). (b) A second, standalone use case: a local catalogue of `~/Pictures`, not necessarily synced, that must be a **single movable file with 1080p previews inside it**. A whole-snapshot push is wrong for (a): a multi-gigabyte upload to save one star rating, and the coarse generation guard cannot merge — if the server moved at all, the client's edits are parked in a backup (`SyncAction::Resynced`) instead of applied.

Decision (owner, Oct 2026) — the target model:
1. **Push is an annotation delta, never a snapshot.** A client pushes only user annotations (rating, label, keyword membership) keyed by stable asset `uid`; never media, thumbnails or previews. The server stays authoritative for media-derived state.
2. **Pull is the catalogue.** Assets, EXIF/`metadata_raw`, annotations and grid thumbnails come from the server; the D18 download path (verify, back up, temp file, atomic swap) is unchanged.
3. **Previews travel server -> client only, content-addressed.** A preview is addressed by `sha256` and fetched on demand (`GET /preview/<sha>`); a client fetches only what it lacks. A preview is always derived from the media by the machine that holds the media, so a client never pushes one.
4. **The preview store is a per-catalogue choice, one format.** A preview row records its content hash; the bytes are either **embedded** in the catalogue file or **external** in `previews/<sha>.jpg` beside it. Embedded gives the standalone single-file portable catalogue; external gives the NAS catalogue (clients pull metadata + grid thumbs, previews arrive lazily). Grid thumbnails stay in the catalogue in both modes (they are the browsing index).
5. **A catalogue may be standalone or a replica.** A standalone catalogue simply never runs `sync`; no new mode flag is needed beyond the preview-store setting. The catalogue id and generation guard still apply when a catalogue is synced.
6. **Single-file portability is a constraint, not an accident.** The catalogue uses a rollback journal (no `PRAGMA journal_mode = WAL`), so there are no `-wal`/`-shm` sidecars to carry. If WAL is ever enabled, a checkpoint on close (or export via `VACUUM INTO`) must be added or "one file" silently becomes three.

Proposed (recommended here; the owner has not accepted the mechanism):
- **Conflict policy: per-item compare-and-swap.** A push carries `{uid, base_rev, rating, label, keywords}`; the server applies a value only where the asset's annotation revision still equals `base_rev`, and reports the rest as conflicts. This replaces the generation guard for annotations and keeps the "no silent last-writer-wins" guarantee per item. Needs a per-asset annotation revision (the next free migration, v6; v4 is D21 and v5 is device provenance). Open: whole-row versus per-field revision.
- **A client with unpushed edits** runs push-then-pull in one `sync`, so a pull never clobbers unpushed work; conflicts are reported. Alternative: refuse to pull while edits are pending.
- **A `previews --external|--embed` conversion command**, so a standalone catalogue can graduate to a replica and a server catalogue can be shipped as one portable file.
- **Auth:** a shared token (or none on a trusted LAN) on the write endpoint; the blast radius is now annotations, not the whole catalogue.

Open: preview tiers and quality (big tier long edge 1080 or 1920; grid 256 only, or add 512); whether the write endpoint survives at all or only a designated master pushes; how a push names a keyword the server does not yet have; whether preview generation is a per-catalogue default and how it is triggered (at ingest or by an explicit command).

Status: **Accepted as the target model (owner, Oct 2026); NOT IMPLEMENTED.** What is built today is D18/D19 (bidirectional whole-snapshot push). This entry fixes the direction and the open points; no code or schema has changed yet.

## D21. Source-rooted folders and device provenance

Context: the format study shows the legacy folder tree (`FSM!`) as a separate virtual hierarchy while every item carries its own native path — the item-to-folder linkage is still undecoded. The owner directed (Oct 2026) that iCatalog's folders should instead be based on **where assets were imported from**: a source may be a local disk, a CD/DVD/Blu-ray, an external HDD or a network share such as a NAS. Sets, albums and other groupings belong to a separate **Collections** concept, which can be manual or **Smart** (rules over metadata such as file type, dates or lens).

Decision:
1. A **source** row records `kind` (`local`, `optical`, `removable`, `network`), `label`, `identity`, `last_mount_path` and `last_seen_utc`. Identity is currently the **absolute import-root path**; `import --kind` declares the kind (default `local`) until volume identity is implemented.
2. `folder` gains `source_id` and `path` (POSIX-relative to the source root; `""` is the root folder), unique per `(source_id, path)`. `parent_id` still gives the tree.
3. `import` makes the import root the source root folder (named after the root) and mirrors the directory tree below it; each asset's `folder_id` points at the folder for its own directory. Re-importing the same root reuses the source and folders.
4. `update` files newly found assets into the same tree and refreshes moved assets' `folder_id`.
5. **Offline is not missing.** A source whose `last_mount_path` is absent (an ejected disc, an unplugged drive, a disconnected share) is skipped by `update`, reported as `offline`, and its assets are never marked `missing`.
6. **Collections stay separate** from folders: `collection` + `collection_asset` is many-to-many and will hold manual sets/albums; Smart Collections will store rule definitions (an `AssetQuery`-shaped predicate over format, capture date, lens, etc.). Neither is built yet.
7. **Device provenance (v5).** A source also records `device_id` (the volume or share root: `/` for the boot volume, `/Volumes/<name>`, `/mnt/<name>`, `/media/<name>`, a Windows drive prefix or UNC share) and `device_label` (`This Mac`, `NAS`, `DVD_01`, ...). Both are derived heuristically from `last_mount_path` with no filesystem call (`device_of`), refreshed by `ensure_source`/`touch_source`, and `Source::device()` falls back to the same derivation for pre-v5 rows. The UI groups source roots under one header per device, so an import from this Mac is visibly "This Mac" rather than an unqualified top-level folder. The header is an accordion: its arrow collapses the section, and clicking the device name selects every asset from every source on that device (`AssetQuery::source_ids`).

Rejected: mirroring the filesystem for every catalogued path with no source concept (unstable across mounts, and it cannot tell an offline disc from deleted files); and reusing `folder` as a many-to-many grouping (that is what collections are for). Also rejected for now: making the source the device itself with the chosen folder as a path inside it. The import root stays the source identity until stable volume identity exists, so a second import from the same device creates a second source under the same device header.

Status: **schema v4/v5 and the import/update wiring are implemented and tested** (`source` with `device_id`/`device_label`, `folder.source_id`, `folder.path`; `import --kind`; offline sources in `update`; the UI groups source roots by device). Open: volume-UUID/hardware identity (two Macs' boot volumes both derive `"/"`; a share remounted elsewhere looks like a new source), a `source`/`folder` command surface (rename, relink, forget), and Collections (manual and smart). Legacy `.ivc` import must map `FSM!` into the same tree or keep it as a separate unsourced hierarchy.

## D22. Native macOS menu bar via `muda` (2026-10-23)

Context: The owner asked to move the Import/Update actions out of the in-app egui menu bar and into a real OS menu. On macOS that means installing an `NSMenu` on `NSApp.mainMenu`. eframe/egui has no built-in native menu installation API in the version used; `rfd` covers file pickers but not menus.

Decision:
- Add `muda` (Apache-2.0/MIT) as a GUI dependency to `icatalog-ui` only. It is allowed for L4 per the layer rules (added to `GUI` list in `crates/icatalog-cli/tests/layers.rs`). No other crate may depend on it.
- On **macOS only**, build an `NSMenu` with an app menu (Quit) and a **File** menu containing **Import…** and **Update**, install it via `menu.init_for_nsapp()`, and forward clicks through `muda::MenuEvent` to the UI thread with `egui::Context::request_repaint()`. Menu item ids are `file.import` / `file.update`.
- Keep the existing in-app egui `MenuBar` as a fallback when the native menu cannot be installed or on non-macOS platforms.
- The actions remain engine commands (`import`/`update`) dispatched the same way as before; only the menu host changes.

Status: Accepted (macOS native menu installed; other platforms fall back to in-app menu).

## D23. Atomic engine commands and bounded desktop work (2026-10-24)

Decision: each engine command runs in a deferred SQLite transaction. Success commits; a returned error, escaped panic, or failed commit rolls back. This covers earlier writes in the same command (including keyword changes before rating validation). Import/update still report recoverable per-file skips in a successful outcome; a fatal command error rolls back the whole command. Sync keeps its separate backup/verify/swap protocol, not this transaction wrapper. Shell panic messages no longer claim that every previously completed action was undone.

Desktop import/update/edit run one at a time on a worker with its own engine context. The UI polls completion, shows a busy indicator, disables overlapping edits, and defers window close while work runs. Database refreshes wait until completion. Grid/list drawing uses visible rows; only visible and selected thumbnails stay cached, including failed/missing-thumbnail attempts to avoid retrying every frame. Catalogue refresh failures preserve prior results and report the error. Invalid catalogue startup returns failure.

Validation: regressions for panic-after-write, commit failure, invalid rating after keyword writes, startup/refresh errors, background outcomes, and viewport thumbnail bounds. `task scale` checks 10,000 generated PNGs through import/browse/search/edit/update/reopen/integrity validation, plus a separate 10,000-asset HTTP snapshot round trip. Synthetic sync thumbnail blobs test transport size, not decoding.

Limits: asset rows are still fetched together; visible thumbnail decoding stays on the UI thread; progress is a busy indicator, not per-file progress or cancellation. No new schema or sync protocol; D20, Collections, legacy import and wasm remain pending.

## D24. Automated UI verification (2026-10-24)

Use the matching `egui_kittest` 0.36.2 dev dependency to drive the real eframe App with pointer/key events and render deterministic image comparisons. Keep ordinary interactions in the workspace gate; GPU snapshots and native macOS smoke tests are separately named tasks. Tests generate their own catalogue/media and validate persistent results, not just button state. Screenshot baselines require explicit regeneration and visual review; CI never updates them.

Native coverage launches the production binary and uses macOS System Events through a Rust test driver for menu Update, picker cancellation and Command-Q. It runs on a logged-in Mac with explicit OS permissions, separate from headless PR CI. Current coverage does not include choosing a folder in the native picker or Windows/Linux native shells. Custom tiles, rating/label choices and text fields now expose accessible names. Native Import/Update menu items disable while a worker is running, matching the in-app fallback.

Status: local headless interactions, four visual baselines and native macOS smoke verified. GitHub workflows are configured; their hosted/self-hosted executions remain unverified until run there. No schema or command-engine changes.

## Open questions
Share-file mode: see the guides. (Samples licensing/commit strategy is resolved: D9.)

