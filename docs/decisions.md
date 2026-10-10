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

## Open questions
Share-file mode: see the guides. (Samples licensing/commit strategy is resolved: D9.)
