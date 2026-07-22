-- Let the local-import provenance carry the structured-archive (.zip pack)
-- creation flow: the explicitly listed format set gains 'structured-archive'.
-- SQLite cannot alter a CHECK in place, so the table is REBUILT (same recipe
-- as 0010/0011/0013): same columns, same FK, every other CHECK VERBATIM —
-- only the source_format set widens. The child-only recipe (no table
-- references story_local_imports) is safe with foreign_keys=ON.
--
-- `source_format_version` for 'structured-archive' is the revision of OUR
-- reader support (1 = the initial stage/action pack schema), not a value
-- declared inside the foreign file — the pack format itself declares none.
CREATE TABLE story_local_imports_new (
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
  -- A sober basename is never empty (the service refuses path separators; the
  -- column guards the non-empty floor at the DB).
  CHECK (length(source_name) >= 1),
  CHECK (import_state IN ('recognized', 'partial', 'needs_review', 'resolved')),
  -- Defense in depth on the explicitly listed formats: the service only ever
  -- writes 'rustory' / 'structured-folder' / 'rss' / 'structured-archive', so
  -- the schema refuses anything else (no implicit format).
  CHECK (source_format IN ('rustory', 'structured-folder', 'rss', 'structured-archive')),
  CHECK (source_format_version >= 1),
  -- The marker invariant is UNCHANGED: a clean (`recognized`) import has NO
  -- report (`findings_summary IS NULL`) and every other state ALWAYS carries
  -- one — a settled (`resolved`) review KEEPS its findings as the trace.
  CHECK ((import_state = 'recognized') = (findings_summary IS NULL))
);

INSERT INTO story_local_imports_new
  SELECT story_id, source_format, source_format_version, source_name,
         artifact_checksum, import_state, findings_summary, imported_at
  FROM story_local_imports;

DROP TABLE story_local_imports;

ALTER TABLE story_local_imports_new RENAME TO story_local_imports;
