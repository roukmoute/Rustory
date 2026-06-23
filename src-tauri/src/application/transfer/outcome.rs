//! Durable transfer-outcome persistence services (the `transfer_jobs` memory).
//!
//! The bridge between the SQLite `transfer_jobs` table and the IPC layer. It keeps
//! the same atomicity discipline as the rest of the mutation surface (`BEGIN
//! IMMEDIATE`, a single row, the DB lock released BEFORE any diagnostics write —
//! the `recovery.rs` pattern) and the same PII discipline (no raw rusqlite message
//! crosses the boundary, only a stable `source` / `kind`).
//!
//! The functions are intentionally thin: they hold no app-handle, no tracing, no UI
//! mapping. The command layer combines them with the diagnostics events. A
//! FUNCTIONAL transfer failure is NEVER surfaced here — it is the terminal job
//! state; these services only ever fail for a persistence TRANSPORT reason
//! (`TRANSFER_OUTCOME_UNAVAILABLE`), or `LIBRARY_INCONSISTENT` when the parent
//! story vanished (FK).

use rusqlite::OptionalExtension;

use crate::application::story::{now_iso_ms, sqlite_kind_label};
use crate::domain::shared::AppError;
use crate::domain::transfer::{
    PersistedTerminalKind, PersistedTransferOutcome, TransferCompleteness, TransferFailureCause,
    VerifiedSummary, VerifyVerdict,
};
use crate::infrastructure::db::DbHandle;

/// Persist (UPSERT "latest wins") the LAST terminal outcome for a story.
///
/// Atomicity: `BEGIN IMMEDIATE` + UPSERT + COMMIT — a second concurrent writer is
/// serialized; a crash mid-transaction leaves the previous row. The terminals of a
/// single job are coarse and single-flight-gated, so a plain last-writer-wins
/// overwrite is the correct "latest wins": a relaunch's terminal replaces the
/// failure it superseded.
///
/// Foreign-key behavior: a non-existent `story_id` trips the FK and surfaces as
/// `LIBRARY_INCONSISTENT` (the canonical "story disappeared" path, exactly like
/// `record_draft`). Any other transport error is `TRANSFER_OUTCOME_UNAVAILABLE`
/// with `details.source = "sqlite_upsert"`.
pub fn record_transfer_outcome(
    db: &mut DbHandle,
    story_id: &str,
    job_id: &str,
    device_identifier: Option<&str>,
    outcome: &PersistedTransferOutcome,
) -> Result<(), AppError> {
    let now_iso = now_iso_ms()?;
    let cause = outcome.cause.map(TransferFailureCause::wire_cause);
    let completeness = outcome
        .completeness
        .map(TransferCompleteness::diagnostic_tag);
    let verify_verdict = outcome.verify_verdict.map(VerifyVerdict::diagnostic_tag);
    let summary_changed = outcome.summary.as_ref().map(|s| s.changed.as_str());
    let summary_unchanged = outcome.summary.as_ref().map(|s| s.unchanged.as_str());

    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| map_transport_error(&err, "sqlite_upsert", story_id))?;

    let upsert = tx.execute(
        "INSERT INTO transfer_jobs ( \
            story_id, job_id, device_identifier, terminal_kind, cause, completeness, \
            verify_verdict, message, user_action, summary_changed, summary_unchanged, recorded_at \
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
         ON CONFLICT(story_id) DO UPDATE SET \
            job_id = excluded.job_id, \
            device_identifier = excluded.device_identifier, \
            terminal_kind = excluded.terminal_kind, \
            cause = excluded.cause, \
            completeness = excluded.completeness, \
            verify_verdict = excluded.verify_verdict, \
            message = excluded.message, \
            user_action = excluded.user_action, \
            summary_changed = excluded.summary_changed, \
            summary_unchanged = excluded.summary_unchanged, \
            recorded_at = excluded.recorded_at",
        rusqlite::params![
            story_id,
            job_id,
            device_identifier,
            outcome.terminal_kind.wire_tag(),
            cause,
            completeness,
            verify_verdict,
            outcome.message.as_str(),
            outcome.user_action.as_str(),
            summary_changed,
            summary_unchanged,
            now_iso,
        ],
    );

    if let Err(err) = upsert {
        let kind = sqlite_kind_label(&err);
        let _ = tx.rollback();
        if is_foreign_key_violation(&err) {
            return Err(story_missing_error(story_id));
        }
        return Err(transfer_outcome_unavailable(
            "sqlite_upsert",
            kind,
            story_id,
        ));
    }

    tx.commit()
        .map_err(|err| map_transport_error(&err, "sqlite_upsert", story_id))?;
    Ok(())
}

