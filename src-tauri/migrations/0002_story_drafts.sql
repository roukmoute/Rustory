CREATE TABLE IF NOT EXISTS story_drafts (
  story_id    TEXT    PRIMARY KEY,
  draft_title TEXT    NOT NULL,
  draft_at    TEXT    NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (length(draft_title) <= 4096)
);

CREATE INDEX IF NOT EXISTS idx_story_drafts__draft_at
  ON story_drafts (draft_at);
