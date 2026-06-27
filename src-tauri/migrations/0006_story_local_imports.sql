CREATE TABLE IF NOT EXISTS story_local_imports (
  story_id              TEXT    PRIMARY KEY,
  source_format         TEXT    NOT NULL,
  source_format_version INTEGER NOT NULL,
  source_name           TEXT    NOT NULL,
  artifact_checksum     TEXT    NOT NULL,
  import_state          TEXT    NOT NULL,
  findings_summary      TEXT,
  imported_at           TEXT    NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (length(artifact_checksum) = 64),
  -- Exactly 64 LOWERCASE hex digits: length 64 above + no character outside
  -- [0-9a-f] here (GLOB '[^...]' negation). Defense in depth against a forged
  -- provenance fingerprint slipping past the service-level validation.
  CHECK (artifact_checksum NOT GLOB '*[^0-9a-f]*'),
  -- A sober basename is never empty (the service refuses path separators / a
  -- non-`.rustory` name; the column guards the non-empty floor at the DB).
  CHECK (length(source_name) >= 1),
  CHECK (import_state IN ('recognized', 'partial', 'needs_review')),
  -- Defense in depth on the only format this iteration imports: the service
  -- only ever writes 'rustory' / 1, so the schema refuses anything else.
  CHECK (source_format = 'rustory'),
  CHECK (source_format_version >= 1),
  -- The marker invariant: a clean (`recognized`) import has NO report
  -- (`findings_summary IS NULL`), and a `partial` / `needs_review` import
  -- ALWAYS carries one — so a card never shows an attention chip with an
  -- empty report (nor a clean import a stray report).
  CHECK ((import_state = 'recognized') = (findings_summary IS NULL))
);