/// A stored outcome paired with its persistence timestamp — the read result that
/// the command maps to the wire DTO. `recorded_at` is the ISO-8601 UTC instant the
/// terminal was last UPSERTed (recency of the remembered outcome).
pub struct StoredTransferOutcome {
    pub outcome: PersistedTransferOutcome,
    pub recorded_at: String,
}

/// Read the durable outcome for a story, if any. Returns `Ok(None)` when no row
/// exists — informational, not an error. A row that fails to re-parse / is
/// incoherent (a corrupt manual edit, a drifted tag) ALSO degrades to `Ok(None)`:
/// the memory is best-effort operational observability, never a hard failure that
/// could block the panel.
pub fn read_transfer_outcome(
    db: &DbHandle,
    story_id: &str,
) -> Result<Option<StoredTransferOutcome>, AppError> {
    let row: Option<StoredRow> = db
        .conn()
        .query_row(
            "SELECT terminal_kind, cause, completeness, verify_verdict, message, user_action, \
                    summary_changed, summary_unchanged, recorded_at \
             FROM transfer_jobs WHERE story_id = ?1",
            rusqlite::params![story_id],
            |row| {
                Ok(StoredRow {
                    terminal_kind: row.get(0)?,
                    cause: row.get(1)?,
                    completeness: row.get(2)?,
                    verify_verdict: row.get(3)?,
                    message: row.get(4)?,
                    user_action: row.get(5)?,
                    summary_changed: row.get(6)?,
                    summary_unchanged: row.get(7)?,
                    recorded_at: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(map_select_error)?;

    Ok(row.and_then(|row| {
        let recorded_at = row.recorded_at.clone();
        reconstruct_outcome(row).map(|outcome| StoredTransferOutcome {
            outcome,
            recorded_at,
        })
    }))
}

/// Drop the durable outcome for a story (the `Abandonner` purge). Idempotent:
/// deleting an already-absent row is a silent `Ok(())`. Never touches canonical
/// state. A transport error surfaces `TRANSFER_OUTCOME_UNAVAILABLE` with
/// `details.source = "sqlite_delete"` so an explicit purge failure stays visible.
pub fn discard_transfer_outcome(db: &mut DbHandle, story_id: &str) -> Result<(), AppError> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| map_transport_error(&err, "sqlite_delete", story_id))?;

    tx.execute(
        "DELETE FROM transfer_jobs WHERE story_id = ?1",
        rusqlite::params![story_id],
    )
    .map_err(|err| map_transport_error(&err, "sqlite_delete", story_id))?;

    tx.commit()
        .map_err(|err| map_transport_error(&err, "sqlite_delete", story_id))?;
    Ok(())
}

/// The raw `transfer_jobs` columns before re-parsing into the domain model.
struct StoredRow {
    terminal_kind: String,
    cause: Option<String>,
    completeness: Option<String>,
    verify_verdict: Option<String>,
    message: String,
    user_action: String,
    summary_changed: Option<String>,
    summary_unchanged: Option<String>,
    recorded_at: String,
}

/// Re-parse a stored row into the pure domain model, validating coherence. Any
/// drifted tag, half-populated summary or F6 violation yields `None` (treated as
/// "no memory") — the read path never panics on a corrupt operational row.
fn reconstruct_outcome(row: StoredRow) -> Option<PersistedTransferOutcome> {
    let terminal_kind = PersistedTerminalKind::from_wire_tag(&row.terminal_kind)?;
    let cause = match row.cause {
        Some(tag) => Some(TransferFailureCause::from_wire_cause(&tag)?),
        None => None,
    };
    let completeness = match row.completeness {
        Some(tag) => Some(TransferCompleteness::from_diagnostic_tag(&tag)?),
        None => None,
    };
    let verify_verdict = match row.verify_verdict {
        Some(tag) => Some(VerifyVerdict::from_diagnostic_tag(&tag)?),
        None => None,
    };
    let summary = match (row.summary_changed, row.summary_unchanged) {
        (Some(changed), Some(unchanged)) => Some(VerifiedSummary { changed, unchanged }),
        (None, None) => None,
        // A half-populated summary is corrupt — degrade to "no memory".
        _ => return None,
    };
    let outcome = PersistedTransferOutcome {
        terminal_kind,
        cause,
        completeness,
        verify_verdict,
        message: row.message,
        user_action: row.user_action,
        summary,
    };
    outcome.is_coherent().then_some(outcome)
}

/// Whether a rusqlite error is a foreign-key constraint violation (the canonical
/// "parent story disappeared" case).
fn is_foreign_key_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(code, _)
            if code.code == rusqlite::ErrorCode::ConstraintViolation
                && code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY,
    )
}

