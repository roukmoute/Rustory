//! Integration tests for the v10 migration: `story_local_imports` accepts
//! the `resolved` import state (the write-path review resolution).
//!
//! v10 is the project's first REBUILD-for-CHECK migration (SQLite cannot
//! alter a CHECK in place): CREATE new / INSERT..SELECT / DROP / RENAME,
//! executed with `foreign_keys=ON`. These tests prove the recipe preserves
//! every pre-v10 row byte-for-byte, keeps every 0006 constraint alive on
//! the rebuilt table, and only widens the `import_state` set.

use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

/// The canonical v3 minimal structure + its exact checksum (the same frozen
/// fixture pair the device tests use).
const V3_MINIMAL: &str = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
const V3_MINIMAL_CHECKSUM: &str =
    "65d663fd2180630fa24693a5ccaee6d663b7a0f78b7d44b0e5ef07adc3f293b2";

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
             VALUES (?1, 'Histoire', 3, ?2, ?3, '2026-07-06T00:00:00.000Z', '2026-07-06T00:00:00.000Z')",
            rusqlite::params![id, V3_MINIMAL, V3_MINIMAL_CHECKSUM],
        )
        .expect("insert parent story");
}

fn insert_import(
    db: &DbHandle,
    story_id: &str,
    state: &str,
    summary: Option<&str>,
) -> Result<usize, rusqlite::Error> {
    db.conn().execute(
        "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
         VALUES (?1, 'rustory', 1, 'histoire.rustory', ?2, ?3, ?4, '2026-07-06T00:00:00.000Z')",
        rusqlite::params![story_id, "a".repeat(64), state, summary],
    )
}

const FINDINGS: &str = "[{\"aspect\":\"structure\",\"category\":\"ambiguous\"}]";

/// One full `story_local_imports` row read back for the row-by-row
/// preservation assertion.
type ProvenanceRow = (
    String,
    String,
    i64,
    String,
    String,
    String,
    Option<String>,
    String,
);

#[test]
fn pre_v10_rows_are_preserved_row_by_row() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    // First boot: a v9-level install (v1..v9 applied by hand, ledger marked —
    // v9's Rust hook is a no-op on a base with no v2 story).
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
            (
                6,
                include_str!("../../migrations/0006_story_local_imports.sql"),
            ),
            (
                7,
                include_str!("../../migrations/0007_node_content_and_media.sql"),
            ),
            (
                8,
                include_str!("../../migrations/0008_assets_content_hash_index.sql"),
            ),
            (
                9,
                include_str!("../../migrations/0009_multi_node_structure.sql"),
            ),
        ] {
            db.conn().execute_batch(file).expect("apply migration");
            db.conn()
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, '2026-07-06T00:00:00Z')",
                    rusqlite::params![version],
                )
                .expect("record migration");
        }
        // One row per pre-v10 persistable state.
        insert_story(&db, "s-recognized");
        insert_import(&db, "s-recognized", "recognized", None).expect("seed recognized");
        insert_story(&db, "s-partial");
        insert_import(&db, "s-partial", "partial", Some(FINDINGS)).expect("seed partial");
        insert_story(&db, "s-review");
        insert_import(&db, "s-review", "needs_review", Some(FINDINGS)).expect("seed needs_review");
    }

    // Second boot: the newer binary rebuilds the table through v10.
    let mut db = open_at(&path).expect("reopen");
    run_migrations(&mut db).expect("upgrade");

    let rows: Vec<ProvenanceRow> = db
        .conn()
        .prepare(
            "SELECT story_id, source_format, source_format_version, source_name, \
                    artifact_checksum, import_state, findings_summary, imported_at \
             FROM story_local_imports ORDER BY story_id",
        )
        .expect("prepare")
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");

    let expected_checksum = "a".repeat(64);
    assert_eq!(rows.len(), 3, "every pre-v10 row survives the rebuild");
    for (story_id, format, version, name, checksum, state, summary, imported_at) in &rows {
        assert_eq!(format, "rustory");
        assert_eq!(*version, 1);
        assert_eq!(name, "histoire.rustory");
        assert_eq!(checksum, &expected_checksum);
        assert_eq!(imported_at, "2026-07-06T00:00:00.000Z");
        match story_id.as_str() {
            "s-recognized" => {
                assert_eq!(state, "recognized");
                assert!(summary.is_none());
            }
            "s-partial" => {
                assert_eq!(state, "partial");
                assert_eq!(summary.as_deref(), Some(FINDINGS));
            }
            "s-review" => {
                assert_eq!(state, "needs_review");
                assert_eq!(summary.as_deref(), Some(FINDINGS));
            }
            other => panic!("unexpected row {other}"),
        }
    }
}

#[test]
fn resolved_is_accepted_and_keeps_its_findings() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-resolved");

    insert_import(&db, "s-resolved", "resolved", Some(FINDINGS))
        .expect("a settled review persists WITH its findings trace");

    let (state, summary): (String, Option<String>) = db
        .conn()
        .query_row(
            "SELECT import_state, findings_summary FROM story_local_imports WHERE story_id = 's-resolved'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row");
    assert_eq!(state, "resolved");
    assert_eq!(summary.as_deref(), Some(FINDINGS));
}

#[test]
fn resolved_without_findings_is_refused_by_the_invariant() {
    // The marker invariant is UNCHANGED by v10: only `recognized` carries a
    // NULL report — a settled review KEEPS its findings trace.
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-bare");

    let err = insert_import(&db, "s-bare", "resolved", None)
        .expect_err("resolved with a NULL report must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn blocked_is_still_refused_after_the_rebuild() {
    // `blocked` is never imported nor persisted — the rebuilt CHECK keeps
    // refusing it (the historical 0006 guarantee must not be weakened).
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-blocked");

    let err = insert_import(&db, "s-blocked", "blocked", Some(FINDINGS))
        .expect_err("blocked must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn cascade_delete_survives_the_rebuild() {
    // The rebuilt table re-declares the FK: deleting the parent story must
    // still cascade onto the provenance row (foreign_keys=ON at open).
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-cascade");
    insert_import(&db, "s-cascade", "resolved", Some(FINDINGS)).expect("seed");

    db.conn()
        .execute("DELETE FROM stories WHERE id = 's-cascade'", [])
        .expect("delete parent");

    let count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM story_local_imports", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 0, "ON DELETE CASCADE must survive the rebuild");
}

#[test]
fn idempotency_v10() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");

    run_migrations(&mut db).expect("first apply");
    run_migrations(&mut db).expect("second apply");

    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 10",
            [],
            |row| row.get(0),
        )
        .expect("count v10");
    assert_eq!(count, 1, "v10 must be recorded exactly once");
}
