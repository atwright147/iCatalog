CREATE TABLE catalogue (
  id TEXT PRIMARY KEY,
  generation INTEGER NOT NULL,
  created_utc TEXT NOT NULL,
  schema_version INTEGER NOT NULL
);
CREATE TABLE folder (
  id INTEGER PRIMARY KEY,
  parent_id INTEGER REFERENCES folder(id),
  name TEXT NOT NULL
);
CREATE TABLE asset (
  id INTEGER PRIMARY KEY,
  uid TEXT NOT NULL UNIQUE,
  folder_id INTEGER REFERENCES folder(id),
  path TEXT NOT NULL,
  path_raw BLOB,
  name TEXT NOT NULL,
  name_raw BLOB,
  format TEXT,
  file_size INTEGER,
  width INTEGER,
  height INTEGER,
  color_space INTEGER,
  mtime INTEGER,
  content_hash TEXT
);
CREATE TABLE thumbnail (
  asset_id INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  size_class INTEGER NOT NULL,
  source TEXT NOT NULL,
  format TEXT NOT NULL,
  width INTEGER NOT NULL,
  height INTEGER NOT NULL,
  orientation_applied INTEGER NOT NULL,
  bytes BLOB NOT NULL,
  derived_from TEXT,
  generator_version INTEGER NOT NULL,
  PRIMARY KEY (asset_id, size_class)
);
CREATE TABLE metadata_raw (
  asset_id INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  scheme TEXT NOT NULL,
  key TEXT NOT NULL,
  seq INTEGER NOT NULL,
  value BLOB NOT NULL,
  PRIMARY KEY (asset_id, scheme, key, seq)
);
CREATE TABLE asset_meta (
  asset_id INTEGER PRIMARY KEY REFERENCES asset(id) ON DELETE CASCADE,
  captured_local TEXT,
  tz_offset_min INTEGER,
  make TEXT, model TEXT, lens TEXT,
  focal_length_mm REAL, f_number REAL, exposure_s REAL, iso INTEGER,
  gps_lat REAL, gps_lon REAL,
  orientation INTEGER,
  duration_ms INTEGER
);
CREATE TABLE collection (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL
);
CREATE TABLE collection_asset (
  collection_id INTEGER NOT NULL REFERENCES collection(id) ON DELETE CASCADE,
  asset_id INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, asset_id)
);
