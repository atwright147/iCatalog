# iCatalog SQLite schema

Purpose: catalogue schema and migrations. Status vocabulary: **Decided** (owner decision or golden rule), **Proposed** (designed here), **Open** (unresolved). Migrations v1, v2, v3, v4 and v5 are **implemented** in `crates/icatalog-catalog` (EXISTS, tested); the Rust model and CRUD cover every table below.

## 1. Basis

**Decided:** the own format is SQLite (D7), via `rusqlite` with the `bundled` feature. Legacy `.ivc` is import-only. The initial schema derives from what `ivc` yields (EXISTS): per item `path`, `name`, `format` (4cc), `file_size`, `width`, `height`, `color_space`, `pict`, `jpeg`, `tail_blocks`, `iptc`; plus the `FSM!` folder tree chunk. Spec: `ivc-file-research/docs/ivc-format.md`.

## 2. Migrations philosophy

**Proposed:**

- Versioned, **forward-only**, each migration in a single transaction.
- A `schema_migrations` table records applied versions; it is created in code (not in a migration file) before migrations run.
- Migrations: `v1.sql` (catalogue, folder, asset, thumbnail, metadata_raw, asset_meta, collection, collection_asset) `v2.sql` (keyword, asset_keyword, annotation, `find` indexes), `v3.sql` (`asset.missing` + index, D14), `v4.sql` (`source`, `folder.source_id`/`folder.path`, D21) and `v5.sql` (`source.device_id`/`device_label`, D21). `foreign_keys` is ON for every connection.
- A catalogue with a newer schema version than the app knows is refused with an actionable error, never opened read-write.
- Every migration has a round-trip test on synthetic data; a never-panic test feeds corrupt or truncated files.
- Back up before migrating a user catalogue (**Open**: exact mechanism).

## 3. DDL (v1-v5, implemented; later columns are marked inline)

```sql
-- catalogue-level metadata (single row)
CREATE TABLE catalogue (
  id          TEXT PRIMARY KEY,        -- per-catalogue id (see sync.md)
  generation  INTEGER NOT NULL,        -- bumped on every accepted push
  created_utc TEXT NOT NULL,
  schema_version INTEGER NOT NULL
);

-- created in code by the migrator
CREATE TABLE schema_migrations (
  version     INTEGER PRIMARY KEY,
  applied_utc TEXT NOT NULL
);

-- v4 (D21): where assets were imported from
CREATE TABLE source (
  id              INTEGER PRIMARY KEY,
  kind            TEXT NOT NULL DEFAULT 'local',   -- local | optical | removable | network
  label           TEXT NOT NULL,                   -- import root's directory name
  identity        TEXT NOT NULL UNIQUE,            -- absolute import-root path (see D21)
  last_mount_path TEXT,                            -- where it was last seen
  last_seen_utc   TEXT,                            -- ISO-8601
  device_id       TEXT,                            -- v5: volume/share root, e.g. "/" or "/Volumes/NAS"
  device_label    TEXT                             -- v5: "This Mac", "NAS", ...
);

-- PROPOSED: folder tree (source-rooted for raster imports, D21; FSM! for legacy imports)
CREATE TABLE folder (
  id        INTEGER PRIMARY KEY,
  parent_id INTEGER REFERENCES folder(id),
  source_id INTEGER REFERENCES source(id) ON DELETE CASCADE,  -- v4
  path      TEXT,                                -- v4: POSIX path relative to the source root, "" = root
  name      TEXT NOT NULL
);

-- v4: one folder per (source, path)
CREATE UNIQUE INDEX idx_folder_source_path ON folder(source_id, path) WHERE source_id IS NOT NULL;

-- PROPOSED: one row per catalogued asset
CREATE TABLE asset (
  id          INTEGER PRIMARY KEY,
  uid         TEXT NOT NULL UNIQUE,    -- stable id; = content_hash for imported files (D13)
  folder_id   INTEGER REFERENCES folder(id),
  path        TEXT NOT NULL,           -- normalised POSIX, not unique (D10)
  path_raw    BLOB,                    -- original legacy bytes
  name        TEXT NOT NULL,
  name_raw    BLOB,
  format      TEXT,                    -- 4cc from legacy import
  file_size   INTEGER,
  width       INTEGER,
  height      INTEGER,
  color_space INTEGER,
  mtime       INTEGER,                 -- epoch seconds
  content_hash TEXT,
  missing     INTEGER NOT NULL DEFAULT 0  -- v3 (D14): 1 when the last rescan did not find the file
);

-- PROPOSED: canonical grid thumbnails (D8); the wasm client and sync cannot regenerate them
CREATE TABLE thumbnail (
  asset_id    INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  size_class  INTEGER NOT NULL,
  source      TEXT NOT NULL,   -- legacy_ivc_jfif | legacy_ivc_pict | exif_ifd1 | mpf_preview | raw_preview | psd_resource | rendered
  format      TEXT NOT NULL,   -- e.g. jpeg
  width       INTEGER NOT NULL,
  height      INTEGER NOT NULL,
  orientation_applied INTEGER NOT NULL,
  bytes       BLOB NOT NULL,
  derived_from TEXT,           -- "<file_size>:<mtime>" (D13)
  generator_version INTEGER NOT NULL,
  PRIMARY KEY (asset_id, size_class)
);

-- PROPOSED: raw-first metadata (D8)
CREATE TABLE metadata_raw (
  asset_id INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  scheme   TEXT NOT NULL,      -- iptc_iim | exif_tiff | xmp_packet | ivc_tail
  key      TEXT NOT NULL,      -- e.g. IPTC "record:dataset"; ex?? block tag
  seq      INTEGER NOT NULL,
  value    BLOB NOT NULL,
  PRIMARY KEY (asset_id, scheme, key, seq)
);

-- PROPOSED: normalised 1:1 side table derived from metadata_raw
CREATE TABLE asset_meta (
  asset_id       INTEGER PRIMARY KEY REFERENCES asset(id) ON DELETE CASCADE,
  captured_local TEXT,         -- capture date/time
  tz_offset_min  INTEGER,
  make TEXT, model TEXT, lens TEXT,
  focal_length_mm REAL, f_number REAL, exposure_s REAL, iso INTEGER,
  gps_lat REAL, gps_lon REAL,
  orientation    INTEGER,
  duration_ms    INTEGER       -- video
);

-- PROPOSED: collections / albums
CREATE TABLE collection (
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL
);
CREATE TABLE collection_asset (
  collection_id INTEGER NOT NULL REFERENCES collection(id) ON DELETE CASCADE,
  asset_id      INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, asset_id)
);

-- v2: user annotations (D11)
CREATE TABLE keyword (
  id       INTEGER PRIMARY KEY,
  name     TEXT NOT NULL UNIQUE,
  name_raw BLOB
);
CREATE TABLE asset_keyword (
  asset_id   INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  keyword_id INTEGER NOT NULL REFERENCES keyword(id) ON DELETE CASCADE,
  PRIMARY KEY (asset_id, keyword_id)
);
CREATE TABLE annotation (
  asset_id INTEGER PRIMARY KEY REFERENCES asset(id) ON DELETE CASCADE,
  rating   INTEGER NOT NULL DEFAULT 0 CHECK (rating >= 0 AND rating <= 5),
  label    INTEGER NOT NULL DEFAULT 0 CHECK (label >= 0 AND label <= 7),
  caption TEXT, headline TEXT, creator TEXT, copyright TEXT,
  updated_utc TEXT
);

-- v2: indexes for browse/find
CREATE INDEX idx_asset_folder ON asset(folder_id);
CREATE INDEX idx_asset_name ON asset(name);
CREATE INDEX idx_asset_path ON asset(path);
CREATE INDEX idx_asset_content_hash ON asset(content_hash);
CREATE INDEX idx_folder_parent ON folder(parent_id);
CREATE INDEX idx_asset_keyword_keyword ON asset_keyword(keyword_id);
CREATE INDEX idx_annotation_rating ON annotation(rating);
CREATE INDEX idx_annotation_label ON annotation(label);
```

