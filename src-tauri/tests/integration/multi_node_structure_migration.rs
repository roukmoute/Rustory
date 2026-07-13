//! Integration tests for the v9 migration: the v2→v3 re-stamp carried by a
//! RUST HOOK (`restamp_v2_to_v3`) inside the migration transaction.
//!
//! v2 rows carry VARIED content (text / label / media references), so the
//! bump changes each row's bytes differently and the `content_checksum` must
//! be recomputed PER ROW — the whole reason this migration cannot be pure
//! SQL. The tests below seed several v2 rows with DIFFERENT contents and
//! verify each lands on v3 losslessly with a checksum the test recomputes
//! itself; plus idempotence, the 1..9 ledger, and the fail-closed behavior on
//! an unreadable / unpromotable v2 row.

use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::domain::story::{content_checksum, CanonicalStructure};
use rustory_lib::infrastructure::db::{open_at, run_migrations, MIGRATIONS};
use tempfile::TempDir;

/// Build a pre-v9 database (migrations 1..=8 applied, ledger marked) so the
/// v2→v3 re-stamp actually has legacy rows to promote.
fn pre_v9_db(path: &std::path::Path) {
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
        (
            7,
            include_str!("../../migrations/0007_node_content_and_media.sql"),
        ),
        (
            8,
            include_str!("../../migrations/0008_assets_content_hash_index.sql"),
        ),
    ] {
        db.conn().execute_batch(file).expect("apply migration");
        db.conn()
            .execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, '2026-07-04T00:00:00Z')",
                rusqlite::params![version],
            )
            .expect("record migration");
    }
}

fn insert_v2_story(path: &std::path::Path, id: &str, structure_json: &str) {
    let db = open_at(path).expect("open for seed");
    db.conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, 'Histoire', 2, ?2, ?3, '2026-07-04T00:00:00.000Z', '2026-07-04T00:00:00.000Z')",
            rusqlite::params![id, structure_json, content_checksum(structure_json)],
        )
        .expect("insert v2 story");
}

/// Three v2 rows with DIFFERENT contents: an empty start node, a node full of
/// text/label/media, and a node with a non-"n1" id (an imported artifact may
/// carry any id). Each produces DIFFERENT v3 bytes, hence a different
/// checksum — the per-row recompute is what this migration exists for.
const V2_EMPTY: &str = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";
const V2_FILLED: &str = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"Il était une fois…\",\"label\":\"Début\",\"imageAssetId\":\"asset-img\",\"audioAssetId\":\"asset-aud\"}]}";
const V2_OTHER_ID: &str = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"noeud-7\",\"text\":\"Autre départ\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";

#[test]
fn migration_v9_restamps_varied_v2_rows_to_v3_with_per_row_checksums() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v9_db(&path);
    insert_v2_story(&path, "s-empty", V2_EMPTY);
    insert_v2_story(&path, "s-filled", V2_FILLED);
    insert_v2_story(&path, "s-other-id", V2_OTHER_ID);

    let mut db = open_at(&path).expect("reopen");
    run_migrations(&mut db).expect("upgrade to v9");

    for (id, expected_start, expected_text, expected_label, expected_img, expected_aud) in [
        ("s-empty", "n1", "", "", None::<&str>, None::<&str>),
        (
            "s-filled",
            "n1",
            "Il était une fois…",
            "Début",
            Some("asset-img"),
            Some("asset-aud"),
        ),
        ("s-other-id", "noeud-7", "Autre départ", "", None, None),
    ] {
        let (schema_version, structure_json, checksum): (u32, String, String) = db
            .conn()
            .query_row(
                "SELECT schema_version, structure_json, content_checksum FROM stories WHERE id = ?1",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("row");

        assert_eq!(schema_version, 3, "{id}: column must be bumped to 3");
        // The row must parse through the LIVE v3 type.
        let structure: CanonicalStructure =
            serde_json::from_str(&structure_json).expect("v3 parse");
        assert_eq!(structure.schema_version, 3, "{id}");
        assert_eq!(structure.start_node_id, expected_start, "{id}: start id");
        assert_eq!(structure.nodes.len(), 1, "{id}");
        let node = &structure.nodes[0];
        assert_eq!(node.id, expected_start, "{id}: node id preserved");
        assert_eq!(node.text, expected_text, "{id}: text preserved");
        assert_eq!(node.label, expected_label, "{id}: label preserved");
        assert_eq!(node.image_asset_id.as_deref(), expected_img, "{id}");
        assert_eq!(node.audio_asset_id.as_deref(), expected_aud, "{id}");
        assert!(node.options.is_empty(), "{id}: options start empty");
        // The checksum was recomputed PER ROW on the new bytes.
        assert_eq!(
            checksum,
            content_checksum(&structure_json),
            "{id}: checksum must match the re-stamped bytes"
        );
    }
}

