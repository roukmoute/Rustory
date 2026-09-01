-- Let the node-media store carry the m4a (MP4 audio) format: the explicitly
-- listed media_format set on `assets` gains 'm4a'. Web-page podcast pages
-- (non-RSS) link their episodes as m4a files, and the magic-bytes sniffer
-- accepts them, so the DB must not refuse the rows the import writes.
-- SQLite cannot alter a CHECK in place, so the table is REBUILT (same recipe
-- as the assets creation in 0007): same columns, same FK, every other CHECK
-- VERBATIM — only the media_format set widens. The two assets indexes
-- (0007 story_id, 0008 content_hash) are recreated after the rename. The
-- child-only recipe (no table references assets) is safe with
-- foreign_keys=ON.
CREATE TABLE assets_new (
  id            TEXT    PRIMARY KEY,
  story_id      TEXT    NOT NULL,
  content_hash  TEXT    NOT NULL,
  media_type    TEXT    NOT NULL,
  media_format  TEXT    NOT NULL,
  byte_size     INTEGER NOT NULL,
  file_name     TEXT    NOT NULL,
  created_at    TEXT    NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  -- 64 LOWERCASE hex digits: a content-addressed SHA-256 of the source bytes.
  CHECK (length(content_hash) = 64),
  CHECK (content_hash NOT GLOB '*[^0-9a-f]*'),
  CHECK (media_type IN ('image', 'audio')),
  CHECK (media_format IN ('png', 'jpeg', 'mp3', 'wav', 'ogg', 'm4a')),
  CHECK (byte_size >= 0),
  -- The stored file name is never empty (the store names it `<hash>.<ext>`).
  CHECK (length(file_name) >= 1)
);

INSERT INTO assets_new
  SELECT id, story_id, content_hash, media_type, media_format,
         byte_size, file_name, created_at
  FROM assets;

DROP TABLE assets;

ALTER TABLE assets_new RENAME TO assets;

CREATE INDEX IF NOT EXISTS idx_assets__story_id ON assets (story_id);
CREATE INDEX IF NOT EXISTS idx_assets__content_hash ON assets (content_hash);
