//! Integration tests for the v6 migration that introduces
//! `story_local_imports`.
//!
//! Mirror of `story_imports_migration.rs`: exercises the on-disk SQLite
//! path (TempDir) so the WAL and migration ledger interactions match a
//! real install. `story_local_imports` is the durable provenance link
//! for a FILE artifact (`.rustory`) — distinct from `story_imports`
//! (device pack provenance): no `pack_uuid`, no source device. It also
//! carries the `import_state` that makes the `Import Issue Marker`
//! durable across restarts.

use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

fn fresh_disk_db(tmp: &TempDir) -> DbHandle {
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");
    run_migrations(&mut db).expect("migrate");
    db
}

fn insert_story(db: &DbHandle, id: &str) {
    db.conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, 'Histoire', 1, '{\"schemaVersion\":1,\"nodes\":[]}', \
             '0000000000000000000000000000000000000000000000000000000000000000', \
             '2026-06-10T00:00:00.000Z', '2026-06-10T00:00:00.000Z')",
            rusqlite::params![id],
        )
        .expect("insert parent story");
}

fn insert_import(db: &DbHandle, story_id: &str, state: &str) -> Result<usize, rusqlite::Error> {
    // The state ⟺ summary CHECK: a `recognized` import has NO report (NULL),
    // a `partial` / `needs_review` one ALWAYS carries one.
    let summary: Option<&str> = if state == "recognized" {
        None
    } else {
        Some("[{\"aspect\":\"title\",\"category\":\"ambiguous\"}]")
    };
    db.conn().execute(
        "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
         VALUES (?1, 'rustory', 1, 'histoire.rustory', ?2, ?3, ?4, '2026-06-27T00:00:00.000Z')",
        rusqlite::params![story_id, "a".repeat(64), state, summary],
    )
}

#[test]
fn fresh_install_applies_v6_migration_with_canonical_columns() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);

    let mut stmt = db
        .conn()
        .prepare("PRAGMA table_info(story_local_imports)")
        .expect("prepare");
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");
    assert_eq!(
        columns,
        vec![
            "story_id",
            "source_format",
            "source_format_version",
            "source_name",
            "artifact_checksum",
            "import_state",
            "findings_summary",
            "imported_at",
        ],
        "schema must match the canonical column set"
    );

    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 6",
            [],
            |row| row.get(0),
        )
        .expect("ledger");
    assert_eq!(count, 1, "v6 must be recorded in the ledger");
}

#[test]
fn idempotency_v6() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");

    run_migrations(&mut db).expect("first apply");
    run_migrations(&mut db).expect("second apply");

    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 6",
            [],
            |row| row.get(0),
        )
        .expect("count v6");
    assert_eq!(count, 1, "v6 must be recorded exactly once");
}

#[test]
fn story_id_is_the_primary_key_one_provenance_row_per_story() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-a");

    insert_import(&db, "story-a", "recognized").expect("first provenance row");
    let err = insert_import(&db, "story-a", "partial")
        .expect_err("a second row for the same story must fail on the PK");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("unique") || message.contains("primary"),
        "expected primary-key conflict, got: {message}"
    );
}

#[test]
fn cascade_delete_removes_the_provenance_link_with_the_story() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-cascade");
    insert_import(&db, "story-cascade", "needs_review").expect("import row");

    db.conn()
        .execute("DELETE FROM stories WHERE id = 'story-cascade'", [])
        .expect("delete parent");

    let count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM story_local_imports", [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(
        count, 0,
        "FK ON DELETE CASCADE must remove the provenance link when the story is deleted"
    );
}

#[test]
fn rejects_orphan_story_id_via_fk() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    let err = insert_import(&db, "orphan", "recognized").expect_err("orphan must be rejected");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("foreign key"),
        "expected FK constraint failure, got: {message}"
    );
}