#[test]
fn migration_v9_is_idempotent() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v9_db(&path);
    insert_v2_story(&path, "s-filled", V2_FILLED);

    let mut db = open_at(&path).expect("reopen");
    run_migrations(&mut db).expect("first upgrade");
    let (v_first, json_first, sum_first): (u32, String, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, structure_json, content_checksum FROM stories WHERE id = 's-filled'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("row");
    run_migrations(&mut db).expect("second upgrade");
    let (v_second, json_second, sum_second): (u32, String, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, structure_json, content_checksum FROM stories WHERE id = 's-filled'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("row");
    assert_eq!(v_first, 3);
    assert_eq!(v_second, 3);
    assert_eq!(json_first, json_second);
    assert_eq!(sum_first, sum_second);
}

#[test]
fn migration_ledger_records_every_declared_version() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open");
    run_migrations(&mut db).expect("migrate");

    let mut stmt = db
        .conn()
        .prepare("SELECT version FROM schema_migrations ORDER BY version")
        .expect("prepare");
    let versions: Vec<u32> = stmt
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");
    assert_eq!(
        versions,
        (1..=MIGRATIONS.len() as u32).collect::<Vec<u32>>()
    );
    assert_eq!(versions.len(), MIGRATIONS.len());
}

#[test]
fn migration_v9_fails_closed_on_an_unreadable_v2_row() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v9_db(&path);
    // A v2 COLUMN whose JSON does not parse as the exact v2 shape: corrupt.
    let corrupt_json = "{\"schemaVersion\":2,\"nodes\":[{\"unexpected\":true}]}";
    {
        let db = open_at(&path).expect("open for seed");
        db.conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES ('s-corrupt', 'Histoire', 2, ?1, ?2, '2026-07-04T00:00:00.000Z', '2026-07-04T00:00:00.000Z')",
                rusqlite::params![corrupt_json, content_checksum(corrupt_json)],
            )
            .expect("insert corrupt v2 story");
    }

    let mut db = open_at(&path).expect("reopen");
    let err = run_migrations(&mut db).expect_err("an unreadable v2 row must abort the migration");
    assert_eq!(err.code, AppErrorCode::LocalStorageUnavailable);
    let details = err.details.as_ref().expect("details");
    assert_eq!(details["source"], "sqlite_migration");
    assert_eq!(details["stage"], "restamp_parse");
    assert_eq!(details["version"], 9);

    // Fail-closed: the transaction rolled back — the row is untouched and the
    // ledger does NOT record version 9.
    let (schema_version, structure_json): (u32, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, structure_json FROM stories WHERE id = 's-corrupt'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row");
    assert_eq!(schema_version, 2, "row must stay v2 after the rollback");
    assert_eq!(structure_json, corrupt_json, "bytes must be untouched");
    let v9_recorded: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 9",
            [],
            |row| row.get(0),
        )
        .expect("ledger");
    assert_eq!(
        v9_recorded, 0,
        "a failed migration must not enter the ledger"
    );
}

#[test]
fn migration_v9_fails_closed_on_an_unpromotable_v2_row() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v9_db(&path);
    // Parses as v2 but has ZERO nodes: no definable start node — promoting
    // would mean inventing one (the forbidden silent repair).
    let empty_nodes = "{\"schemaVersion\":2,\"nodes\":[]}";
    {
        let db = open_at(&path).expect("open for seed");
        db.conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES ('s-empty-nodes', 'Histoire', 2, ?1, ?2, '2026-07-04T00:00:00.000Z', '2026-07-04T00:00:00.000Z')",
                rusqlite::params![empty_nodes, content_checksum(empty_nodes)],
            )
            .expect("insert empty-nodes v2 story");
    }

    let mut db = open_at(&path).expect("reopen");
    let err = run_migrations(&mut db).expect_err("an unpromotable v2 row must abort the migration");
    let details = err.details.as_ref().expect("details");
    assert_eq!(details["stage"], "restamp_promote");
    assert_eq!(details["version"], 9);
}

