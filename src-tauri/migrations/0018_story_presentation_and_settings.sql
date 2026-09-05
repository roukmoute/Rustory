-- How a story is PRESENTED on a device, beside its canonical structure (which
-- stays untouched: `structure_json` says what the story IS, these rows say
-- how a Lunii plays it). A missing `story_layouts` row means the default,
-- `sequential` (episodes chained in order — the layout every story had so
-- far). `menu` lets the child pick an episode on the wheel, like an official
-- pack: that needs SPOKEN announcements — the series title on the cover, the
-- menu question, and one spoken title per episode — synthesized by a voice
-- and stored as ordinary story audio assets (node-media store + `assets`
-- rows, so they are swept, exported and cascaded like any node media).
CREATE TABLE story_layouts (
  story_id                TEXT    PRIMARY KEY,
  layout                  TEXT    NOT NULL,
  -- Spoken series title (the cover prompt) and spoken menu question, as
  -- asset ids plus the exact text that was synthesized (staleness when the
  -- title or the question wording changes); NULL until generated.
  title_audio_asset_id    TEXT,
  title_spoken_text       TEXT,
  question_audio_asset_id TEXT,
  question_spoken_text    TEXT,
  -- The voice that produced the announcements (stable voice id), so a voice
  -- change can be detected as staleness.
  voice_id                TEXT,
  updated_at              TEXT    NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (layout IN ('sequential', 'menu'))
);

-- One spoken title per node (the prompt heard on the wheel for that
-- episode). `spoken_text` is the exact text that was synthesized: when the
-- node label changes, the prompt is detectably stale.
CREATE TABLE story_node_prompts (
  story_id        TEXT NOT NULL,
  node_id         TEXT NOT NULL,
  audio_asset_id  TEXT NOT NULL,
  spoken_text     TEXT NOT NULL,
  voice_id        TEXT NOT NULL,
  updated_at      TEXT NOT NULL,
  PRIMARY KEY (story_id, node_id),
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (length(node_id) >= 1),
  CHECK (length(audio_asset_id) >= 1),
  CHECK (length(voice_id) >= 1)
);

-- User-level application settings (the first: the announcement voice).
-- A flat key/value ledger, one row per setting, values as text.
CREATE TABLE app_settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (length(key) >= 1)
);
