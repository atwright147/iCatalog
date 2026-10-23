-- v5 (D21): device provenance for a source. The import root names the source
-- folder; `device_*` says which medium it came from (this Mac, a disc, a drive
-- or a share), so the UI can group sources by device.

ALTER TABLE source ADD COLUMN device_id TEXT;
ALTER TABLE source ADD COLUMN device_label TEXT;

CREATE INDEX idx_source_device ON source(device_id);
