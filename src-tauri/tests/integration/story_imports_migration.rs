//! Integration tests for the v3 migration that introduces `story_imports`.
//!
//! Mirror of `story_drafts_migration.rs`: exercises the on-disk SQLite
//! path (TempDir) so the WAL and migration ledger interactions match a
//! real install. `story_imports` is the durable provenance link
//! `pack_uuid ↔ story_id` of the device-import flow; its UNIQUE index on
//! `pack_uuid` is the DB-level lock behind "re-import bloqué".

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

fn insert_import(db: &DbHandle, story_id: &str, pack_uuid: &str) -> Result<usize, rusqlite::Error> {
    db.conn().execute(
        "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
         VALUES (?1, ?2, 'deadbeefdeadbeefdeadbeefdeadbeef', '2026-06-10T00:00:00.000Z', 8, 4096, ?3)",
        rusqlite::params![story_id, pack_uuid, "a".repeat(64)],
    )
}

const PACK_UUID: &str = "12345678-9abc-def0-1122-334455667788";

#[test]
fn fresh_install_applies_v3_migration_with_canonical_columns() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);

    let mut stmt = db
        .conn()
        .prepare("PRAGMA table_info(story_imports)")
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
            "pack_uuid",
            "source_device_identifier",
            "imported_at",
            "pack_file_count",
            "pack_total_bytes",
            "pack_checksum",
        ],
        "schema must match the canonical column set"
    );

    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 3",
            [],
            |row| row.get(0),
        )
        .expect("ledger");
    assert_eq!(count, 1, "v3 must be recorded in the ledger");
}

#[test]
fn idempotency_v3() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");

    run_migrations(&mut db).expect("first apply");
    run_migrations(&mut db).expect("second apply");

    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 3",
            [],
            |row| row.get(0),
        )
        .expect("count v3");
    assert_eq!(count, 1, "v3 must be recorded exactly once");
}

#[test]
fn unique_pack_uuid_index_blocks_a_second_import_of_the_same_pack() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-a");
    insert_story(&db, "story-b");

    insert_import(&db, "story-a", PACK_UUID).expect("first import row");
    let err = insert_import(&db, "story-b", PACK_UUID).expect_err("same pack_uuid must fail");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("unique"),
        "expected UNIQUE violation on pack_uuid, got: {message}"
    );
}

#[test]
fn cascade_delete_removes_the_provenance_link_with_the_story() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-cascade");
    insert_import(&db, "story-cascade", PACK_UUID).expect("import row");

    db.conn()
        .execute("DELETE FROM stories WHERE id = 'story-cascade'", [])
        .expect("delete parent");

    let count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM story_imports", [], |row| row.get(0))
        .expect("count");
    assert_eq!(
        count, 0,
        "FK ON DELETE CASCADE must remove the provenance link — re-opening the copy right"
    );
}

#[test]
fn rejects_orphan_story_id_via_fk() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    let err = insert_import(&db, "orphan", PACK_UUID).expect_err("orphan must be rejected");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("foreign key"),
        "expected FK constraint failure, got: {message}"
    );
}

#[test]
fn check_constraints_guard_uuid_count_and_checksum_shapes() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "story-checks");

    // pack_uuid must be the 36-char canonical form.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
             VALUES ('story-checks', 'short-uuid', 'id', '2026-06-10T00:00:00.000Z', 1, 0, ?1)",
            rusqlite::params!["a".repeat(64)],
        )
        .expect_err("non-canonical pack_uuid must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // pack_file_count must be >= 1 (an imported pack has at least the
    // four required files).
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
             VALUES ('story-checks', ?1, 'id', '2026-06-10T00:00:00.000Z', 0, 0, ?2)",
            rusqlite::params![PACK_UUID, "a".repeat(64)],
        )
        .expect_err("zero file count must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // pack_checksum must be a 64-char SHA-256 hex digest.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
             VALUES ('story-checks', ?1, 'id', '2026-06-10T00:00:00.000Z', 1, 0, 'abc')",
            rusqlite::params![PACK_UUID],
        )
        .expect_err("short checksum must trip CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn existing_v2_database_upgrades_to_v3() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    // First boot: mimic a v2-aware install (v1 + v2 applied by hand).
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
        db.conn()
            .execute_batch(include_str!("../../migrations/0001_init.sql"))
            .expect("apply v1");
        db.conn()
            .execute_batch(include_str!("../../migrations/0002_story_drafts.sql"))
            .expect("apply v2");
        db.conn()
            .execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES \
                 (1, '2026-04-22T00:00:00Z'), (2, '2026-04-25T00:00:00Z')",
                [],
            )
            .expect("record v1+v2");
    }

    // Second boot: the newer binary adds v3 without touching v1/v2 rows.
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
        "v1+v2 preserved, v3 added (and later migrations recorded)"
    );

    insert_story(&db, "story-upgraded");
    insert_import(&db, "story-upgraded", PACK_UUID).expect("table usable after upgrade");
}
