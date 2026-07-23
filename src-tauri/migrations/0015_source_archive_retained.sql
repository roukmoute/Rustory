-- A structured-archive (.zip pack) import can RETAIN its ORIGINAL source
-- archive under {app_data_dir}/source-archives/<story_id>.zip, so
-- "Envoyer vers la Lunii" can send the imported story to a Lunii V3
-- (transcode + re-cipher for the TARGET device) WITHOUT re-picking the file.
--
-- This flag records that a retained archive EXISTS for the story (1); the
-- file itself is resolved by convention from the story id (never a stored
-- path). Only structured-archive imports ever set it — every other row
-- (rustory / structured-folder / rss, and pre-existing rows) stays 0, so a
-- library imported before this feature simply reports "not V3-sendable" until
-- re-imported. A plain ADD COLUMN (no CHECK touched) needs no table rebuild.
ALTER TABLE story_local_imports
  ADD COLUMN source_archive_retained INTEGER NOT NULL DEFAULT 0;
