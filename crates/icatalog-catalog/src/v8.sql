-- D57: inclusive maximum rating, preserving collection identities/revisions.
DROP TRIGGER smart_no_manual_members;
DROP TRIGGER smart_no_manual_member_updates;
DROP TRIGGER smart_no_existing_members;
DROP TRIGGER smart_collection_revision_insert;
DROP TRIGGER smart_collection_revision_update;
DROP TRIGGER smart_collection_revision_delete;
DROP TRIGGER smart_collection_generation_insert;
DROP TRIGGER smart_collection_generation_update;
DROP TRIGGER smart_collection_generation_delete;
ALTER TABLE smart_collection RENAME TO smart_collection_v7;
-- A row marks a Smart Collection; absent row means a manual collection.
CREATE TABLE smart_collection (
  collection_id INTEGER PRIMARY KEY REFERENCES collection(id) ON DELETE CASCADE,
  format TEXT,
  captured_from TEXT,
  captured_to TEXT,
  camera TEXT,
  lens TEXT,
  min_rating INTEGER CHECK (min_rating BETWEEN 0 AND 5),
  max_rating INTEGER CHECK (max_rating IS NULL OR (typeof(max_rating)='integer' AND max_rating BETWEEN 0 AND 5)),
  label INTEGER CHECK (label BETWEEN 0 AND 7),
  keyword TEXT,
  text TEXT,
  CHECK (format IS NOT NULL OR captured_from IS NOT NULL OR captured_to IS NOT NULL
         OR camera IS NOT NULL OR lens IS NOT NULL OR min_rating IS NOT NULL
         OR max_rating IS NOT NULL OR label IS NOT NULL OR keyword IS NOT NULL OR text IS NOT NULL),
  CHECK (min_rating IS NULL OR max_rating IS NULL OR min_rating <= max_rating)
);
INSERT INTO smart_collection(collection_id,format,captured_from,captured_to,camera,lens,min_rating,label,keyword,text) SELECT collection_id,format,captured_from,captured_to,camera,lens,min_rating,label,keyword,text FROM smart_collection_v7;
DROP TABLE smart_collection_v7;
CREATE TRIGGER smart_no_manual_members BEFORE INSERT ON collection_asset
WHEN EXISTS (SELECT 1 FROM smart_collection WHERE collection_id = NEW.collection_id)
BEGIN SELECT RAISE(ABORT, 'Smart Collection membership is determined by its rules'); END;
CREATE TRIGGER smart_no_manual_member_updates BEFORE UPDATE ON collection_asset
WHEN EXISTS (SELECT 1 FROM smart_collection WHERE collection_id = NEW.collection_id)
BEGIN SELECT RAISE(ABORT, 'Smart Collection membership is determined by its rules'); END;
CREATE TRIGGER smart_no_existing_members BEFORE INSERT ON smart_collection
WHEN EXISTS (SELECT 1 FROM collection_asset WHERE collection_id = NEW.collection_id)
BEGIN SELECT RAISE(ABORT, 'Cannot apply smart rules to a manual collection'); END;
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
CREATE TRIGGER smart_collection_generation_insert AFTER INSERT ON smart_collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER smart_collection_generation_update AFTER UPDATE ON smart_collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
CREATE TRIGGER smart_collection_generation_delete AFTER DELETE ON smart_collection BEGIN
  UPDATE catalogue SET generation=generation+1;
END;
UPDATE catalogue SET schema_version=8;
