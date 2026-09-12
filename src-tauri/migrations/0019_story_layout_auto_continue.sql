-- Whether an episode's end chains straight into the next one on the device
-- (the menu layout only: the sequential layout always chains). Off by
-- default: the end of an episode returns to the wheel, positioned on the
-- episode just heard. A per-story choice — one child wants the series to
-- roll on, another wants to pick every time.
ALTER TABLE story_layouts ADD COLUMN auto_continue INTEGER NOT NULL DEFAULT 0;
