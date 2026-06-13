CREATE TABLE IF NOT EXISTS story_imports (
  story_id                 TEXT    PRIMARY KEY,
  pack_uuid                TEXT    NOT NULL,
  source_device_identifier TEXT    NOT NULL,
  imported_at              TEXT    NOT NULL,
  pack_file_count          INTEGER NOT NULL,
  pack_total_bytes         INTEGER NOT NULL,
  pack_checksum            TEXT    NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (length(pack_uuid) = 36),
  CHECK (pack_file_count >= 1),
  CHECK (pack_total_bytes >= 0),
  CHECK (length(pack_checksum) = 64)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_story_imports__pack_uuid
  ON story_imports (pack_uuid);