#[test]
fn migration_v9_fails_closed_on_a_diverging_v2_checksum() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v9_db(&path);
    // Parsable v2 bytes, but the STORED checksum already diverges: the row is
    // corrupt. Promoting it and recomputing a fresh checksum would ERASE the
    // integrity signal — the migration must fail closed instead.
    {
        let db = open_at(&path).expect("open for seed");
        db.conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES ('s-diverged', 'Histoire', 2, ?1, ?2, '2026-07-04T00:00:00.000Z', '2026-07-04T00:00:00.000Z')",
                rusqlite::params![V2_FILLED, "0".repeat(64)],
            )
            .expect("insert diverged v2 story");
    }

    let mut db = open_at(&path).expect("reopen");
    let err = run_migrations(&mut db).expect_err("a diverged v2 row must abort the migration");
    let details = err.details.as_ref().expect("details");
    assert_eq!(details["stage"], "restamp_checksum");
    assert_eq!(details["version"], 9);

    // Fail-closed: the row keeps its v2 bytes AND its diverging checksum (the
    // corruption evidence survives), the ledger does not record version 9.
    let (schema_version, structure_json, checksum): (u32, String, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, structure_json, content_checksum FROM stories WHERE id = 's-diverged'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("row");
    assert_eq!(schema_version, 2);
    assert_eq!(structure_json, V2_FILLED);
    assert_eq!(checksum, "0".repeat(64));
    let v9_recorded: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 9",
            [],
            |row| row.get(0),
        )
        .expect("ledger");
    assert_eq!(v9_recorded, 0);
}

#[test]
fn migration_v9_fails_closed_when_the_promoted_graph_is_blocked() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v9_db(&path);
    // Parses as v2, single node, coherent checksum — but the node id is
    // BLANK: the mechanical promotion would land a v3 the app refuses to
    // project (StructureCorrupt + StartNodeInvalid, both Blocking) while the
    // ledger says the migration succeeded. The re-validation of the promoted
    // graph must abort instead.
    let blank_id = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"   \",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";
    {
        let db = open_at(&path).expect("open for seed");
        db.conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES ('s-blank-id', 'Histoire', 2, ?1, ?2, '2026-07-04T00:00:00.000Z', '2026-07-04T00:00:00.000Z')",
                rusqlite::params![blank_id, content_checksum(blank_id)],
            )
            .expect("insert blank-id v2 story");
    }

    let mut db = open_at(&path).expect("reopen");
    let err =
        run_migrations(&mut db).expect_err("a blocked promoted graph must abort the migration");
    let details = err.details.as_ref().expect("details");
    assert_eq!(details["stage"], "restamp_invalid");
    assert_eq!(details["version"], 9);

    // Rollback: the row is untouched (still the blank-id v2 bytes).
    let (schema_version, structure_json): (u32, String) = db
        .conn()
        .query_row(
            "SELECT schema_version, structure_json FROM stories WHERE id = 's-blank-id'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row");
    assert_eq!(schema_version, 2, "row must stay v2 after the rollback");
    assert_eq!(structure_json, blank_id, "bytes must be untouched");
}

#[test]
fn migration_v9_fails_closed_on_a_forged_multi_node_v2_row() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("rustory.sqlite");
    pre_v9_db(&path);
    // Parses as v2 but carries TWO nodes: the v2 model carried EXACTLY one —
    // promoting a forged multi-node payload would silently repair a corrupt
    // row into a "healthy" v3 graph.
    let multi = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null},{\"id\":\"n2\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";
    {
        let db = open_at(&path).expect("open for seed");
        db.conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES ('s-multi', 'Histoire', 2, ?1, ?2, '2026-07-04T00:00:00.000Z', '2026-07-04T00:00:00.000Z')",
                rusqlite::params![multi, content_checksum(multi)],
            )
            .expect("insert forged multi-node v2 story");
    }

    let mut db = open_at(&path).expect("reopen");
    let err = run_migrations(&mut db).expect_err("a forged v2 row must abort the migration");
    let details = err.details.as_ref().expect("details");
    assert_eq!(details["stage"], "restamp_promote");
    assert_eq!(details["version"], 9);
}
