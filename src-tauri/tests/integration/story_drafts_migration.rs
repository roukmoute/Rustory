//! Integration tests for the v2 migration that introduces `story_drafts`.
//!
//! These tests exercise the on-disk SQLite path (TempDir) so the WAL and
//! migration ledger interactions match what runs on a real install. The
//! v2 SQL is loaded transitively through `run_migrations` — there is no
//! "apply v2 only" entry point, by design (forward-only migrations).

use rustory_lib::infrastructure::db::{open_at, run_migrations};
use tempfile::TempDir;

#[test]
fn fresh_install_applies_v2_migration() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");
    run_migrations(&mut db).expect("migrate");

    // The table is queryable after migration → schema is in place.
    let count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM story_drafts", [], |row| row.get(0))
        .expect("query story_drafts");
    assert_eq!(count, 0, "fresh DB must have an empty story_drafts table");

    // The migration ledger records both versions.
    let versions: Vec<u32> = db
        .conn()
        .prepare("SELECT version FROM schema_migrations ORDER BY version ASC")
        .expect("prepare")
        .query_map([], |row| row.get::<_, u32>(0))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");
    assert_eq!(versions, vec![1, 2], "ledger must contain v1 and v2");
}

#[test]
fn idempotency_v2() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");

    run_migrations(&mut db).expect("first apply");
    run_migrations(&mut db).expect("second apply");

    // Calling run_migrations twice must not duplicate the ledger row.
    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 2",
            [],
            |row| row.get(0),
        )
        .expect("count v2");
    assert_eq!(count, 1, "v2 must be recorded exactly once");
}

#[test]
fn existing_v1_database_upgrades_to_v2() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    // First boot: pretend the binary only knew about v1. Apply v1 SQL by
    // hand and record the ledger row to mimic a v1-aware install.
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
        let v1_sql = include_str!("../../migrations/0001_init.sql");
        db.conn().execute_batch(v1_sql).expect("apply v1");
        db.conn()
            .execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (1, '2026-04-22T00:00:00Z')",
                [],
            )
            .expect("record v1");
    }

    // Second boot: a newer binary running the full migration set must add
    // v2 without touching v1's ledger row.
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
    assert_eq!(versions, vec![1, 2], "v1 row preserved + v2 added");

    // The new table is usable.
    let count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM story_drafts", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 0);
}
