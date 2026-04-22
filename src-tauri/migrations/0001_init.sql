CREATE TABLE IF NOT EXISTS stories (
  id               TEXT    PRIMARY KEY,
  title            TEXT    NOT NULL,
  schema_version   INTEGER NOT NULL,
  structure_json   TEXT    NOT NULL,
  content_checksum TEXT    NOT NULL,
  created_at       TEXT    NOT NULL,
  updated_at       TEXT    NOT NULL,
  CHECK (length(trim(title)) > 0),
  CHECK (schema_version >= 1),
  CHECK (length(content_checksum) = 64)
);

CREATE INDEX IF NOT EXISTS idx_stories__created_at_id
  ON stories (created_at, id);
