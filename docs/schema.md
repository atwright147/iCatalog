# iCatalog SQLite schema

Purpose: catalogue schema and migrations. Status vocabulary: **Decided** (owner decision or golden rule), **Proposed** (designed here), **Open** (unresolved). Migrations v1 and v2 are **implemented** in `crates/icatalog-catalog` (EXISTS, tested); the Rust model and CRUD cover every table below.

## 1. Basis

**Decided:** the own format is SQLite (D7), via `rusqlite` with the `bundled` feature. Legacy `.ivc` is import-only. The initial schema derives from what `ivc` yields (EXISTS): per item `path`, `name`, `format` (4cc), `file_size`, `width`, `height`, `color_space`, `pict`, `jpeg`, `tail_blocks`, `iptc`; plus the `FSM!` folder tree chunk. Spec: `ivc-file-research/docs/ivc-format.md`.

## 2. Migrations philosophy

**Proposed:**

- Versioned, **forward-only**, each migration in a single transaction.
- A `schema_migrations` table records applied versions; it is created in code (not in a migration file) before migrations run.
- Migrations: `v1.sql` (catalogue, folder, asset, thumbnail, metadata_raw, asset_meta, collection, collection_asset) and `v2.sql` (keyword, asset_keyword, annotation, `find` indexes). `foreign_keys` is ON for every connection.
- A catalogue with a newer schema version than the app knows is refused with an actionable error, never opened read-write.
- Every migration has a round-trip test on synthetic data; a never-panic test feeds corrupt or truncated files.
- Back up before migrating a user catalogue (**Open**: exact mechanism).

## 3. DDL (v1 + v2, implemented)

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

-- PROPOSED: folder tree (from FSM! for imports)
CREATE TABLE folder (
  id        INTEGER PRIMARY KEY,
  parent_id INTEGER REFERENCES folder(id),
  name      TEXT NOT NULL
);

-- PROPOSED: one row per catalogued asset
CREATE TABLE asset (
  id          INTEGER PRIMARY KEY,
  uid         TEXT NOT NULL UNIQUE,    -- stable id for future merge (Open)
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
  content_hash TEXT
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
  derived_from TEXT,           -- source size/mtime or hash
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

User-annotation tables (D11) and `find` indexes are done (migration v2). Still **Open**: where unknown chunks go.

## 4. Open items

- **Encoding:** resolved by D10 (decoded text plus raw bytes). Mac Roman detection remains an `ivc` RE item.
- **Path normalisation:** separators (legacy Mac paths versus POSIX/Windows), case, Unicode normalisation, and the rule that a catalogued path is never trusted to stay inside a directory (**Decided** in golden rules; representation **Open**).
- Stable asset ids across machines (needed for merge, see `docs/sync.md`).
- Exact `size_class` values, `derived_from` form (size/mtime versus hash) and `orientation_applied` semantics (**Open**). Raw-versus-decoded metadata storage is answered by D8.

## 5. Test requirements

**Decided:** synthetic and round-trip tests for the catalogue; public-domain images only. **Proposed:** import tests built from the `ivc` sample oracles; migration tests from every prior version; hostile-input tests (truncated or mutated bytes into import must return `Err`).
