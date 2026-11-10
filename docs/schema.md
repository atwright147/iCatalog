# iCatalog SQLite schema

Purpose: catalogue schema and migrations. Status vocabulary: **Decided** (owner decision or golden rule), **Proposed** (designed here), **Open** (unresolved). Migrations v1 through v8 are **implemented** in `crates/icatalog-catalog` (EXISTS, tested); the Rust model and CRUD cover every table below.

## 1. Basis

**Decided:** the own format is SQLite (D7), via `rusqlite` with the `bundled` feature. Legacy `.ivc` is import-only. The initial schema derives from what `ivc` yields (EXISTS): per item `path`, `name`, `format` (4cc), `file_size`, `width`, `height`, `color_space`, `pict`, `jpeg`, `tail_blocks`, `iptc`; plus the `FSM!` folder tree chunk. Spec: `ivc-file-research/docs/ivc-format.md`.

## 2. Migrations philosophy

**Proposed:**

- Versioned, **forward-only**, each migration in a single transaction.
- A `schema_migrations` table records applied versions; it is created in code (not in a migration file) before migrations run.
- Migrations: `v1.sql` (catalogue, folder, asset, thumbnail, metadata_raw, asset_meta, collection, collection_asset) `v2.sql` (keyword, asset_keyword, annotation, `find` indexes), `v3.sql` (`asset.missing` + index, D14), `v4.sql` (`source`, `folder.source_id`/`folder.path`, D21), `v5.sql` (`source.device_id`/`device_label`, D21), `v6.sql` (`smart_collection` typed rules and membership guards, D35), `v7.sql` (stable Collection UIDs, transactional revisions/generations, replica baseline and previews, D20/D44), and `v8.sql` (inclusive maximum rating and ordered rating bounds, D57). `foreign_keys` is ON for every connection.
- A catalogue with a newer schema version than the app knows is refused with an actionable error, never opened read-write.
- Every migration has a round-trip test on synthetic data; a never-panic test feeds corrupt or truncated files.
- Back up before migrating a user catalogue (**Open**: exact mechanism).

Engine command writes are grouped in a single deferred transaction (D23), with rollback on error, panic or failed commit. This does not change the schema version. Direct catalogue CRUD callers manage their own transaction boundaries.

## 3. DDL (v1-v6, implemented; later columns are marked inline)

