-- Let the local-import provenance carry the web-page (non-RSS) import flow:
-- the explicitly listed format set gains 'web'.
-- SQLite cannot alter a CHECK in place, so the table is REBUILT (same recipe
-- as 0010/0011/0013/0014): same columns, same FK, every other CHECK VERBATIM —
-- only the source_format set widens. The current table carries the 9th column
-- added by 0015 (`source_archive_retained`), so it is carried over too. The
-- child-only recipe (no table references story_local_imports) is safe with
-- foreign_keys=ON.
--
-- `source_format_version` for 'web' is the revision of OUR extraction support
-- (1 = first frozen-contract extraction stage), not a value declared inside
-- the foreign page.
CREATE TABLE story_local_imports_new (
  story_id              TEXT    PRIMARY KEY,
  source_format         TEXT    NOT NULL,
  source_format_version INTEGER NOT NULL,
  source_name           TEXT    NOT NULL,
  artifact_checksum     TEXT    NOT NULL,
  import_state          TEXT    NOT NULL,
  findings_summary      TEXT,
  imported_at           TEXT    NOT NULL,
  source_archive_retained INTEGER NOT NULL DEFAULT 0,
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
  -- writes 'rustory' / 'structured-folder' / 'rss' / 'structured-archive' /
  -- 'web', so the schema refuses anything else (no implicit format).
  CHECK (source_format IN ('rustory', 'structured-folder', 'rss', 'structured-archive', 'web')),
  CHECK (source_format_version >= 1),
  -- The marker invariant is UNCHANGED: a clean (`recognized`) import has NO
  -- report (`findings_summary IS NULL`) and every other state ALWAYS carries
  -- one — a settled (`resolved`) review KEEPS its findings as the trace.
  CHECK ((import_state = 'recognized') = (findings_summary IS NULL))
);

INSERT INTO story_local_imports_new
  SELECT story_id, source_format, source_format_version, source_name,
         artifact_checksum, import_state, findings_summary, imported_at,
         source_archive_retained
  FROM story_local_imports;

DROP TABLE story_local_imports;

ALTER TABLE story_local_imports_new RENAME TO story_local_imports;
