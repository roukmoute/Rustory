//! Integration tests for the durable transfer-outcome memory on a real on-disk
//! SQLite path. These cover the cross-session scenarios that matter for AC2/AC3:
//! the last terminal outcome must survive a process restart, an `Abandonner` must
//! purge it, and neither the memory nor its purge may ever touch the canonical
//! story (FR18).

use rustory_lib::application::story::{create_story, CreateStoryInput};
use rustory_lib::application::transfer::{
    discard_transfer_outcome, read_transfer_outcome, record_transfer_outcome,
};
use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::domain::transfer::{
    PersistedTerminalKind, PersistedTransferOutcome, TransferCompleteness, TransferFailureCause,
    VerifiedSummary,
};
use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

fn open_db(path: &std::path::Path) -> DbHandle {
    let mut db = open_at(path).expect("open");
    run_migrations(&mut db).expect("migrate");
    db
}

fn seed_story(db: &mut DbHandle, title: &str) -> String {
    create_story(
        db,
        CreateStoryInput {
            title: title.to_string(),
        },
    )
    .expect("create")
    .id
}

/// Snapshot of the canonical columns that FR18 forbids the memory from mutating.
fn canonical_snapshot(db: &DbHandle, story_id: &str) -> (String, u32, String, String, String) {
    db.conn()
        .query_row(
            "SELECT title, schema_version, structure_json, content_checksum, updated_at \
             FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("read canonical row")
}

fn verified_summary() -> VerifiedSummary {
    VerifiedSummary {
        changed: "« Mon histoire » est maintenant sur la Lunii.".into(),
        unchanged: "2 autres histoires de l'appareil restent inchangées.".into(),
    }
}

#[test]
fn transfer_outcome_survives_app_restart() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");

    let story_id = {
        let mut db = open_db(&path);
        let id = seed_story(&mut db, "Persisted");
        let outcome = PersistedTransferOutcome::from_write_terminal(
            TransferFailureCause::WriteRejected,
            TransferCompleteness::Incomplete,
        );
        record_transfer_outcome(&mut db, &id, "job-1", Some("dev"), &outcome).expect("record");
        id
        // db drops here — equivalent to a process exit.
    };

    // Reopen — the terminal must still be re-hydratable (AC2 durable memory).
    let db = open_db(&path);
    let read = read_transfer_outcome(&db, &story_id)
        .expect("read")
        .expect("some outcome after restart");
    assert_eq!(
        read.outcome.terminal_kind,
        PersistedTerminalKind::Incomplete
    );
    assert_eq!(
        read.outcome.completeness,
        Some(TransferCompleteness::Incomplete)
    );
    assert_eq!(
        read.outcome.cause,
        Some(TransferFailureCause::WriteRejected)
    );
    assert!(!read.outcome.message.is_empty() && !read.outcome.user_action.is_empty());
    assert!(
        read.recorded_at.ends_with('Z'),
        "recorded_at survives the restart as ISO-8601 UTC"
    );
}

#[test]
fn record_upserts_latest_wins_across_a_relaunch() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_db(&path);
    let id = seed_story(&mut db, "Persisted");

    // A failure is remembered…
    record_transfer_outcome(
        &mut db,
        &id,
        "job-1",
        None,
        &PersistedTransferOutcome::from_write_terminal(
            TransferFailureCause::Interrupted,
            TransferCompleteness::Failed,
        ),
    )
    .expect("first record");
    // …then a relaunch succeeds: the verified terminal supersedes it.
    record_transfer_outcome(
        &mut db,
        &id,
        "job-2",
        Some("dev"),
        &PersistedTransferOutcome::from_verified(verified_summary()),
    )
    .expect("second record");

    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM transfer_jobs WHERE story_id = ?1",
            rusqlite::params![&id],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(count, 1, "UPSERT keeps a single row per story");
    let read = read_transfer_outcome(&db, &id)
        .expect("read")
        .expect("some");
    assert_eq!(read.outcome.terminal_kind, PersistedTerminalKind::Verified);
}

#[test]
fn discard_purges_and_is_idempotent() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_db(&path);
    let id = seed_story(&mut db, "Persisted");

    record_transfer_outcome(
        &mut db,
        &id,
        "job-1",
        None,
        &PersistedTransferOutcome::from_verified(verified_summary()),
    )
    .expect("record");

    discard_transfer_outcome(&mut db, &id).expect("first discard");
    assert!(read_transfer_outcome(&db, &id).expect("read").is_none());
    // A second discard on an already-empty row resolves silently.
    discard_transfer_outcome(&mut db, &id).expect("second discard");
    assert!(read_transfer_outcome(&db, &id).expect("read").is_none());
}

#[test]
fn fk_cascade_removes_the_outcome_when_the_story_is_deleted() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_db(&path);
    let id = seed_story(&mut db, "Persisted");
    record_transfer_outcome(
        &mut db,
        &id,
        "job-1",
        None,
        &PersistedTransferOutcome::from_verified(verified_summary()),
    )
    .expect("record");

    db.conn()
        .execute("DELETE FROM stories WHERE id = ?1", rusqlite::params![&id])
        .expect("delete story");

    let count: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM transfer_jobs WHERE story_id = ?1",
            rusqlite::params![&id],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(count, 0, "FK CASCADE removes the operational memory");
}

#[test]
fn record_with_a_missing_story_is_library_inconsistent() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_db(&path);
    let err = record_transfer_outcome(
        &mut db,
        "ghost",
        "job-1",
        None,
        &PersistedTransferOutcome::from_verified(verified_summary()),
    )
    .expect_err("orphan must fail");
    assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
}

#[test]
fn canonical_story_is_never_mutated_by_record_or_discard() {
    // FR18: the memory is purely operational — recording AND purging an outcome
    // must leave the canonical story (structure / checksum / title / updated_at)
    // byte-for-byte unchanged.
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rustory.sqlite");
    let mut db = open_db(&path);
    let id = seed_story(&mut db, "Canonical");

    let before = canonical_snapshot(&db, &id);

    record_transfer_outcome(
        &mut db,
        &id,
        "job-1",
        Some("dev"),
        &PersistedTransferOutcome::from_write_terminal(
            TransferFailureCause::DeviceChanged,
            TransferCompleteness::Failed,
        ),
    )
    .expect("record");
    assert_eq!(
        canonical_snapshot(&db, &id),
        before,
        "record must not touch the canonical story"
    );

    discard_transfer_outcome(&mut db, &id).expect("discard");
    assert_eq!(
        canonical_snapshot(&db, &id),
        before,
        "discard must not touch the canonical story"
    );
}