```sql
-- catalogue-level metadata (single row)
CREATE TABLE catalogue (
  id          TEXT PRIMARY KEY,        -- 1–1024 printable ASCII bytes, no edge spaces (see sync.md)
  generation  INTEGER NOT NULL,        -- advanced transactionally on catalogue-data writes
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
  uid         TEXT NOT NULL UNIQUE,    -- stable id; initially content_hash, with suffix on historical collision (D66)
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
  mtime       INTEGER,                 -- Unix epoch seconds (UTC instant)
  content_hash TEXT,
  missing     INTEGER NOT NULL DEFAULT 0  -- v3 (D14/D77): 1 when a rescan confirmed the file absent
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
  captured_local TEXT,         -- ISO Gregorian camera-local wall-clock date/time
  tz_offset_min  INTEGER,
  make TEXT, model TEXT, lens TEXT,
  focal_length_mm REAL, f_number REAL, exposure_s REAL, iso INTEGER,
  gps_lat REAL, gps_lon REAL,
  orientation    INTEGER,
  duration_ms    INTEGER       -- video
);

-- Manual and Smart Collections (D25/D35); smart rules are stored separately in v6
CREATE TABLE collection (
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL
);
CREATE TABLE collection_asset (
  collection_id INTEGER NOT NULL REFERENCES collection(id) ON DELETE CASCADE,
  asset_id      INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, asset_id)
);

-- v6 introduced Smart Collections; this is the v8 rules table (D57).
-- Presence marks a Smart Collection; NULL fields add no restriction.
CREATE TABLE smart_collection (
  collection_id INTEGER PRIMARY KEY REFERENCES collection(id) ON DELETE CASCADE,
  format TEXT, captured_from TEXT, captured_to TEXT,
  camera TEXT, lens TEXT,
  min_rating INTEGER CHECK (min_rating BETWEEN 0 AND 5),
  max_rating INTEGER CHECK (max_rating IS NULL OR
    (typeof(max_rating)='integer' AND max_rating BETWEEN 0 AND 5)),
  label INTEGER CHECK (label BETWEEN 0 AND 7),
  keyword TEXT, text TEXT,
  CHECK (format IS NOT NULL OR captured_from IS NOT NULL OR captured_to IS NOT NULL
         OR camera IS NOT NULL OR lens IS NOT NULL OR min_rating IS NOT NULL
         OR max_rating IS NOT NULL OR label IS NOT NULL OR keyword IS NOT NULL OR text IS NOT NULL),
  CHECK (min_rating IS NULL OR max_rating IS NULL OR min_rating <= max_rating)
);
-- Triggers reject manual membership insert/update for smart collections and
-- adding rules to collections that already hold manual members.

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

`AssetQuery` and `find` treat an absent annotation row as rating 0 and label 0. A minimum rating of 0 includes every asset; an exact label of 0 includes both explicitly unlabelled assets and assets without annotations. Smart Collection rules use the same zero defaults.

### v8 additions (D57, implemented)

`smart_collection.max_rating` is optional and inclusive. The at-least-one-rule constraint includes it, allowing maximum-only rules; a cross-field constraint rejects inverted bounds. Because SQLite cannot replace those table constraints in place, v8 rebuilds the rules table inside the migration transaction, copies every existing rule with NULL maximum, and recreates its membership/revision/generation triggers. Collection UIDs, revisions, manual membership and replica baseline bytes remain unchanged. The schema update advances catalogue generation once. Failed migration rolls back the entire rebuild and can be retried; reopening an upgraded catalogue is idempotent.

### v7 additions (D20/D44, implemented)

`collection.uid TEXT` has a unique index. The migration fills existing rows as `legacy:<catalogue-id>:<numeric-id>`; new rows receive random `collection:<128-bit hex>` UIDs through a trigger. A non-null UID cannot change. Numeric IDs, names, rules and memberships survive migration.

```sql
CREATE TABLE sync_revision (
  kind TEXT NOT NULL CHECK(kind IN ('asset','collection')),
  entity_id INTEGER NOT NULL,
  revision INTEGER NOT NULL DEFAULT 0
    CHECK(typeof(revision)='integer' AND revision>=0),
  PRIMARY KEY(kind,entity_id)
);
CREATE TABLE replica_base (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  generation INTEGER NOT NULL CHECK(generation>=0),
  data BLOB NOT NULL CHECK(length(data)<=67108864)
);
CREATE TABLE preview_setting (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  mode TEXT NOT NULL CHECK(mode IN ('embedded','external'))
);
CREATE TABLE preview_blob (
  hash TEXT PRIMARY KEY CHECK(length(hash)=64 AND hash NOT GLOB '*[^0-9a-f]*'),
  bytes BLOB CHECK(bytes IS NULL OR length(bytes)<=16777216)
);
CREATE TABLE preview (
  asset_id INTEGER PRIMARY KEY REFERENCES asset(id) ON DELETE CASCADE,
  hash TEXT NOT NULL REFERENCES preview_blob(hash),
  width INTEGER NOT NULL CHECK(width>0 AND width<=1920),
  height INTEGER NOT NULL CHECK(height>0 AND height<=1920),
  source_hash TEXT NOT NULL
);
```

Every asset and Collection has a revision row. Triggers track rating/label and keyword-membership/name changes per asset; name, Smart rules and manual membership per Collection. Deleting an entity removes its revision; missing entities conflict during sync. Revisions cover whole editable entities rather than individual fields. Catalogue-data writes advance generation transactionally, including direct CRUD; internal revision/baseline bookkeeping is excluded. Type/range guards abort generation or revision overflow and roll back the triggering write. Schema-version changes also advance generation.

`replica_base` contains the last accepted server generation and a bounded binary baseline of user-editable values/revisions (codec in `icatalog-sync::delta`). It is established in a verified download before replacement, or before initial publication by `clone` (D45). Cloned replicas can immediately upload their first user edits; only manually seeded/untracked copies need the backup-first initial sync. Its presence identifies a replica; no manual mode flag or baseline sidecar is needed. Triggers reject media/metadata/source/folder/preview-setting writes on replicas. Rating/label edits update an existing annotation row while preserving any server-owned caption, headline, creator and copyright; inserting or changing that text on a replica remains forbidden. Collection edits remain available, but deletion is unsupported there. This baseline replaces D19 fingerprint markers, which are ignored on upgrade. First v2 sync backs up the previous local state and establishes the server baseline without uploading unknown local edits.

Previews default to embedded bytes. External mode records the same hash and reference but stores bytes in `previews/<sha>.jpg` beside the catalogue. JPEG generation is explicit through the engine, using a 1920-pixel maximum long edge and existing media decoding/orientation. Source content-hash changes remove stale references. Generate and conversion commands remove unreferenced preview blobs transactionally, retaining blobs shared by another asset (D67). Conversion verifies all referenced bytes before committing; external files are retained. Grid thumbnails keep their existing embedded representation. Generation/cache reads and writes are bounded; SHA-256 verification and filesystem storage live in the engine/media layers. Full protocol, safety and CLI behavior: `docs/sync.md`.

Notes (**Proposed**, per D8):

- **Thumbnail sources:** `legacy_ivc_jfif`, `legacy_ivc_pict`, `exif_ifd1`, `mpf_preview`, `raw_preview`, `psd_resource`, `rendered`. Legacy embedded JPEGs are stored unchanged, never re-encoded.
- **`metadata_raw` rows:** EXIF as one blob, XMP as one packet, IPTC one row per field, `ex??` one row per block. Legacy `tail_blocks` go to scheme `ivc_tail`. Changed-media Update replaces the `xmp_packet` scheme from the new source bytes and removes it when the replacement has no XMP (D80); user annotations remain separate.
- **Source colour display data (D32, EXISTS):** `metadata_raw` scheme `image_info`, keys `color_model` and `icc_profile`, `seq=0`, contain UTF-8 model/profile descriptions from supported source image headers/ICC data. Empty values mean unavailable; both rows are replaced together during import/changed-file Update, so lost profiles clear. Update fills absent rows in older entries, retaining unchanged thumbnail bytes and asset identity. RAW entries store empty values rather than the embedded preview's colour. The existing integer `asset.color_space` is unchanged. No migration or whole ICC blob is needed; `metadata_raw_for_asset_scheme` reads one scheme without loading unrelated binary packets.
- **Legacy `pict`:** raw bytes are retained only for items with no extracted JPEG, so the PICT decoder (in `ivc`) can fill the thumbnail later.
- **User annotations** (keywords, rating, label, caption) are authoritative and stored separately from extracted metadata. Merge/display order: EXIF, then IPTC fills gaps, then XMP overrides, then the user's edit wins.
- **Sources and folders (v4, D21):** `import` makes the import root a `source` (`identity` = its absolute path) and mirrors the directory tree below it into `folder` rows keyed by `(source_id, path)`. Each asset's `folder_id` points at the folder for its own directory. `--kind` declares `optical`/`removable`/`network`; detection by volume identity is **Open**. `update` treats a source whose `last_mount_path` is absent as **offline**: it neither scans it nor marks its assets missing.
- **Device provenance (v5, D21):** `source.device_id`/`source.device_label` record the volume or share root and a human label, derived from `last_mount_path` by `device_of` (`/` → `This Mac`, `/Volumes/NAS` → `NAS`, `/mnt/*` and `/media/*` likewise). Both are refreshed on import and update; pre-v5 rows are derived on read by `Source::device()`. This is a display/provenance key, not yet stable hardware identity.

User-annotation tables (D11) and `find` indexes are done (migration v2). Still **Open**: where unknown chunks go.

Manual Collections (D25) use the existing v1 tables without a migration. Collection ids identify albums; display names may repeat. Membership is many-to-many, adding/removing a member is idempotent, and removing membership preserves the asset and its source/folder. `AssetQuery::collection_id` composes with other filters and paging. `collection_ids` (D28) matches membership in any listed collection using EXISTS, so overlapping albums produce no duplicate assets before paging. An empty list matches nothing; an absent list adds no restriction. Both fields, when set, are ANDed, as with all other filters. To bound query allocations and SQLite parameters, the list accepts at most 32,000 ids; larger lists return an InvalidQuery error. This is a query API change without a migration. `all_collections` orders by name then id; `asset_collection_ids` returns membership for a selected asset. Smart Collection rules are stored in v6 (D35); the same queries evaluate them live, including counts and mixed collection unions.

`SmartRules` is a bounded, typed AND predicate, not stored SQL or a recursive query tree. A collection with no `smart_collection` row is manual. Rules support exact ASCII-case-insensitive format, inclusive local capture dates, camera make/model or lens substring, inclusive minimum/maximum rating, exact label, exact keyword and filename/path substring. Missing metadata cannot satisfy its rule; missing annotations have rating/label zero. Real calendar dates and ordered ranges are validated in Rust, with 1024-byte/control-character limits on rule text. `Collection.rules`, membership reads, counts and `AssetQuery` all use live SQL results; no membership is materialized. Either rating bound may stand alone; 0 includes unrated assets and equal bounds select an exact rating. Bounds must be integers in 0–5 and minimum cannot exceed maximum. Smart create/rule-replacement uses the engine transaction, preserving the previous rules on failure. The v6 migration preserves existing asset identities/manual membership and updates the catalogue's schema version; D20/D44 revisions and preview storage are implemented in v7.

`Catalog::count_smart_matches` (D37) validates unsaved `SmartRules` and binds them as one CTE row, reusing the saved membership predicate for a single COUNT(*) query. It allocates no asset result set and writes no collection or rule rows. Preview counts have the same missing-data, rating/label-zero and AND semantics as saved collections. No migration.

`AssetQuery::folder_recursive` (D43) optionally includes all descendants of `folder_id` using `folder.parent_id`. The recursive SQL CTE deduplicates folder ids with `UNION`, so cyclic links terminate and assets appear once. Filtering happens before paging and composes with other predicates; names, path prefixes and UI expansion state do not determine membership. The default remains exact-folder matching. Unassigned (`folder_id_null = Some(true)`) takes precedence; recursion without a folder id has no effect. The desktop enables recursion for folder browsing. No migration or catalogue write.

`rename_collection` (D36) updates only the name of an existing collection, regardless of kind; ids, members and smart rules are unchanged. The engine `collection rename` action applies the creation-name validation and atomic transaction. Duplicate names remain allowed. No schema migration is required.

`collection set-rules` may also rename the Smart Collection (D38): optional `--name` uses the existing validation and updates `collection.name` in the same engine transaction as `smart_collection` replacement. Any error rolls both back. Omitted names preserve the existing value, and ids/asset identity remain stable. No migration.

## 4. Open items

- **Encoding:** resolved by D10 (decoded text plus raw bytes). Mac Roman detection remains an `ivc` RE item.
- **Path normalisation:** separators (legacy Mac paths versus POSIX/Windows), case, Unicode normalisation, and the rule that a catalogued path is never trusted to stay inside a directory (**Decided** in golden rules; representation **Open**).
- Stable asset UIDs are used by D20 sync; detecting moved source files remains open.
- **Source identity:** currently the absolute import-root path, so a NAS share remounted elsewhere or a drive that changes mount point looks like a new source. Volume UUID/label detection is **Open**.
- `size_class` (long edge in pixels, grid = 256), `source` (`"rendered"`), `derived_from` (`"<file_size>:<mtime>"`) and `content_hash` (`"sha256:<hex>"`) are **RESOLVED per D13**. The initial `asset.uid` is that hash unless an older asset retains it after changing content; then the new UID receives a numeric suffix (D66). Since D31, generator version 2 sets `orientation_applied` true when a valid EXIF orientation (1–8, including identity) has been applied to the thumbnail pixels. Thumbnail width/height describe the oriented pixels; asset dimensions and `asset_meta.orientation` retain source values. Missing/invalid orientation leaves pixels unchanged and the flag false. Older false-flag thumbnails use stored asset orientation at display time without a migration or rewrite. Raw-versus-decoded metadata storage is answered by D8.

## 5. Test requirements

**Decided:** synthetic and round-trip tests for the catalogue; public-domain images only. **Proposed:** import tests built from the `ivc` sample oracles; migration tests from every prior version; hostile-input tests (truncated or mutated bytes into import must return `Err`).

D46 desktop sync uses the same v7 replica baseline and L3 commands without a migration. Remembered desktop server addresses live outside SQLite in optional local `<catalogue>.sync-url` preferences; they are not catalogue data or uploaded state.
