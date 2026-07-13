-- Make the device-import provenance FAMILY-AWARE durably: a `story_imports`
-- row now records which device family the pack bytes came from, so the
-- transfer/preparation flows can refuse fail-closed to treat a pack of one
-- family as a device-format pack for another (a FLAM import must never
-- become writable toward a Lunii). SQLite cannot add a CHECK in place, so
-- the table is REBUILT (same recipe as 0010/0011): same columns, same FK,
-- every other CHECK VERBATIM — one new NOT NULL column with a closed set
-- and NO DEFAULT: the family is never implicit, every INSERT must state
-- it explicitly (a caller that forgot the column would otherwise record
-- 'lunii' silently — the exact fail-open path this column exists to
-- close). The historical backfill is carried by the INSERT…SELECT below.
-- Every pre-existing row is backfilled 'lunii': the device import flow only
-- ever acquired Lunii packs before this column existed.
CREATE TABLE story_imports_new (
  story_id                 TEXT    PRIMARY KEY,
  pack_uuid                TEXT    NOT NULL,
  source_device_identifier TEXT    NOT NULL,
  imported_at              TEXT    NOT NULL,
  pack_file_count          INTEGER NOT NULL,
  pack_total_bytes         INTEGER NOT NULL,
  pack_checksum            TEXT    NOT NULL,
  source_family            TEXT    NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (length(pack_uuid) = 36),
  CHECK (pack_file_count >= 1),
  CHECK (pack_total_bytes >= 0),
  CHECK (length(pack_checksum) = 64),
  -- Defense in depth on the closed family set: the service only ever writes
  -- the diagnostic tags of the recognized families, so the schema refuses
  -- anything else (no implicit family).
  CHECK (source_family IN ('lunii', 'flam'))
);

INSERT INTO story_imports_new
  SELECT story_id, pack_uuid, source_device_identifier, imported_at,
         pack_file_count, pack_total_bytes, pack_checksum, 'lunii'
  FROM story_imports;

DROP TABLE story_imports;

ALTER TABLE story_imports_new RENAME TO story_imports;

-- The UNIQUE content-identity index does not survive the DROP: recreate it
-- VERBATIM (the `already_imported` dedup and the UNIQUE-race close both
-- stand on it).
CREATE UNIQUE INDEX IF NOT EXISTS idx_story_imports__pack_uuid
  ON story_imports (pack_uuid);
