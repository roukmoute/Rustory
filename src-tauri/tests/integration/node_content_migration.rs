//! Integration tests for the v7 migration (`assets`, `node_drafts`, and the
//! v1→v2 re-stamp of existing stories) plus the FULL v1→v3 chain (0007 then
//! 0009).
//!
//! Exercises the on-disk SQLite path (TempDir) so the WAL + migration ledger
//! interactions match a real install. The re-stamp is a DATA migration (not
//! just DDL): a legacy v1 row must land on the current canonical shape with a
//! recomputed `content_checksum`, idempotently. The 0007 SQL itself is frozen
//! (its hardcoded checksum must keep matching its LITERAL v2 bytes), so the
//! v2 oracle below is inlined — never derived from the live `minimal()`,
//! which now produces v3.

use rustory_lib::domain::story::{canonical_structure_json, content_checksum, CanonicalStructure};
use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

/// The EXACT v2 bytes the 0007 SQL re-stamps onto every v1 row, inlined
/// LITERALLY. This constant is the guard that keeps the 0007 hardcoded
/// checksum in agreement with its frozen v2 byte shape — deriving it from
/// `CanonicalStructure::minimal()` would silently follow any later schema
/// bump and void the check.
const V2_RESTAMP_JSON: &str = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";

/// The checksum hardcoded in `0007_node_content_and_media.sql`.
const V2_RESTAMP_CHECKSUM: &str =
    "86077d78a039fc6e70ae076ff1dc9cce65ebda3d0c2a77de10502d2fee36b333";

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
fn migration_v7_hardcoded_checksum_matches_its_frozen_v2_bytes() {
    // The 0007 SQL could hardcode ONE checksum only because every v1 row was
    // re-stamped to the SAME v2 bytes. Both literals must still live verbatim
    // in the SQL file, and the checksum must still be the SHA-256 of those
    // exact bytes — the guard is anchored to the shipped file, not to any
    // live schema type.
    assert_eq!(content_checksum(V2_RESTAMP_JSON), V2_RESTAMP_CHECKSUM);
    let sql = include_str!("../../migrations/0007_node_content_and_media.sql");
    assert!(
        sql.contains(V2_RESTAMP_JSON),
        "0007 must re-stamp the frozen v2 byte shape verbatim"
    );
    assert!(
        sql.contains(V2_RESTAMP_CHECKSUM),
        "0007 must carry the checksum of its frozen v2 bytes"
    );
}

#[test]
fn migration_v7_sql_alone_restamps_a_v1_row_to_the_frozen_v2_bytes() {
    // Apply 0007 ISOLATED (not the full chain) so the v1→v2 step keeps its
    // own oracle: the literal v2 bytes + the hardcoded checksum.
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v7_db(&path);
    let db = open_at(&path).expect("reopen");
    insert_v1_story(&db, "legacy");

    db.conn()
        .execute_batch(include_str!(
            "../../migrations/0007_node_content_and_media.sql"
        ))
        .expect("apply 0007 alone");

    let (schema_version, structure_json, checksum): (u32, String, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, structure_json, content_checksum FROM stories WHERE id = 'legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("row");
    assert_eq!(schema_version, 2);
    assert_eq!(structure_json, V2_RESTAMP_JSON);
    assert_eq!(checksum, V2_RESTAMP_CHECKSUM);
    assert_eq!(checksum, content_checksum(&structure_json));
}

#[test]
fn full_migration_chain_lands_a_legacy_v1_row_on_v3() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v7_db(&path);
    insert_v1_story(&open_at(&path).expect("reopen"), "legacy");

    // Newer binary boots and applies the whole chain: 0007 (v1→v2, SQL) then
    // 0009 (v2→v3, Rust hook).
    let mut db = open_at(&path).expect("reopen2");
    run_migrations(&mut db).expect("upgrade to v3");

    let (schema_version, structure_json, checksum): (u32, String, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, structure_json, content_checksum FROM stories WHERE id = 'legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("row");

    // A v1 row was empty, so its final v3 form IS the minimal v3 structure.
    let expected_json = canonical_structure_json(&CanonicalStructure::minimal());
    assert_eq!(schema_version, 3);
    assert_eq!(structure_json, expected_json);
    assert_eq!(checksum, content_checksum(&expected_json));
}

#[test]
fn full_migration_chain_is_idempotent() {
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
    // A second run is a no-op (every version is in the ledger); the row is
    // unchanged and still v3.
    run_migrations(&mut db).expect("second upgrade");
    let (schema_version, after_second): (u32, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, content_checksum FROM stories WHERE id = 'legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row");
    assert_eq!(schema_version, 3);
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
