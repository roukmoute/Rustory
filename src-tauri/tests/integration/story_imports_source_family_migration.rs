//! Integration tests for the v12 migration: `story_imports` becomes
//! FAMILY-AWARE (`source_family`, closed set, backfilled `'lunii'`).
//!
//! v12 reuses the v10/v11 REBUILD-for-CHECK recipe (SQLite cannot add a
//! CHECK in place): CREATE new / INSERT..SELECT / DROP / RENAME, executed
//! with `foreign_keys=ON`, then the UNIQUE content-identity index is
//! recreated verbatim. These tests prove the recipe preserves every
//! pre-v12 row byte-for-byte with the `'lunii'` backfill, keeps every
//! other constraint (CHECKs, FK cascade, UNIQUE pack identity) alive on
//! the rebuilt table, and refuses any family outside the closed set.

use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

/// The canonical v3 minimal structure + its exact checksum (the same frozen
/// fixture pair the sibling migration tests use).
const V3_MINIMAL: &str = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
const V3_MINIMAL_CHECKSUM: &str =
    "65d663fd2180630fa24693a5ccaee6d663b7a0f78b7d44b0e5ef07adc3f293b2";

const PACK_UUID_A: &str = "abababab-abab-abab-abab-ababfac5562d";
const PACK_UUID_B: &str = "12345678-9abc-def0-1122-334455667788";

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
             VALUES (?1, 'Histoire', 3, ?2, ?3, '2026-07-13T00:00:00.000Z', '2026-07-13T00:00:00.000Z')",
            rusqlite::params![id, V3_MINIMAL, V3_MINIMAL_CHECKSUM],
        )
        .expect("insert parent story");
}

fn insert_import_with_family(
    db: &DbHandle,
    story_id: &str,
    pack_uuid: &str,
    family: &str,
) -> Result<usize, rusqlite::Error> {
    db.conn().execute(
        "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
         VALUES (?1, ?2, 'devhash', '2026-07-13T00:00:00.000Z', 3, 448, ?3, ?4)",
        rusqlite::params![story_id, pack_uuid, "a".repeat(64), family],
    )
}

#[test]
fn pre_v12_rows_are_preserved_and_backfilled_lunii() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    // First boot: a v11-level install (v1..v11 applied by hand, ledger marked
    // — v9's Rust hook is a no-op on a base with no v2 story).
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
            (
                10,
                include_str!("../../migrations/0010_import_review_resolution.sql"),
            ),
            (
                11,
                include_str!("../../migrations/0011_structured_folder_provenance.sql"),
            ),
        ] {
            db.conn().execute_batch(file).expect("apply migration");
            db.conn()
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, '2026-07-13T00:00:00Z')",
                    rusqlite::params![version],
                )
                .expect("record migration");
        }
        insert_story(&db, "s-lunii");
        // Pre-v12 shape: no `source_family` column exists yet.
        db.conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
                 VALUES ('s-lunii', ?1, 'devhash', '2026-06-10T00:00:00.000Z', 7, 7168, ?2)",
                rusqlite::params![PACK_UUID_A, "b".repeat(64)],
            )
            .expect("seed pre-v12 row");
    }

    // Second boot: the newer binary rebuilds the table through v12.
    let mut db = open_at(&path).expect("reopen");
    run_migrations(&mut db).expect("upgrade");

    let row: (String, String, String, String, u32, u64, String, String) = db
        .conn()
        .query_row(
            "SELECT story_id, pack_uuid, source_device_identifier, imported_at, \
                    pack_file_count, pack_total_bytes, pack_checksum, source_family \
             FROM story_imports",
            [],
            |r| {
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
            },
        )
        .expect("the pre-v12 row survives the rebuild");
    assert_eq!(row.0, "s-lunii");
    assert_eq!(row.1, PACK_UUID_A);
    assert_eq!(row.2, "devhash");
    assert_eq!(row.3, "2026-06-10T00:00:00.000Z");
    assert_eq!(row.4, 7);
    assert_eq!(row.5, 7168);
    assert_eq!(row.6, "b".repeat(64));
    // The backfill: every pre-v12 device import could only be a Lunii.
    assert_eq!(row.7, "lunii");
}

