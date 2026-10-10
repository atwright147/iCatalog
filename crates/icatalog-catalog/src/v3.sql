ALTER TABLE asset ADD COLUMN missing INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_asset_missing ON asset(missing);
