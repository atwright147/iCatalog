-- v4 (D21): source-rooted folders. A source is where assets were imported
-- from (a local directory, optical disc, external drive or network share).
-- Folders below a source mirror the directory tree under the import root;
-- `path` is POSIX-relative to the source root and "" is the root itself.

CREATE TABLE source (
  id              INTEGER PRIMARY KEY,
  kind            TEXT NOT NULL DEFAULT 'local', -- local | optical | removable | network
  label           TEXT NOT NULL,
  identity        TEXT NOT NULL UNIQUE,          -- canonical import-root path (see D21)
  last_mount_path TEXT,
  last_seen_utc   TEXT
);

ALTER TABLE folder ADD COLUMN source_id INTEGER REFERENCES source(id) ON DELETE CASCADE;
ALTER TABLE folder ADD COLUMN path TEXT;

CREATE INDEX idx_folder_source ON folder(source_id);
CREATE UNIQUE INDEX idx_folder_source_path ON folder(source_id, path) WHERE source_id IS NOT NULL;