#[test]
fn flam_family_is_accepted_and_an_unknown_family_is_refused() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-flam");
    insert_import_with_family(&db, "s-flam", PACK_UUID_B, "flam")
        .expect("the closed set accepts 'flam'");

    let family: String = db
        .conn()
        .query_row(
            "SELECT source_family FROM story_imports WHERE story_id = 's-flam'",
            [],
            |r| r.get(0),
        )
        .expect("row");
    assert_eq!(family, "flam");

    insert_story(&db, "s-tonies");
    let err = insert_import_with_family(&db, "s-tonies", PACK_UUID_A, "tonies")
        .expect_err("a family outside the closed set must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn an_insert_without_a_family_is_refused_by_the_schema() {
    // NO implicit family: the column has no DEFAULT, so an INSERT that
    // forgets it FAILS instead of silently recording 'lunii' — the exact
    // fail-open path the family-aware provenance exists to close (a
    // non-Lunii pack recorded 'lunii' would become transferable toward a
    // Lunii). The historical backfill is carried by the migration's own
    // INSERT…SELECT, not by a live default.
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-nofamily");
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
             VALUES ('s-nofamily', ?1, 'devhash', '2026-07-13T00:00:00.000Z', 1, 16, ?2)",
            rusqlite::params![PACK_UUID_A, "c".repeat(64)],
        )
        .expect_err("an INSERT without source_family must be refused");
    assert!(err.to_string().to_lowercase().contains("not null"));
}

#[test]
fn unique_pack_identity_index_survives_the_rebuild() {
    // The UNIQUE `pack_uuid` index does not survive a DROP TABLE by itself:
    // v12 recreates it, and the `already_imported` dedup stands on it.
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-one");
    insert_story(&db, "s-two");
    insert_import_with_family(&db, "s-one", PACK_UUID_A, "flam").expect("first row");
    let err = insert_import_with_family(&db, "s-two", PACK_UUID_A, "flam")
        .expect_err("the same pack_uuid must trip the UNIQUE index");
    assert!(err.to_string().to_lowercase().contains("unique"));
}

#[test]
fn cascade_delete_survives_the_rebuild() {
    // The rebuilt table re-declares the FK: deleting the parent story must
    // still cascade onto the provenance row (foreign_keys=ON at open).
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-cascade");
    insert_import_with_family(&db, "s-cascade", PACK_UUID_B, "flam").expect("seed");

    db.conn()
        .execute("DELETE FROM stories WHERE id = 's-cascade'", [])
        .expect("delete parent");

    let count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM story_imports", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 0, "ON DELETE CASCADE must survive the rebuild");
}

#[test]
fn every_other_historical_check_survives_the_rebuild() {
    // The v12 rebuild must carry every other 0003 CHECK verbatim: probe one
    // representative refusal per constraint on the rebuilt table.
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-checks");

    // pack_uuid length gate.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
             VALUES ('s-checks', 'short', 'devhash', '2026-07-13T00:00:00.000Z', 1, 16, ?1, 'flam')",
            rusqlite::params!["a".repeat(64)],
        )
        .expect_err("a non-36-char pack_uuid must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // pack_file_count floor.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
             VALUES ('s-checks', ?1, 'devhash', '2026-07-13T00:00:00.000Z', 0, 16, ?2, 'flam')",
            rusqlite::params![PACK_UUID_B, "a".repeat(64)],
        )
        .expect_err("a zero file count must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // pack_checksum length gate.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
             VALUES ('s-checks', ?1, 'devhash', '2026-07-13T00:00:00.000Z', 1, 16, 'tooshort', 'flam')",
            rusqlite::params![PACK_UUID_B],
        )
        .expect_err("a short checksum must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn idempotency_v12() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");

    run_migrations(&mut db).expect("first apply");
    run_migrations(&mut db).expect("second apply");

    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 12",
            [],
            |row| row.get(0),
        )
        .expect("count v12");
    assert_eq!(count, 1, "v12 must be recorded exactly once");
}
