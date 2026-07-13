//! Story edit scope (FR21): the AUTHORITATIVE derivation of what may be
//! edited on a story, declared PER IMPORT FORMAT — never "imported =
//! read-only" as a block.
//!
//! The scope is derived from `story_imports` ALONE (the device-pack
//! provenance table): a pack's content is the binary truth of the device
//! (its local canonical row is a placeholder), so only its TITLE — a local
//! Rustory metadata, packs store none — stays editable. A `.rustory` import
//! (`story_local_imports`) carries the SAME canonical v3 structure as a
//! native story and edits exactly like one: it is `Full` BY CONSTRUCTION,
//! because this derivation never consults `story_local_imports`. The forged
//! two-table case degrades to `TitleOnly` (the pack takes precedence — its
//! placeholder content must never be edited).

use rusqlite::Connection;

/// What may be edited on a story, declared per import format (FR21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryEditScope {
    /// The complete editor: node content, media, structure, option links.
    /// Native stories and `.rustory` imports.
    Full,
    /// Only the title (a local Rustory metadata). Device-pack stories.
    TitleOnly,
}

impl StoryEditScope {
    /// The wire tag projected to the frontend (`editScope`).
    pub fn wire_tag(self) -> &'static str {
        match self {
            StoryEditScope::Full => "full",
            StoryEditScope::TitleOnly => "titleOnly",
        }
    }
}

/// Derive the story's edit scope from its device-pack provenance.
///
/// This is the SINGLE authoritative authorization derivation on the write
/// path, so it fails CLOSED: a provenance query that errors (e.g. a
/// transient `SQLITE_BUSY`) is treated as a pack (`TitleOnly`) rather than
/// letting a content write slip through on a read hiccup. The title stays
/// editable either way — `update_story` never had a provenance guard, and a
/// pack is the very case whose title must remain locally renameable.
///
/// Callers that must FAIL their write rather than swallow the read error
/// (an acknowledgement must never carry a fabricated state) use
/// [`try_story_edit_scope`] — this is its infallible authorization wrapper.
pub fn story_edit_scope(conn: &Connection, story_id: &str) -> StoryEditScope {
    try_story_edit_scope(conn, story_id).unwrap_or(StoryEditScope::TitleOnly)
}

/// Fallible variant of the SAME derivation, for callers inside a write
/// transaction whose acknowledgement depends on the scope: a provenance
/// read that errors must fail the write (rolled back, retryable) instead of
/// degrading to `TitleOnly` and acknowledging `importState: null` while a
/// review is actually pending. The authorization guards keep the fail-closed
/// wrapper above.
pub fn try_story_edit_scope(
    conn: &Connection,
    story_id: &str,
) -> Result<StoryEditScope, rusqlite::Error> {
    let device_pack: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM story_imports WHERE story_id = ?1)",
        rusqlite::params![story_id],
        |r| r.get(0),
    )?;
    Ok(if device_pack {
        StoryEditScope::TitleOnly
    } else {
        StoryEditScope::Full
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::{create_story, CreateStoryInput};
    use crate::infrastructure::db::{self, DbHandle};

    fn fresh_db() -> DbHandle {
        let mut db = db::open_in_memory().expect("open");
        db::run_migrations(&mut db).expect("migrate");
        db
    }

    fn new_story(db: &mut DbHandle) -> String {
        create_story(
            db,
            CreateStoryInput {
                title: "Histoire".into(),
            },
        )
        .expect("create")
        .id
    }

    fn mark_device_pack(db: &DbHandle, story_id: &str) {
        db.conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
                 VALUES (?1, '019739b2-0000-7000-8000-000000000000', '0123456789abcdef0123456789abcdef', '2026-07-06T00:00:00.000Z', 5, 18, ?2, 'lunii')",
                rusqlite::params![story_id, "ab".repeat(32)],
            )
            .expect("insert pack provenance");
    }

    fn mark_local_import(db: &DbHandle, story_id: &str) {
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'a.rustory', ?2, 'recognized', NULL, '2026-07-06T00:00:00.000Z')",
                rusqlite::params![story_id, "a".repeat(64)],
            )
            .expect("insert local provenance");
    }

    #[test]
    fn a_native_story_has_full_scope() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        assert_eq!(story_edit_scope(db.conn(), &id), StoryEditScope::Full);
    }

    #[test]
    fn a_rustory_import_has_full_scope_by_construction() {
        // A `.rustory` import carries the REAL canonical v3 structure — its
        // declared edit scope is the complete editor, exactly like a native.
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_local_import(&db, &id);
        assert_eq!(story_edit_scope(db.conn(), &id), StoryEditScope::Full);
    }

    #[test]
    fn a_device_pack_has_title_only_scope() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_device_pack(&db, &id);
        assert_eq!(story_edit_scope(db.conn(), &id), StoryEditScope::TitleOnly);
    }

    #[test]
    fn forged_rows_in_both_tables_degrade_to_title_only() {
        // The pack takes precedence: its placeholder canonical content must
        // never be edited, whatever a forged local-import row claims.
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_device_pack(&db, &id);
        mark_local_import(&db, &id);
        assert_eq!(story_edit_scope(db.conn(), &id), StoryEditScope::TitleOnly);
    }

    #[test]
    fn scope_fails_closed_to_title_only_on_a_query_error() {
        // The authorization derivation must deny content editing (fail
        // closed) when the provenance cannot be read — never allow it.
        let db = fresh_db();
        db.conn().execute("DROP TABLE story_imports", []).unwrap();
        assert_eq!(
            story_edit_scope(db.conn(), "any-id"),
            StoryEditScope::TitleOnly,
            "a query error must degrade to TitleOnly (fail closed)"
        );
    }

    #[test]
    fn try_scope_surfaces_the_query_error_instead_of_degrading() {
        // The fallible variant lets ACK-building callers fail their write
        // rather than acknowledge a state fabricated from a read hiccup.
        let db = fresh_db();
        db.conn().execute("DROP TABLE story_imports", []).unwrap();
        assert!(
            try_story_edit_scope(db.conn(), "any-id").is_err(),
            "the fallible variant must propagate the query error"
        );
    }

    #[test]
    fn wire_tags_are_the_frozen_contract_values() {
        assert_eq!(StoryEditScope::Full.wire_tag(), "full");
        assert_eq!(StoryEditScope::TitleOnly.wire_tag(), "titleOnly");
    }
}