Notes (**Proposed**, per D8):

- **Thumbnail sources:** `legacy_ivc_jfif`, `legacy_ivc_pict`, `exif_ifd1`, `mpf_preview`, `raw_preview`, `psd_resource`, `rendered`. Legacy embedded JPEGs are stored unchanged, never re-encoded.
- **`metadata_raw` rows:** EXIF as one blob, XMP as one packet, IPTC one row per field, `ex??` one row per block. Legacy `tail_blocks` go to scheme `ivc_tail`.
- **Legacy `pict`:** raw bytes are retained only for items with no extracted JPEG, so the PICT decoder (in `ivc`) can fill the thumbnail later.
- **User annotations** (keywords, rating, label, caption) are authoritative and stored separately from extracted metadata. Merge/display order: EXIF, then IPTC fills gaps, then XMP overrides, then the user's edit wins.
- **Sources and folders (v4, D21):** `import` makes the import root a `source` (`identity` = its absolute path) and mirrors the directory tree below it into `folder` rows keyed by `(source_id, path)`. Each asset's `folder_id` points at the folder for its own directory. `--kind` declares `optical`/`removable`/`network`; detection by volume identity is **Open**. `update` treats a source whose `last_mount_path` is absent as **offline**: it neither scans it nor marks its assets missing.
- **Device provenance (v5, D21):** `source.device_id`/`source.device_label` record the volume or share root and a human label, derived from `last_mount_path` by `device_of` (`/` → `This Mac`, `/Volumes/NAS` → `NAS`, `/mnt/*` and `/media/*` likewise). Both are refreshed on import and update; pre-v5 rows are derived on read by `Source::device()`. This is a display/provenance key, not yet stable hardware identity.

User-annotation tables (D11) and `find` indexes are done (migration v2). Still **Open**: where unknown chunks go.

## 4. Open items

- **Encoding:** resolved by D10 (decoded text plus raw bytes). Mac Roman detection remains an `ivc` RE item.
- **Path normalisation:** separators (legacy Mac paths versus POSIX/Windows), case, Unicode normalisation, and the rule that a catalogued path is never trusted to stay inside a directory (**Decided** in golden rules; representation **Open**).
- Stable asset ids across machines (needed for merge, see `docs/sync.md`).
- **Source identity:** currently the absolute import-root path, so a NAS share remounted elsewhere or a drive that changes mount point looks like a new source. Volume UUID/label detection is **Open**.
- `size_class` (long edge in pixels, grid = 256), `source` (`"rendered"`), `derived_from` (`"<file_size>:<mtime>"`), `orientation_applied` (false until pixels are rotated) and `content_hash` (`"sha256:<hex>"`, also the `asset.uid`) are **RESOLVED per D13**. Raw-versus-decoded metadata storage is answered by D8.

## 5. Test requirements

**Decided:** synthetic and round-trip tests for the catalogue; public-domain images only. **Proposed:** import tests built from the `ivc` sample oracles; migration tests from every prior version; hostile-input tests (truncated or mutated bytes into import must return `Err`).
