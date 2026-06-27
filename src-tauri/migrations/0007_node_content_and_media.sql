-- Schema v2: the canonical story model gains a single editable current node
-- (text, metadata, optional media references). This migration ships the two
-- tables the node-editing surface needs and re-stamps every legacy v1 row to
-- the v2 single-node shape.

-- Per-node media assets. The BYTES live on disk under the node-media store
-- (content-addressed `<content_hash>.<ext>`); SQLite owns the metadata. Tied
-- to a story by `ON DELETE CASCADE` so deleting a story reclaims its asset
-- rows. `media_format` is the closed set the editor accepts (sniffed by magic
-- bytes, never by extension).
CREATE TABLE IF NOT EXISTS assets (
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
  CHECK (media_format IN ('png', 'jpeg', 'mp3', 'wav', 'ogg')),
  CHECK (byte_size >= 0),
  -- The stored file name is never empty (the store names it `<hash>.<ext>`).
  CHECK (length(file_name) >= 1)
);

CREATE INDEX IF NOT EXISTS idx_assets__story_id ON assets (story_id);

-- Node-content recovery buffer (NFR8): a kill -9 mid-edit must not lose the
-- typed node text. Mirrors `story_drafts` (PK story_id, FK CASCADE, UPSERT
-- "latest wins" guarded by `draft_at`) but buffers the node's in-progress
-- text + label instead of the title. Kept separate from `story_drafts` so the
-- title recovery flow is never entangled or regressed.
CREATE TABLE IF NOT EXISTS node_drafts (
  story_id    TEXT PRIMARY KEY,
  node_id     TEXT NOT NULL,
  draft_text  TEXT NOT NULL,
  draft_label TEXT NOT NULL,
  draft_at    TEXT NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (length(node_id) >= 1),
  -- Generous upper bounds (anti-DoS), mirrored by the Rust-side guards.
  CHECK (length(draft_text) <= 65536),
  CHECK (length(draft_label) <= 4096)
);

CREATE INDEX IF NOT EXISTS idx_node_drafts__draft_at ON node_drafts (draft_at);

-- Re-stamp every legacy v1 story to the v2 single-node shape. In v1 the node
-- list was ALWAYS empty (guaranteed by the type), so every v1 row carries the
-- same empty structure and loses NOTHING by being re-stamped to the same empty
-- v2 starting node. The structure_json below is byte-identical to
-- `canonical_structure_json(&CanonicalStructure::minimal())`; the checksum is
-- its precomputed SHA-256 (a Rust integration test asserts the two agree).
-- Idempotent: after this runs every row is v2, so a re-run matches nothing.
-- Imported stories are re-stamped too — their distinguishing data is their
-- provenance row, never the (empty) structure; provenance is untouched.
UPDATE stories
SET
  structure_json = '{"schemaVersion":2,"nodes":[{"id":"n1","text":"","label":"","imageAssetId":null,"audioAssetId":null}]}',
  schema_version = 2,
  content_checksum = '86077d78a039fc6e70ae076ff1dc9cce65ebda3d0c2a77de10502d2fee36b333'
WHERE schema_version = 1;
