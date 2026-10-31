-- D20: stable collection identity, revisions, replica base and preview storage.
ALTER TABLE collection ADD COLUMN uid TEXT;
UPDATE collection SET uid='legacy:' || COALESCE((SELECT id FROM catalogue LIMIT 1),'uninitialised') || ':' || id;
CREATE UNIQUE INDEX idx_collection_uid ON collection(uid);
CREATE TABLE sync_revision (
  kind TEXT NOT NULL CHECK(kind IN ('asset','collection')),
  entity_id INTEGER NOT NULL,
  revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision)='integer' AND revision>=0),
  PRIMARY KEY(kind,entity_id)
);
INSERT INTO sync_revision SELECT 'asset',id,0 FROM asset;
INSERT INTO sync_revision SELECT 'collection',id,0 FROM collection;
CREATE TABLE replica_base (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  generation INTEGER NOT NULL CHECK(generation>=0),
  data BLOB NOT NULL CHECK(length(data)<=67108864)
);
CREATE TABLE preview_setting (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  mode TEXT NOT NULL CHECK(mode IN ('embedded','external'))
);
INSERT INTO preview_setting VALUES(1,'embedded');
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
CREATE TRIGGER generation_bounds BEFORE UPDATE OF generation ON catalogue
WHEN typeof(NEW.generation)!='integer' OR NEW.generation<0
BEGIN SELECT RAISE(ABORT,'catalogue generation exhausted or invalid'); END;
CREATE TRIGGER asset_sync_insert AFTER INSERT ON asset BEGIN
  INSERT INTO sync_revision(kind,entity_id) VALUES('asset',NEW.id);
END;
CREATE TRIGGER asset_sync_delete AFTER DELETE ON asset BEGIN
  DELETE FROM sync_revision WHERE kind='asset' AND entity_id=OLD.id;
END;
CREATE TRIGGER collection_sync_insert AFTER INSERT ON collection BEGIN
  UPDATE collection SET uid='collection:' || lower(hex(randomblob(16))) WHERE id=NEW.id AND uid IS NULL;
  INSERT INTO sync_revision(kind,entity_id) VALUES('collection',NEW.id);
END;
CREATE TRIGGER collection_uid_immutable BEFORE UPDATE OF uid ON collection
WHEN OLD.uid IS NOT NULL AND NEW.uid IS NOT OLD.uid
BEGIN SELECT RAISE(ABORT,'collection uid is immutable'); END;
CREATE TRIGGER collection_sync_delete AFTER DELETE ON collection BEGIN
  DELETE FROM sync_revision WHERE kind='collection' AND entity_id=OLD.id;
END;
CREATE TRIGGER collection_name_revision AFTER UPDATE OF name ON collection WHEN NEW.name IS NOT OLD.name BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=NEW.id;
END;
CREATE TRIGGER annotation_revision_insert AFTER INSERT ON annotation BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='asset' AND entity_id=NEW.asset_id;
END;
CREATE TRIGGER annotation_revision_update AFTER UPDATE ON annotation
WHEN NEW.rating IS NOT OLD.rating OR NEW.label IS NOT OLD.label BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='asset' AND entity_id=NEW.asset_id;
END;
CREATE TRIGGER annotation_revision_delete AFTER DELETE ON annotation BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='asset' AND entity_id=OLD.asset_id;
END;
CREATE TRIGGER keyword_name_revision AFTER UPDATE OF name ON keyword WHEN NEW.name IS NOT OLD.name BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='asset' AND entity_id IN
  (SELECT asset_id FROM asset_keyword WHERE keyword_id=NEW.id);
END;
CREATE TRIGGER asset_keyword_revision_insert AFTER INSERT ON asset_keyword BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='asset' AND entity_id=NEW.asset_id;
END;
CREATE TRIGGER asset_keyword_revision_update AFTER UPDATE ON asset_keyword BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='asset' AND entity_id=NEW.asset_id;
  UPDATE sync_revision SET revision=revision+1 WHERE kind='asset' AND entity_id=OLD.asset_id AND OLD.asset_id!=NEW.asset_id;
