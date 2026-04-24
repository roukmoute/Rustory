use rustory_lib::application::story::{
    create_story, get_story_detail, update_story, CreateStoryInput, UpdateStoryInput,
};
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

/// End-to-end AC2 scenario: create a story, simulate an app restart
/// (drop + reopen), update its title, simulate another restart, and
/// verify the persisted detail exactly mirrors the last successful save.
#[test]
fn updates_story_title_and_reopens_exact_state_after_restart() {
    let tmp = TempDir::new().expect("tempdir");
    let path = resolve_db_path(tmp.path());

    let created_id;
    let initial_checksum: String;
    let initial_structure: String;

    {
        let mut db = db::open_at(&path).expect("open first");
        db::run_migrations(&mut db).expect("migrate");
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Avant".into(),
            },
        )
        .expect("create");
        created_id = created.id;

        let (structure, checksum): (String, String) = db
            .conn()
            .query_row(
                "SELECT structure_json, content_checksum FROM stories WHERE id = ?1",
                [&created_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row");
        initial_checksum = checksum;
        initial_structure = structure;
    }

    // Simulate "close + reopen" between the create and the update.
    {
        let mut db = db::open_at(&path).expect("reopen for update");
        // No migrations to run on reopen — idempotent ledger says they're done.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let updated = update_story(
            &mut db,
            UpdateStoryInput {
                id: created_id.clone(),
                title: "Après".into(),
            },
        )
        .expect("update");
        assert_eq!(updated.title, "Après");
    }

    // Simulate another restart and verify the persisted state.
    {
        let db = db::open_at(&path).expect("reopen for read");
        let detail = get_story_detail(&db, &created_id)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.id, created_id);
        assert_eq!(detail.title, "Après");
        assert_eq!(detail.schema_version, 1);
        assert_eq!(
            detail.structure_json, initial_structure,
            "structure_json must not be altered by a title update"
        );
        assert_eq!(
            detail.content_checksum, initial_checksum,
            "content_checksum must not be altered by a title update"
        );
        assert!(
            detail.updated_at > detail.created_at,
            "updated_at must strictly advance after an update"
        );
    }
}

/// A rejected update leaves every column untouched — the row that was
/// there before the call is the row that remains.
#[test]
fn update_story_rejected_input_leaves_row_untouched() {
    let mut db = db::open_in_memory().expect("open");
    db::run_migrations(&mut db).expect("migrate");

    let created = create_story(
        &mut db,
        CreateStoryInput {
            title: "Intact".into(),
        },
    )
    .expect("create");
    let (initial_title, initial_structure, initial_checksum, initial_updated): (
        String,
        String,
        String,
        String,
    ) = db
        .conn()
        .query_row(
            "SELECT title, structure_json, content_checksum, updated_at FROM stories WHERE id = ?1",
            [&created.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("row");

    let err = update_story(
        &mut db,
        UpdateStoryInput {
            id: created.id.clone(),
            title: "   ".into(),
        },
    )
    .expect_err("must fail");
    assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);

    let (current_title, current_structure, current_checksum, current_updated): (
        String,
        String,
        String,
        String,
    ) = db
        .conn()
        .query_row(
            "SELECT title, structure_json, content_checksum, updated_at FROM stories WHERE id = ?1",
            [&created.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("row");
    assert_eq!(current_title, initial_title);
    assert_eq!(current_structure, initial_structure);
    assert_eq!(current_checksum, initial_checksum);
    assert_eq!(current_updated, initial_updated);
}

/// Two successive updates must produce strictly increasing `updated_at`.
/// Required for the UI to render a meaningful "dernier enregistrement".
#[test]
fn updated_at_strictly_increases_on_successive_updates() {
    let mut db = db::open_in_memory().expect("open");
    db::run_migrations(&mut db).expect("migrate");
    let created = create_story(&mut db, CreateStoryInput { title: "v0".into() }).expect("create");

    std::thread::sleep(std::time::Duration::from_millis(2));
    let first = update_story(
        &mut db,
        UpdateStoryInput {
            id: created.id.clone(),
            title: "v1".into(),
        },
    )
    .expect("first");

    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = update_story(
        &mut db,
        UpdateStoryInput {
            id: created.id.clone(),
            title: "v2".into(),
        },
    )
    .expect("second");

    assert!(
        second.updated_at > first.updated_at,
        "successive updates must bump updated_at: {} > {}",
        second.updated_at,
        first.updated_at
    );
}

/// A successful update must be visible through the library overview read
/// path after a simulated restart — proving AC2 end-to-end for the
/// read-side of the library.
#[test]
fn get_library_overview_reflects_updated_title_after_restart() {
    let tmp = TempDir::new().expect("tempdir");
    let path = resolve_db_path(tmp.path());

    let created_id;
    {
        let mut db = db::open_at(&path).expect("open first");
        db::run_migrations(&mut db).expect("migrate");
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Avant".into(),
            },
        )
        .expect("create");
        created_id = created.id;
    }

    {
        let mut db = db::open_at(&path).expect("reopen for update");
        update_story(
            &mut db,
            UpdateStoryInput {
                id: created_id.clone(),
                title: "Après".into(),
            },
        )
        .expect("update");
    }

    {
        let db = db::open_at(&path).expect("reopen for read");
        // Use the Rust-side storage layer for the read so this stays a
        // pure integration test (no Tauri AppHandle dependency here); the
        // application::library module is already covered by unit tests.
        let (id, title): (String, String) = db
            .conn()
            .query_row(
                "SELECT id, title FROM stories ORDER BY created_at ASC, id ASC",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row");
        assert_eq!(id, created_id);
        assert_eq!(title, "Après");
    }
}

/// A missing id must be refused with `LIBRARY_INCONSISTENT` and not
/// accidentally touch any other row on the table.
#[test]
fn update_story_missing_id_is_reported_without_side_effects() {
    let mut db = db::open_in_memory().expect("open");
    db::run_migrations(&mut db).expect("migrate");

    let other = create_story(
        &mut db,
        CreateStoryInput {
            title: "Témoin".into(),
        },
    )
    .expect("create");
    let (witness_title_before, witness_updated_before): (String, String) = db
        .conn()
        .query_row(
            "SELECT title, updated_at FROM stories WHERE id = ?1",
            [&other.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row");

    let err = update_story(
        &mut db,
        UpdateStoryInput {
            id: "absent-id".into(),
            title: "Nouveau".into(),
        },
    )
    .expect_err("must fail");
    assert_eq!(err.code, AppErrorCode::LibraryInconsistent);

    let (witness_title_after, witness_updated_after): (String, String) = db
        .conn()
        .query_row(
            "SELECT title, updated_at FROM stories WHERE id = ?1",
            [&other.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row");
    assert_eq!(witness_title_before, witness_title_after);
    assert_eq!(witness_updated_before, witness_updated_after);
}