fn map_transport_error(err: &rusqlite::Error, source: &str, story_id: &str) -> AppError {
    let kind = sqlite_kind_label(err);
    transfer_outcome_unavailable(source, kind, story_id)
}

fn map_select_error(err: rusqlite::Error) -> AppError {
    let kind = sqlite_kind_label(&err);
    AppError::transfer_outcome_unavailable(
        "Mémoire de transfert indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_select",
        "table": "transfer_jobs",
        "kind": kind,
    }))
}

fn transfer_outcome_unavailable(source: &str, kind: &str, story_id: &str) -> AppError {
    AppError::transfer_outcome_unavailable(
        "Mémoire de transfert indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": source,
        "table": "transfer_jobs",
        "kind": kind,
        "id": story_id,
    }))
}

fn story_missing_error(story_id: &str) -> AppError {
    AppError::library_inconsistent(
        "Histoire introuvable, recharge la bibliothèque.",
        "Retourne à la bibliothèque et recharge la liste.",
    )
    .with_details(serde_json::json!({
        "source": "story_missing",
        "id": story_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::{create_story, CreateStoryInput};
    use crate::domain::shared::AppErrorCode;
    use crate::infrastructure::db;

    fn fresh_db() -> DbHandle {
        let mut db = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut db).expect("migrate");
        db
    }

    fn seed_story(db: &mut DbHandle, title: &str) -> String {
        create_story(
            db,
            CreateStoryInput {
                title: title.to_string(),
            },
        )
        .expect("seed story")
        .id
    }

    fn count_rows(db: &DbHandle, story_id: &str) -> u32 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM transfer_jobs WHERE story_id = ?1",
                rusqlite::params![story_id],
                |row| row.get(0),
            )
            .expect("count")
    }

    fn verified_summary() -> VerifiedSummary {
        VerifiedSummary {
            changed: "« Mon histoire » est maintenant sur la Lunii.".into(),
            unchanged: "2 autres histoires de l'appareil restent inchangées.".into(),
        }
    }

    #[test]
    fn record_then_read_round_trips_a_write_terminal() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        let outcome = PersistedTransferOutcome::from_write_terminal(
            TransferFailureCause::WriteRejected,
            TransferCompleteness::Incomplete,
        );

        record_transfer_outcome(&mut db, &id, "job-1", Some("dev"), &outcome).expect("record");

        let read = read_transfer_outcome(&db, &id)
            .expect("read")
            .expect("some");
        assert_eq!(read.outcome, outcome);
        assert_eq!(
            read.outcome.terminal_kind,
            PersistedTerminalKind::Incomplete
        );
        assert_eq!(
            read.outcome.cause,
            Some(TransferFailureCause::WriteRejected)
        );
        assert_eq!(
            read.outcome.completeness,
            Some(TransferCompleteness::Incomplete)
        );
        assert!(read.outcome.verify_verdict.is_none());
        assert!(
            read.recorded_at.ends_with('Z'),
            "recorded_at must be ISO-8601 UTC, got {}",
            read.recorded_at
        );
    }

    #[test]
    fn record_then_read_round_trips_a_verified_terminal_with_summary() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        let outcome = PersistedTransferOutcome::from_verified(verified_summary());

        record_transfer_outcome(&mut db, &id, "job-1", Some("dev"), &outcome).expect("record");

        let read = read_transfer_outcome(&db, &id)
            .expect("read")
            .expect("some");
        assert_eq!(read.outcome.terminal_kind, PersistedTerminalKind::Verified);
        let summary = read.outcome.summary.as_ref().expect("summary present");
        assert!(summary.changed.contains("Mon histoire"));
        assert!(summary.unchanged.starts_with("2 autres histoires"));
    }

    #[test]
    fn record_then_read_round_trips_a_verify_partial_terminal() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        let outcome =
            PersistedTransferOutcome::from_verify_verdict(VerifyVerdict::Partial).expect("partial");

        record_transfer_outcome(&mut db, &id, "job-1", None, &outcome).expect("record");

        let read = read_transfer_outcome(&db, &id)
            .expect("read")
            .expect("some");
        assert_eq!(read.outcome.terminal_kind, PersistedTerminalKind::Partial);
        assert_eq!(read.outcome.verify_verdict, Some(VerifyVerdict::Partial));
        assert!(read.outcome.cause.is_none() && read.outcome.completeness.is_none());
    }

    #[test]
    fn record_upserts_latest_wins_keeping_a_single_row() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        let first = PersistedTransferOutcome::from_write_terminal(
            TransferFailureCause::Interrupted,
            TransferCompleteness::Failed,
        );
        record_transfer_outcome(&mut db, &id, "job-1", None, &first).expect("first record");

        // A relaunch succeeds: the verified terminal supersedes the failure.
        let second = PersistedTransferOutcome::from_verified(verified_summary());
        record_transfer_outcome(&mut db, &id, "job-2", Some("dev"), &second)
            .expect("second record");

        assert_eq!(count_rows(&db, &id), 1, "UPSERT must keep a single row");
        let read = read_transfer_outcome(&db, &id)
            .expect("read")
            .expect("some");
        assert_eq!(read.outcome.terminal_kind, PersistedTerminalKind::Verified);
    }

    #[test]
    fn read_returns_none_when_no_row() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        assert!(read_transfer_outcome(&db, &id).expect("read").is_none());
    }

    #[test]
    fn record_rejects_an_orphan_story_id_with_library_inconsistent() {
        let mut db = fresh_db();
        let outcome = PersistedTransferOutcome::from_verified(verified_summary());
        let err = record_transfer_outcome(&mut db, "ghost", "job-1", None, &outcome)
            .expect_err("orphan must fail");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        assert_eq!(
            err.details.as_ref().expect("details")["source"],
            "story_missing"
        );
    }

    #[test]
    fn discard_removes_the_row_and_is_idempotent() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        record_transfer_outcome(
            &mut db,
            &id,
            "job-1",
            None,
            &PersistedTransferOutcome::from_verify_verdict(VerifyVerdict::Failed).expect("failed"),
        )
        .expect("record");
        assert_eq!(count_rows(&db, &id), 1);

        discard_transfer_outcome(&mut db, &id).expect("first discard");
        assert_eq!(count_rows(&db, &id), 0);
        // A second discard on an already-empty row resolves silently.
        discard_transfer_outcome(&mut db, &id).expect("second discard");
        assert_eq!(count_rows(&db, &id), 0);
    }

    #[test]
    fn cascade_delete_removes_the_outcome_when_the_story_is_deleted() {
        let mut db = fresh_db();
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

        assert_eq!(count_rows(&db, &id), 0, "FK CASCADE removes the outcome");
    }

    #[test]
    fn read_degrades_to_none_on_a_drifted_cause_tag() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        // The `terminal_kind` CHECK is closed, so a drifted KIND cannot be injected;
        // corrupt the `cause` column instead (the CHECK does not constrain it).
        record_transfer_outcome(
            &mut db,
            &id,
            "job-1",
            None,
            &PersistedTransferOutcome::from_write_terminal(
                TransferFailureCause::WriteRejected,
                TransferCompleteness::Failed,
            ),
        )
        .expect("record");
        db.conn()
            .execute(
                "UPDATE transfer_jobs SET cause = 'bogusCause' WHERE story_id = ?1",
                rusqlite::params![&id],
            )
            .expect("corrupt cause");

        // A drifted cause tag re-parses to None ⇒ the read degrades to "no memory".
        assert!(read_transfer_outcome(&db, &id).expect("read").is_none());
    }

    #[test]
    fn read_degrades_to_none_on_a_kind_structure_incoherence() {
        // Exercise the central anti-corruption guard `is_coherent` FROM the DB: a row
        // whose `terminal_kind` is valid but whose structure is incoherent (here a
        // `verified` row carrying a write-phase `completeness`) must be rejected by
        // `reconstruct_outcome` — never re-hydrated as a usable outcome.
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        record_transfer_outcome(
            &mut db,
            &id,
            "job-1",
            Some("dev"),
            &PersistedTransferOutcome::from_verified(verified_summary()),
        )
        .expect("record");
        db.conn()
            .execute(
                "UPDATE transfer_jobs SET completeness = 'failed' WHERE story_id = ?1",
                rusqlite::params![&id],
            )
            .expect("inject incoherent structure");

        // `verified` + a non-NULL `completeness` violates the F6 coherence invariant
        // (`summary` xor write/verify discriminants) ⇒ degraded to "no memory".
        assert!(read_transfer_outcome(&db, &id).expect("read").is_none());
    }
}