END;
CREATE TRIGGER asset_keyword_revision_delete AFTER DELETE ON asset_keyword BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='asset' AND entity_id=OLD.asset_id;
END;
CREATE TRIGGER collection_asset_revision_insert AFTER INSERT ON collection_asset BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=NEW.collection_id;
END;
CREATE TRIGGER collection_asset_revision_update AFTER UPDATE ON collection_asset BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=NEW.collection_id;
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=OLD.collection_id AND OLD.collection_id!=NEW.collection_id;
END;
CREATE TRIGGER collection_asset_revision_delete AFTER DELETE ON collection_asset BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=OLD.collection_id;
END;
CREATE TRIGGER smart_collection_revision_insert AFTER INSERT ON smart_collection BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=NEW.collection_id;
END;
CREATE TRIGGER smart_collection_revision_update AFTER UPDATE ON smart_collection BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=NEW.collection_id;
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=OLD.collection_id AND OLD.collection_id!=NEW.collection_id;
END;
CREATE TRIGGER smart_collection_revision_delete AFTER DELETE ON smart_collection BEGIN
  UPDATE sync_revision SET revision=revision+1 WHERE kind='collection' AND entity_id=OLD.collection_id;
END;
CREATE TRIGGER asset_generation_insert AFTER INSERT ON asset BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER asset_generation_update AFTER UPDATE ON asset BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER asset_generation_delete AFTER DELETE ON asset BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER folder_generation_insert AFTER INSERT ON folder BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER folder_generation_update AFTER UPDATE ON folder BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER folder_generation_delete AFTER DELETE ON folder BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER source_generation_insert AFTER INSERT ON source BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER source_generation_update AFTER UPDATE ON source BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER source_generation_delete AFTER DELETE ON source BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER thumbnail_generation_insert AFTER INSERT ON thumbnail BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER thumbnail_generation_update AFTER UPDATE ON thumbnail BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER thumbnail_generation_delete AFTER DELETE ON thumbnail BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER metadata_raw_generation_insert AFTER INSERT ON metadata_raw BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER metadata_raw_generation_update AFTER UPDATE ON metadata_raw BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER metadata_raw_generation_delete AFTER DELETE ON metadata_raw BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER asset_meta_generation_insert AFTER INSERT ON asset_meta BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER asset_meta_generation_update AFTER UPDATE ON asset_meta BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER asset_meta_generation_delete AFTER DELETE ON asset_meta BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER collection_generation_insert AFTER INSERT ON collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER collection_generation_update AFTER UPDATE ON collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER collection_generation_delete AFTER DELETE ON collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER collection_asset_generation_insert AFTER INSERT ON collection_asset BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER collection_asset_generation_update AFTER UPDATE ON collection_asset BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER collection_asset_generation_delete AFTER DELETE ON collection_asset BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER smart_collection_generation_insert AFTER INSERT ON smart_collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER smart_collection_generation_update AFTER UPDATE ON smart_collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER smart_collection_generation_delete AFTER DELETE ON smart_collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER keyword_generation_insert AFTER INSERT ON keyword BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER keyword_generation_update AFTER UPDATE ON keyword BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER keyword_generation_delete AFTER DELETE ON keyword BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER asset_keyword_generation_insert AFTER INSERT ON asset_keyword BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER asset_keyword_generation_update AFTER UPDATE ON asset_keyword BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER asset_keyword_generation_delete AFTER DELETE ON asset_keyword BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER annotation_generation_insert AFTER INSERT ON annotation BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER annotation_generation_update AFTER UPDATE ON annotation BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER annotation_generation_delete AFTER DELETE ON annotation BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_generation_insert AFTER INSERT ON preview BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_generation_update AFTER UPDATE ON preview BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_generation_delete AFTER DELETE ON preview BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_blob_generation_insert AFTER INSERT ON preview_blob BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_blob_generation_update AFTER UPDATE ON preview_blob BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_blob_generation_delete AFTER DELETE ON preview_blob BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_setting_generation_insert AFTER INSERT ON preview_setting BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_setting_generation_update AFTER UPDATE ON preview_setting BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER preview_setting_generation_delete AFTER DELETE ON preview_setting BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER replica_asset_insert BEFORE INSERT ON asset
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_asset_update BEFORE UPDATE ON asset
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_asset_delete BEFORE DELETE ON asset
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_folder_insert BEFORE INSERT ON folder
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_folder_update BEFORE UPDATE ON folder
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_folder_delete BEFORE DELETE ON folder
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_source_insert BEFORE INSERT ON source
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_source_update BEFORE UPDATE ON source
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_source_delete BEFORE DELETE ON source
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_thumbnail_insert BEFORE INSERT ON thumbnail
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_thumbnail_update BEFORE UPDATE ON thumbnail
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_thumbnail_delete BEFORE DELETE ON thumbnail
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_metadata_raw_insert BEFORE INSERT ON metadata_raw
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_metadata_raw_update BEFORE UPDATE ON metadata_raw
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_metadata_raw_delete BEFORE DELETE ON metadata_raw
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_asset_meta_insert BEFORE INSERT ON asset_meta
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_asset_meta_update BEFORE UPDATE ON asset_meta
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_asset_meta_delete BEFORE DELETE ON asset_meta
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_preview_insert BEFORE INSERT ON preview
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_preview_update BEFORE UPDATE ON preview
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_preview_delete BEFORE DELETE ON preview
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_preview_setting_insert BEFORE INSERT ON preview_setting
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_preview_setting_update BEFORE UPDATE ON preview_setting
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_preview_setting_delete BEFORE DELETE ON preview_setting
WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'media and preview settings are server-managed on a synced replica'); END;
CREATE TRIGGER replica_annotation_text BEFORE UPDATE ON annotation
WHEN EXISTS(SELECT 1 FROM replica_base) AND
(NEW.caption IS NOT OLD.caption OR NEW.headline IS NOT OLD.headline OR NEW.creator IS NOT OLD.creator OR NEW.copyright IS NOT OLD.copyright)
BEGIN SELECT RAISE(ABORT,'only ratings, labels and keywords can be edited on a replica'); END;
CREATE TRIGGER replica_collection_delete BEFORE DELETE ON collection WHEN EXISTS(SELECT 1 FROM replica_base)
BEGIN SELECT RAISE(ABORT,'collection deletion is not supported on replicas'); END;
CREATE TRIGGER preview_invalidate AFTER UPDATE OF content_hash ON asset
WHEN NEW.content_hash IS NOT OLD.content_hash BEGIN DELETE FROM preview WHERE asset_id=NEW.id; END;
CREATE TRIGGER catalogue_schema_generation AFTER UPDATE OF schema_version ON catalogue
WHEN NEW.schema_version IS NOT OLD.schema_version BEGIN UPDATE catalogue SET generation=generation+1; END;
UPDATE catalogue SET schema_version=7;
CREATE TRIGGER replica_annotation_insert_text BEFORE INSERT ON annotation
WHEN EXISTS(SELECT 1 FROM replica_base) AND
(NEW.caption IS NOT NULL OR NEW.headline IS NOT NULL OR NEW.creator IS NOT NULL OR NEW.copyright IS NOT NULL)
BEGIN SELECT RAISE(ABORT,'only ratings, labels and keywords can be edited on a replica'); END;
CREATE TRIGGER replica_annotation_delete_text BEFORE DELETE ON annotation
WHEN EXISTS(SELECT 1 FROM replica_base) AND
(OLD.caption IS NOT NULL OR OLD.headline IS NOT NULL OR OLD.creator IS NOT NULL OR OLD.copyright IS NOT NULL)
BEGIN SELECT RAISE(ABORT,'server annotation text cannot be deleted on a replica'); END;
