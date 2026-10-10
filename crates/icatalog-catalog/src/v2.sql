CREATE TABLE keyword (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  name_raw BLOB
);
CREATE TABLE asset_keyword (
  asset_id INTEGER NOT NULL REFERENCES asset(id) ON DELETE CASCADE,
  keyword_id INTEGER NOT NULL REFERENCES keyword(id) ON DELETE CASCADE,
  PRIMARY KEY (asset_id, keyword_id)
);
CREATE TABLE annotation (
  asset_id INTEGER PRIMARY KEY REFERENCES asset(id) ON DELETE CASCADE,
  rating INTEGER NOT NULL DEFAULT 0 CHECK (rating >= 0 AND rating <= 5),
  label INTEGER NOT NULL DEFAULT 0 CHECK (label >= 0 AND label <= 7),
  caption TEXT,
  headline TEXT,
  creator TEXT,
  copyright TEXT,
  updated_utc TEXT
);
CREATE INDEX idx_asset_folder ON asset(folder_id);
CREATE INDEX idx_asset_name ON asset(name);
CREATE INDEX idx_asset_path ON asset(path);
CREATE INDEX idx_asset_content_hash ON asset(content_hash);
CREATE INDEX idx_folder_parent ON folder(parent_id);
CREATE INDEX idx_asset_keyword_keyword ON asset_keyword(keyword_id);
CREATE INDEX idx_annotation_rating ON annotation(rating);
CREATE INDEX idx_annotation_label ON annotation(label);
