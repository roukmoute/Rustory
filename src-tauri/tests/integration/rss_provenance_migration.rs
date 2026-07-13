//! Integration tests for the v13 migration: `story_local_imports` accepts
//! the `rss` source format (the external-source creation flow).
//!
//! v13 reuses the v10/v11 REBUILD-for-CHECK recipe (SQLite cannot alter a
//! CHECK in place): CREATE new / INSERT..SELECT / DROP / RENAME, executed
//! with `foreign_keys=ON`. These tests prove the recipe preserves every
//! pre-v13 row byte-for-byte, keeps every other constraint alive on the
//! rebuilt table, and only widens the `source_format` set.

use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

/// The canonical v3 minimal structure + its exact checksum (the same frozen
/// fixture pair the sibling migration tests use).
const V3_MINIMAL: &str = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
const V3_MINIMAL_CHECKSUM: &str =
    "65d663fd2180630fa24693a5ccaee6d663b7a0f78b7d44b0e5ef07adc3f293b2";

const FINDINGS: &str = "[{\"aspect\":\"source\",\"category\":\"ambiguous\"}]";

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

fn insert_import(
    db: &DbHandle,
    story_id: &str,
    format: &str,
    state: &str,
    summary: Option<&str>,
) -> Result<usize, rusqlite::Error> {
    db.conn().execute(
        "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
         VALUES (?1, ?2, 1, 'exemple.fr', ?3, ?4, ?5, '2026-07-13T00:00:00.000Z')",
        rusqlite::params![story_id, format, "a".repeat(64), state, summary],
    )
}

#[test]
fn pre_v13_rows_of_both_prior_formats_are_preserved_row_by_row() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    // First boot: a v12-level install (v1..v12 applied by hand, ledger
    // marked — v9's Rust hook is a no-op on a base with no v2 story).
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
            (
                12,
                include_str!("../../migrations/0012_story_imports_source_family.sql"),
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
        insert_story(&db, "s-rustory");
        insert_story(&db, "s-folder");
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES ('s-rustory', 'rustory', 1, 'histoire.rustory', ?1, 'resolved', ?2, '2026-07-13T00:00:00.000Z')",
                rusqlite::params!["b".repeat(64), FINDINGS],
            )
            .expect("seed pre-v13 rustory row");
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES ('s-folder', 'structured-folder', 1, 'mon-dossier', ?1, 'partial', ?2, '2026-07-13T00:00:00.000Z')",
                rusqlite::params!["c".repeat(64), FINDINGS],
            )
            .expect("seed pre-v13 folder row");
    }

    // Second boot: the newer binary rebuilds the table through v13.
    let mut db = open_at(&path).expect("reopen");
    run_migrations(&mut db).expect("upgrade");

    let rows: Vec<(String, String, String, Option<String>)> = db
        .conn()
        .prepare(
            "SELECT story_id, source_format, import_state, findings_summary \
             FROM story_local_imports ORDER BY story_id",
        )
        .expect("prepare")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert_eq!(
        rows,
        vec![
            (
                "s-folder".to_string(),
                "structured-folder".to_string(),
                "partial".to_string(),
                Some(FINDINGS.to_string()),
            ),
            (
                "s-rustory".to_string(),
                "rustory".to_string(),
                "resolved".to_string(),
                Some(FINDINGS.to_string()),
            ),
        ],
        "both pre-v13 rows survive the rebuild byte-for-byte"
    );
}

#[test]
fn rss_format_is_accepted() {
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-rss");

    insert_import(&db, "s-rss", "rss", "needs_review", Some(FINDINGS))
        .expect("the widened CHECK accepts the rss format");

    let (format, state): (String, String) = db
        .conn()
        .query_row(
            "SELECT source_format, import_state FROM story_local_imports WHERE story_id = 's-rss'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row");
    assert_eq!(format, "rss");
    assert_eq!(state, "needs_review");
}

#[test]
fn unknown_format_is_still_refused_after_the_rebuild() {
    // The explicit-format guarantee is NOT weakened by widening: anything
    // outside the listed set still trips the CHECK.
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-atom");

    let err = insert_import(&db, "s-atom", "atom", "needs_review", Some(FINDINGS))
        .expect_err("an unknown source_format must trip the rebuilt CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn every_other_historical_check_survives_the_rebuild() {
    // The v13 rebuild must carry every other 0006/0010/0011 CHECK verbatim:
    // probe one representative refusal per constraint on the rebuilt table.
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-checks");

    // recognized ⟺ NULL report (both directions).
    let err = insert_import(&db, "s-checks", "rss", "recognized", Some(FINDINGS))
        .expect_err("recognized with a report must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
    let err = insert_import(&db, "s-checks", "rss", "needs_review", None)
        .expect_err("needs_review without a report must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // blocked is still never persistable.
    let err = insert_import(&db, "s-checks", "rss", "blocked", Some(FINDINGS))
        .expect_err("blocked must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));

    // The checksum shape gate (64 lowercase hex) is alive.
    let err = db
        .conn()
        .execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES ('s-checks', 'rss', 1, 'exemple.fr', ?1, 'recognized', NULL, '2026-07-13T00:00:00.000Z')",
            rusqlite::params!["Z".repeat(64)],
        )
        .expect_err("a non-hex checksum must trip the CHECK");
    assert!(err.to_string().to_lowercase().contains("check"));
}

#[test]
fn cascade_delete_survives_the_rebuild() {
    // The rebuilt table re-declares the FK: deleting the parent story must
    // still cascade onto the provenance row (foreign_keys=ON at open).
    let tmp = TempDir::new().expect("tempdir");
    let db = fresh_disk_db(&tmp);
    insert_story(&db, "s-cascade");
    insert_import(&db, "s-cascade", "rss", "needs_review", Some(FINDINGS)).expect("seed");

    db.conn()
        .execute("DELETE FROM stories WHERE id = 's-cascade'", [])
        .expect("delete parent");

    let orphan_count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM story_local_imports", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        orphan_count, 0,
        "ON DELETE CASCADE must survive the rebuild"
    );
}

#[test]
fn idempotency_v13() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");

    run_migrations(&mut db).expect("first apply");
    run_migrations(&mut db).expect("second apply");

    let applied: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 13",
            [],
            |row| row.get(0),
        )
        .expect("count v13");
    assert_eq!(applied, 1, "v13 must be recorded exactly once");
}
