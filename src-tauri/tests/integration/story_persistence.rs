use rustory_lib::application::story::{create_story, CreateStoryInput};
use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::infrastructure::db;
use rustory_lib::infrastructure::filesystem::resolve_db_path;
use tempfile::TempDir;

/// Drop the handle, rebuild a fresh one on the same file path, and confirm
/// that the story created in the first run is still present and intact.
/// Exercises the "reopenable without device dependency" acceptance
/// criterion end to end.
#[test]
fn reopens_persisted_story_after_restart_sim() {
    let tmp = TempDir::new().expect("tempdir");
    let path = resolve_db_path(tmp.path());

    let (expected_id, expected_title) = {
        let mut db = db::open_at(&path).expect("open first");
        db::run_migrations(&mut db).expect("migrate first");
        let dto = create_story(
            &mut db,
            CreateStoryInput {
                title: "Brouillon persistant".into(),
            },
        )
        .expect("create");
        (dto.id, dto.title)
    }; // `db` dropped here; WAL flushed, file closed cleanly.

    let db = db::open_at(&path).expect("reopen");

    let (id, title, schema_version): (String, String, u32) = db
        .conn()
        .query_row("SELECT id, title, schema_version FROM stories", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .expect("persisted row");

    assert_eq!(id, expected_id);
    assert_eq!(title, expected_title);
    assert_eq!(schema_version, 1);
}

/// Re-running `run_migrations` on an already-initialized database must be
/// a no-op and must not disturb user rows.
#[test]
fn stories_survive_migration_rerun() {
    let mut db = db::open_in_memory().expect("open");
    db::run_migrations(&mut db).expect("first migrate");
    create_story(
        &mut db,
        CreateStoryInput {
            title: "Survivante".into(),
        },
    )
    .expect("create");

    db::run_migrations(&mut db).expect("second migrate");

    let count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 1);

    let ledger_rows: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("count ledger");
    assert_eq!(ledger_rows, 1);
}

/// An invalid title must be refused before any SQL mutation: the base
/// must be observably empty immediately after the rejection.
#[test]
fn creation_is_atomic_under_invalid_input() {
    let mut db = db::open_in_memory().expect("open");
    db::run_migrations(&mut db).expect("migrate");

    let err = create_story(
        &mut db,
        CreateStoryInput {
            title: "   ".into(),
        },
    )
    .expect_err("must fail");
    assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);

    let count: u32 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 0, "no row must be inserted on validation failure");
}
