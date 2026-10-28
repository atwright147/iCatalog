-- A row marks a Smart Collection; absent row means a manual collection.
CREATE TABLE smart_collection (
  collection_id INTEGER PRIMARY KEY REFERENCES collection(id) ON DELETE CASCADE,
  format TEXT,
  captured_from TEXT,
  captured_to TEXT,
  camera TEXT,
  lens TEXT,
  min_rating INTEGER CHECK (min_rating BETWEEN 0 AND 5),
  label INTEGER CHECK (label BETWEEN 0 AND 7),
  keyword TEXT,
  text TEXT,
  CHECK (format IS NOT NULL OR captured_from IS NOT NULL OR captured_to IS NOT NULL
         OR camera IS NOT NULL OR lens IS NOT NULL OR min_rating IS NOT NULL
         OR label IS NOT NULL OR keyword IS NOT NULL OR text IS NOT NULL)
);
CREATE TRIGGER smart_no_manual_members BEFORE INSERT ON collection_asset
WHEN EXISTS (SELECT 1 FROM smart_collection WHERE collection_id = NEW.collection_id)
BEGIN SELECT RAISE(ABORT, 'Smart Collection membership is determined by its rules'); END;
CREATE TRIGGER smart_no_manual_member_updates BEFORE UPDATE ON collection_asset
WHEN EXISTS (SELECT 1 FROM smart_collection WHERE collection_id = NEW.collection_id)
BEGIN SELECT RAISE(ABORT, 'Smart Collection membership is determined by its rules'); END;
CREATE TRIGGER smart_no_existing_members BEFORE INSERT ON smart_collection
WHEN EXISTS (SELECT 1 FROM collection_asset WHERE collection_id = NEW.collection_id)
BEGIN SELECT RAISE(ABORT, 'Cannot apply smart rules to a manual collection'); END;
UPDATE catalogue SET schema_version = 6;
