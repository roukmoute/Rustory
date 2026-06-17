-- Local UUID -> title index for device-story recognition (STUdio model).
--
-- One row per (pack_uuid, source): the same pack can carry BOTH a user-typed
-- title and an official catalog title at once; resolution priority
-- (User > Official > Unofficial) is applied in the application layer.
--
-- The `official` rows are a DISPOSABLE cache of the commercial catalog,
-- fetched on an explicit user action and replaced wholesale on refresh.
-- The `user` rows are durable and NEVER touched by a catalog refresh, so a
-- name the user typed survives unplug/replug and any later recognition.
-- The `unofficial` source is reserved for a future community index; titles
-- inferred from the local library (imported/transferred stories) are derived
-- on the fly from `story_imports` and are not stored here.
CREATE TABLE IF NOT EXISTS pack_metadata (
  pack_uuid  TEXT NOT NULL,
  source     TEXT NOT NULL,
  title      TEXT NOT NULL,
  thumbnail  TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (pack_uuid, source),
  CHECK (length(pack_uuid) = 36),
  CHECK (source IN ('user', 'official', 'unofficial')),
  CHECK (length(trim(title)) > 0),
  CHECK (length(title) <= 120),
  CHECK (thumbnail IS NULL OR length(thumbnail) <= 2048)
);

-- Refreshing the official catalog deletes every `source = 'official'` row
-- then re-inserts; the index keeps that bulk delete cheap.
CREATE INDEX IF NOT EXISTS idx_pack_metadata__source
  ON pack_metadata (source);
