-- Durable cross-session memory of a transfer's LAST terminal outcome.
--
-- One row per story (PK `story_id`, UPSERT "latest wins" — the exact shape of
-- `story_drafts`). Only TERMINALS are persisted: `verified`, `partial`,
-- `retryable`, `incomplete`. An in-flight `transferring` / `verifying` phase is
-- NEVER written (it would be a lie after a restart — the job died with the app).
--
-- This is NOT event sourcing; it is minimal operational observability — the gap
-- the live appliance cannot reproduce (a non-success terminal a passive re-read
-- cannot re-derive) plus the last useful result. The canonical story data is
-- never carried here; the FK CASCADE keeps the memory purely operational.
--
-- `device_identifier` is recorded for traceability only — it is STALE by
-- construction (a write mutates `.pi`, so the identity changes) and is never used
-- to relaunch; a relaunch always re-validates a FRESH writable device. `cause`,
-- `completeness` and `verify_verdict` are the closed wire tags of the job
-- terminal; a write-phase terminal carries `cause` + `completeness`, a verify
-- terminal carries `verify_verdict` (mutually exclusive, mirroring the event
-- contract). `summary_changed` / `summary_unchanged` carry the `verified`
-- confirmation lines (composed in Rust); `message` / `user_action` carry the
-- canonical FR copy the panel renders verbatim.
CREATE TABLE IF NOT EXISTS transfer_jobs (
  story_id          TEXT    PRIMARY KEY,
  job_id            TEXT    NOT NULL,
  device_identifier TEXT,
  terminal_kind     TEXT    NOT NULL,
  cause             TEXT,
  completeness      TEXT,
  verify_verdict    TEXT,
  message           TEXT    NOT NULL,
  user_action       TEXT    NOT NULL,
  summary_changed   TEXT,
  summary_unchanged TEXT,
  recorded_at       TEXT    NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (terminal_kind IN ('verified', 'partial', 'retryable', 'incomplete')),
  CHECK (length(message) <= 4096),
  CHECK (length(user_action) <= 4096)
);

CREATE INDEX IF NOT EXISTS idx_transfer_jobs__recorded_at
  ON transfer_jobs (recorded_at);