#[test]
fn check_constraints_guard_checksum_length_and_import_state() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-checks");

    // artifact_checksum must be a 64-char SHA-256 hex digest.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('story-checks', 'rustory', 1, 'h.rustory', 'abc', 'recognized', NULL, '2026-06-27T00:00:00.000Z')",
            [],
        )
        .expect_err("short checksum must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // artifact_checksum of the right length but NOT lowercase hex must also
    // trip the CHECK (forged fingerprint defense in depth).
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('story-checks', 'rustory', 1, 'h.rustory', ?1, 'recognized', NULL, '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["Z".repeat(64)],
        )
        .expect_err("non-hex checksum must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // source_name must be non-empty.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('story-checks', 'rustory', 1, '', ?1, 'recognized', NULL, '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["a".repeat(64)],
        )
        .expect_err("empty source_name must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // import_state must be in the closed durable set ('blocked' is never
    // persisted — nothing is imported — and must be refused at the DB level).
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('story-checks', 'rustory', 1, 'h.rustory', ?1, 'blocked', NULL, '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["a".repeat(64)],
        )
        .expect_err("an unpersistable import_state must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn check_couples_import_state_with_findings_summary_presence() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-couple");

    // `needs_review` (or `partial`) WITHOUT a report must be refused: a card
    // never shows an attention chip with an empty `<details>`.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('story-couple', 'rustory', 1, 'h.rustory', ?1, 'needs_review', NULL, '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["a".repeat(64)],
        )
        .expect_err("needs_review without a report must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // `recognized` WITH a report must also be refused (a clean import has none).
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('story-couple', 'rustory', 1, 'h.rustory', ?1, 'recognized', '[]', '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["a".repeat(64)],
        )
        .expect_err("recognized with a report must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn check_guards_source_format_and_version() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-format");

    // Only the explicitly-listed `.rustory` format / version 1 is imported.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('story-format', 'zip', 1, 'h.rustory', ?1, 'recognized', NULL, '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["a".repeat(64)],
        )
        .expect_err("an unknown source_format must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('story-format', 'rustory', 0, 'h.rustory', ?1, 'recognized', NULL, '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["a".repeat(64)],
        )
        .expect_err("source_format_version 0 must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn findings_summary_is_nullable_for_a_clean_import() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-clean");
    // A `recognized` import carries no findings — the column stays NULL.
    insert_import(&db, "story-clean", "recognized").expect("clean import row");

    let summary: Option<String> = db
        .conn()
        .query_row(
            "SELECT findings_summary FROM story_local_imports WHERE story_id = 'story-clean'",
            [],
            |row| row.get(0),
        )
        .expect("row");
    assert!(
        summary.is_none(),
        "a clean import has a NULL findings_summary"
    );
}

#[test]
fn existing_v5_database_upgrades_to_v6() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    // First boot: mimic a v5-aware install (v1..v5 applied by hand).
    {
        let db = open_at(&path).expect("open initial");
        db.conn()
            .execute(
                "CREATE TABLE IF NOT EXISTS schema_migrations ( \
                   version INTEGER PRIMARY KEY, \
                   applied_at TEXT NOT NULL \
                 )",
                [],
            )
            .expect("ledger");
        for (version, file) in [
            (1, include_str!("../../migrations/0001_init.sql")),
            (2, include_str!("../../migrations/0002_story_drafts.sql")),
            (3, include_str!("../../migrations/0003_story_imports.sql")),
            (4, include_str!("../../migrations/0004_pack_metadata.sql")),
            (5, include_str!("../../migrations/0005_transfer_jobs.sql")),
        ] {
            db.conn().execute_batch(file).expect("apply migration");
            db.conn()
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, '2026-06-26T00:00:00Z')",
                    rusqlite::params![version],
                )
                .expect("record migration");
        }
    }

    // Second boot: the newer binary adds v6 without touching v1..v5 rows.
    let mut db = open_at(&path).expect("reopen");
    run_migrations(&mut db).expect("upgrade");

    let versions: Vec<u32> = db
        .conn()
        .prepare("SELECT version FROM schema_migrations ORDER BY version ASC")
        .expect("prepare")
        .query_map([], |row| row.get::<_, u32>(0))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");
    assert_eq!(
        versions,
        vec![1, 2, 3, 4, 5, 6, 7, 8],
        "v1..v5 preserved, v6 added"
    );

    insert_story(&db, "story-upgraded");
    insert_import(&db, "story-upgraded", "partial").expect("table usable after upgrade");
}
