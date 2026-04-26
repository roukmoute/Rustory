//! Integration tests for the recovery-draft flow on a real on-disk
//! SQLite path. These cover the cold-reopen scenarios that matter for
//! AC1 and AC2: a draft buffer must survive a process restart.

use rustory_lib::application::story::recovery::{
    apply_recovery, discard_draft, read_recoverable_draft, record_draft, ApplyRecoveryInput,
    RecordDraftInput,
};
use rustory_lib::application::story::{
    create_story, update_story, CreateStoryInput, UpdateStoryInput,
};
use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

/// Helper to (re)open a database at the same path. The returned handle
/// owns its own SQLite connection — closing it drops the WAL state and
/// behaves like an app shutdown.
fn open_db(path: &std::path::Path) -> DbHandle {
    let mut db = open_at(path).expect("open");
    run_migrations(&mut db).expect("migrate");
    db
}

#[test]
fn recoverable_draft_survives_app_restart() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    let story_id = {
        let mut db = open_db(&path);
        let card = create_story(
            &mut db,
            CreateStoryInput {
                title: "Persisted".into(),
            },
        )
        .expect("create");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: card.id.clone(),
                draft_title: "Live keystroke".into(),
            },
        )
        .expect("record");
        card.id
        // db drops here — equivalent to a process exit.
    };

    // Reopen — the draft must still be queryable byte-for-byte.
    let db = open_db(&path);
    let draft = read_recoverable_draft(&db, &story_id)
        .expect("read")
        .expect("some draft after restart");
    assert_eq!(draft.story_id, story_id);
    assert_eq!(draft.draft_title, "Live keystroke");
    assert!(
        draft.draft_at.ends_with('Z'),
        "draft_at must be canonical ISO-8601 UTC, got {}",
        draft.draft_at
    );
}

#[test]
fn apply_recovery_after_restart_persists_the_recovered_title() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    let story_id = {
        let mut db = open_db(&path);
        let card = create_story(
            &mut db,
            CreateStoryInput {
                title: "Old".into(),
            },
        )
        .expect("create");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: card.id.clone(),
                draft_title: "Recovered".into(),
            },
        )
        .expect("record");
        card.id
    };

    let mut db = open_db(&path);
    let output = apply_recovery(
        &mut db,
        ApplyRecoveryInput {
            story_id: story_id.clone(),
        },
    )
    .expect("apply after restart");
    assert_eq!(output.title, "Recovered");

    // Cold-reopen one more time to confirm the UPDATE landed durably.
    drop(db);
    let db = open_db(&path);
    let title: String = db
        .conn()
        .query_row(
            "SELECT title FROM stories WHERE id = ?1",
            rusqlite::params![&story_id],
            |row| row.get(0),
        )
        .expect("read title");
    assert_eq!(title, "Recovered");
    assert!(
        read_recoverable_draft(&db, &story_id)
            .expect("read")
            .is_none(),
        "draft must be consumed across restart"
    );
}

#[test]
fn update_story_clears_pending_draft_in_same_transaction() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    let story_id = {
        let mut db = open_db(&path);
        let card = create_story(
            &mut db,
            CreateStoryInput {
                title: "Old".into(),
            },
        )
        .expect("create");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: card.id.clone(),
                draft_title: "Buffered".into(),
            },
        )
        .expect("record");
        update_story(
            &mut db,
            UpdateStoryInput {
                id: card.id.clone(),
                title: "Saved".into(),
            },
        )
        .expect("autosave success");
        card.id
    };

    // After restart, the draft row must be absent.
    let db = open_db(&path);
    assert!(
        read_recoverable_draft(&db, &story_id)
            .expect("read")
            .is_none(),
        "successful autosave consumes the draft buffer atomically"
    );
}

#[test]
fn discard_draft_after_restart_clears_the_row() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    let story_id = {
        let mut db = open_db(&path);
        let card = create_story(
            &mut db,
            CreateStoryInput {
                title: "Old".into(),
            },
        )
        .expect("create");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: card.id.clone(),
                draft_title: "Buffered".into(),
            },
        )
        .expect("record");
        card.id
    };

    {
        let mut db = open_db(&path);
        discard_draft(&mut db, &story_id, None).expect("discard");
    }

    let db = open_db(&path);
    assert!(read_recoverable_draft(&db, &story_id)
        .expect("read")
        .is_none());
}

#[test]
fn apply_recovery_atomicity_on_invalid_draft() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_db(&path);

    let card = create_story(
        &mut db,
        CreateStoryInput {
            title: "Old".into(),
        },
    )
    .expect("create");
    // Whitespace-only draft: passes the SQLite CHECK (length ≤ 4096) but
    // fails `validate_title` (empty after trim).
    record_draft(
        &mut db,
        RecordDraftInput {
            story_id: card.id.clone(),
            draft_title: "   ".into(),
        },
    )
    .expect("record whitespace");

    let err = apply_recovery(
        &mut db,
        ApplyRecoveryInput {
            story_id: card.id.clone(),
        },
    )
    .expect_err("invalid must fail");
    assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);

    // Stories row untouched, draft row preserved.
    let title: String = db
        .conn()
        .query_row(
            "SELECT title FROM stories WHERE id = ?1",
            rusqlite::params![&card.id],
            |row| row.get(0),
        )
        .expect("title");
    assert_eq!(title, "Old");

    let draft = read_recoverable_draft(&db, &card.id)
        .expect("read")
        .expect("draft must survive an invalid apply");
    assert_eq!(draft.draft_title, "   ");
}

#[test]
fn cascade_delete_on_story_removal_clears_draft() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_db(&path);

    let card = create_story(
        &mut db,
        CreateStoryInput {
            title: "Old".into(),
        },
    )
    .expect("create");
    record_draft(
        &mut db,
        RecordDraftInput {
            story_id: card.id.clone(),
            draft_title: "Buffered".into(),
        },
    )
    .expect("record");

    db.conn()
        .execute(
            "DELETE FROM stories WHERE id = ?1",
            rusqlite::params![&card.id],
        )
        .expect("manual delete");

    assert!(
        read_recoverable_draft(&db, &card.id)
            .expect("read")
            .is_none(),
        "FK CASCADE must remove the orphan draft"
    );
}
