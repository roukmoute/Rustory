//! Integration tests for the v7 migration (`assets`, `node_drafts`, and the
//! v1→v2 re-stamp of existing stories).
//!
//! Exercises the on-disk SQLite path (TempDir) so the WAL + migration ledger
//! interactions match a real install. The re-stamp is a DATA migration (not
//! just DDL): a legacy v1 row must come out as the canonical v2 single-node
//! shape with a recomputed `content_checksum`, idempotently.

use rustory_lib::domain::story::{canonical_structure_json, content_checksum, CanonicalStructure};
use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

fn fresh_disk_db(tmp: &TempDir) -> DbHandle {
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");
    run_migrations(&mut db).expect("migrate");
    db
}

fn insert_v1_story(db: &DbHandle, id: &str) {
    // A legacy v1 row: schema_version 1 + the always-empty v1 structure.
    let json = "{\"schemaVersion\":1,\"nodes\":[]}";
    db.conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, 'Histoire', 1, ?2, ?3, '2026-06-10T00:00:00.000Z', '2026-06-10T00:00:00.000Z')",
            rusqlite::params![id, json, content_checksum(json)],
        )
        .expect("insert v1 story");
}

fn columns(db: &DbHandle, table: &str) -> Vec<String> {
    let mut stmt = db
        .conn()
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect")
}

#[test]
fn migration_v7_creates_assets_table_with_canonical_columns() {
    let tmp = TempDir::new().expect("tmp");
    let db = fresh_disk_db(&tmp);
    assert_eq!(
        columns(&db, "assets"),
        vec![
            "id",
            "story_id",
            "content_hash",
            "media_type",
            "media_format",
            "byte_size",
            "file_name",
            "created_at",
        ],
    );
}

#[test]
fn migration_v7_creates_node_drafts_table_with_canonical_columns() {
    let tmp = TempDir::new().expect("tmp");
    let db = fresh_disk_db(&tmp);
    assert_eq!(
        columns(&db, "node_drafts"),
        vec![
            "story_id",
            "node_id",
            "draft_text",
            "draft_label",
            "draft_at"
        ],
    );
}

/// Build a pre-v7 database (migrations 1..=6 applied, ledger marked) so the
/// re-stamp in v7 actually has a legacy row to upgrade.
fn pre_v7_db(path: &std::path::Path) {
    let db = open_at(path).expect("open initial");
    db.conn()
        .execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations ( \
               version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL )",
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

#[test]
fn migration_v7_restamps_legacy_v1_story_to_v2_single_node() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v7_db(&path);
    insert_v1_story(&open_at(&path).expect("reopen"), "legacy");

    // Newer binary boots and applies v7, re-stamping the v1 row.
    let mut db = open_at(&path).expect("reopen2");
    run_migrations(&mut db).expect("upgrade to v7");

    let (schema_version, structure_json, checksum): (u32, String, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, structure_json, content_checksum FROM stories WHERE id = 'legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("row");

    let expected_json = canonical_structure_json(&CanonicalStructure::minimal());
    assert_eq!(schema_version, 2);
    assert_eq!(structure_json, expected_json);
    // The migration's hardcoded checksum MUST equal the freshly computed one —
    // a drift in the canonical shape would surface here, not silently.
    assert_eq!(checksum, content_checksum(&expected_json));
}

#[test]
fn migration_v7_restamp_is_idempotent() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v7_db(&path);
    insert_v1_story(&open_at(&path).expect("reopen"), "legacy");

    let mut db = open_at(&path).expect("reopen2");
    run_migrations(&mut db).expect("first upgrade");
    let after_first: String = db
        .conn()
        .query_row(
            "SELECT content_checksum FROM stories WHERE id = 'legacy'",
            [],
            |row| row.get(0),
        )
        .expect("row");
    // A second run is a no-op (the version is in the ledger); the row is
    // unchanged and still v2.
    run_migrations(&mut db).expect("second upgrade");
    let (schema_version, after_second): (u32, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, content_checksum FROM stories WHERE id = 'legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row");
    assert_eq!(schema_version, 2);
    assert_eq!(after_first, after_second);
}

#[test]
fn assets_cascade_delete_on_story_removal() {
    let tmp = TempDir::new().expect("tmp");
    let db = fresh_disk_db(&tmp);
    insert_v1_story(&db, "s1");
    db.conn()
        .execute(
            "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
             VALUES ('a1', 's1', ?1, 'image', 'png', 12, 'a.png', '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["a".repeat(64)],
        )
        .expect("insert asset");
    db.conn()
        .execute("DELETE FROM stories WHERE id = 's1'", [])
        .expect("delete story");
    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE story_id = 's1'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(count, 0, "FK CASCADE must drop the asset rows");
}

#[test]
fn assets_reject_invalid_media_type_format_and_hash() {
    let tmp = TempDir::new().expect("tmp");
    let db = fresh_disk_db(&tmp);
    insert_v1_story(&db, "s1");
    let valid_hash = "a".repeat(64);

    // Unknown media_type.
    assert!(db
        .conn()
        .execute(
            "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
             VALUES ('a1', 's1', ?1, 'video', 'png', 1, 'a.png', '2026-06-27T00:00:00.000Z')",
            rusqlite::params![valid_hash],
        )
        .is_err());
    // Unknown media_format.
    assert!(db
        .conn()
        .execute(
            "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
             VALUES ('a2', 's1', ?1, 'image', 'gif', 1, 'a.gif', '2026-06-27T00:00:00.000Z')",
            rusqlite::params![valid_hash],
        )
        .is_err());
    // Bad checksum (uppercase hex breaks the GLOB guard).
    assert!(db
        .conn()
        .execute(
            "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
             VALUES ('a3', 's1', ?1, 'image', 'png', 1, 'a.png', '2026-06-27T00:00:00.000Z')",
            rusqlite::params!["A".repeat(64)],
        )
        .is_err());
}

#[test]
fn node_drafts_cascade_delete_on_story_removal() {
    let tmp = TempDir::new().expect("tmp");
    let db = fresh_disk_db(&tmp);
    insert_v1_story(&db, "s1");
    db.conn()
        .execute(
            "INSERT INTO node_drafts (story_id, node_id, draft_text, draft_label, draft_at) \
             VALUES ('s1', 'n1', 'texte', 'lab', '2026-06-27T00:00:00.000Z')",
            [],
        )
        .expect("insert node draft");
    db.conn()
        .execute("DELETE FROM stories WHERE id = 's1'", [])
        .expect("delete story");
    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM node_drafts WHERE story_id = 's1'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(count, 0, "FK CASCADE must drop the node draft");
}

#[test]
fn node_drafts_reject_orphan_story_id() {
    let tmp = TempDir::new().expect("tmp");
    let db = fresh_disk_db(&tmp);
    assert!(db
        .conn()
        .execute(
            "INSERT INTO node_drafts (story_id, node_id, draft_text, draft_label, draft_at) \
             VALUES ('ghost', 'n1', 't', 'l', '2026-06-27T00:00:00.000Z')",
            [],
        )
        .is_err());
}
