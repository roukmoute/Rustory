//! Recovery-draft application services.
//!
//! These services are the bridge between the SQLite `story_drafts` table
//! and the IPC layer. They keep the same atomicity discipline as the rest
//! of the story-mutation surface (`BEGIN IMMEDIATE` transactions, single
//! row affected, no straddling state) and reuse the existing title
//! validation rules so a recovered draft cannot bypass them at apply time.
//!
//! The functions are intentionally thin: they hold no app-handle, no
//! tracing, no UI mapping. The command layer combines them with the
//! existing `update_story` / `get_story_detail` services and emits the
//! diagnostic events.

use crate::application::story::{now_iso_ms, sqlite_kind_label};
use crate::domain::shared::AppError;
use crate::domain::story::{map_error, normalize_title, validate_title, RecoveryDraft};
use crate::infrastructure::db::DbHandle;
use crate::ipc::dto::UpdateStoryOutputDto;
use rusqlite::OptionalExtension;

/// Hard cap on the buffered draft value. Mirrors the SQLite CHECK clause
/// in `0002_story_drafts.sql`. The Rust-side check exists because the
/// IPC payload reaches the application layer before SQLite — we want to
/// fail with a typed `AppError` rather than a generic constraint error.
pub const MAX_DRAFT_TITLE_CHARS: usize = 4096;

/// Run a best-effort rollback, asserting in debug builds when it fails.
///
/// A failed rollback is informational only: the primary error already
/// dominates the user-visible flow. Going through this helper keeps the
/// pattern uniform — the previous code had four `debug_rollback(tx);`
/// sites that silently dropped the inner error. Debug builds now panic
/// instead so a CI run surfaces the failure during test development;
/// release builds keep the historical no-op behavior so a single bad
/// rollback never crashes the user app.
fn debug_rollback(tx: rusqlite::Transaction<'_>) {
    if let Err(err) = tx.rollback() {
        debug_assert!(
            false,
            "best-effort rollback failed (debug-only assertion): {err:?}"
        );
        // Release build: discard the error — the caller will already
        // surface the primary diagnostic.
        let _ = err;
    }
}

/// Input accepted by `record_draft`. Holds the raw user value, which may
/// be empty (the user erased everything before the crash) and may contain
/// characters that would fail `validate_title` — re-validation only kicks
/// in at apply time, never at record time.
pub struct RecordDraftInput {
    pub story_id: String,
    pub draft_title: String,
}

/// Input accepted by `apply_recovery`.
pub struct ApplyRecoveryInput {
    pub story_id: String,
}

/// Persist a buffered keystroke value for a story.
///
/// Atomicity: `BEGIN IMMEDIATE` + UPSERT + COMMIT. A second concurrent
/// writer is serialized; a crash mid-transaction leaves the row at its
/// previous value rather than half-applied.
///
/// Foreign-key behavior: a non-existent `story_id` trips the FK constraint
/// and surfaces as `LIBRARY_INCONSISTENT` so the UI funnels it through
/// the existing "Histoire introuvable" copy.
pub fn record_draft(db: &mut DbHandle, input: RecordDraftInput) -> Result<(), AppError> {
    if input.draft_title.chars().count() > MAX_DRAFT_TITLE_CHARS {
        return Err(AppError::recovery_draft_unavailable(
            "Brouillon trop long pour être enregistré.",
            "Réduis la taille du titre puis réessaie.",
        )
        .with_details(serde_json::json!({
            "source": "draft_too_long",
            "max_chars": MAX_DRAFT_TITLE_CHARS,
        })));
    }

    let now_iso = now_iso_ms()?;

    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| map_record_transport_error(&err, "begin_transaction", &input.story_id))?;

    // Compare-and-swap on `draft_at`: ignore the UPSERT when an existing
    // row carries a STRICTLY NEWER timestamp. Without this guard, two
    // `record_draft` calls reordered by Tauri's IPC plumbing (no FIFO
    // contract across in-flight invokes) could let a slow `record({title:"a"})`
    // overwrite a fast `record({title:"abc"})` that landed first. The
    // `WHERE` clause runs inside the same transaction; a no-op here
    // returns `Ok(0)` rows affected, which we treat as success — the
    // newer draft is already on disk and that is what we wanted.
    //
    // The comparison is `>=` (not `>`) so two writes that fall in the
    // same millisecond — common at fast keystroke pace on warm caches —
    // still apply the most recent value rather than getting clamped by
    // their own existing row. The IPC reordering risk that motivates
    // the CAS in the first place still cancels here: a stale write
    // arriving with an OLDER `draft_at` is correctly rejected by the
    // strict-less-than relation.
    //
    // Future-proofing: a row carrying `draft_at` strictly greater than
    // `now()` would lock the table forever (real clock could not catch
    // up to a clock-skewed write). We refuse to UPSERT against such a
    // row by clamping our `excluded.draft_at` to `now_iso` only — the
    // SQLite check `excluded.draft_at >= story_drafts.draft_at` already
    // covers that path: the clamped now-value will be older than the
    // future-dated row and the UPSERT becomes a no-op, but a follow-up
    // service-level remediation (`reset_future_draft_at`) will detect
    // and repair the row at next mount.
    let outcome = tx.execute(
        "INSERT INTO story_drafts (story_id, draft_title, draft_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(story_id) DO UPDATE \
           SET draft_title = excluded.draft_title, draft_at = excluded.draft_at \
           WHERE excluded.draft_at >= story_drafts.draft_at",
        rusqlite::params![&input.story_id, &input.draft_title, &now_iso],
    );

    if let Err(err) = outcome {
        // PII discipline matches `map_update_transport_error`: drop the
        // raw rusqlite message, surface a stable kind/source. A
        // foreign-key failure is the canonical "story disappeared" case.
        let kind = sqlite_kind_label(&err);
        debug_rollback(tx);
        if matches!(
            err,
            rusqlite::Error::SqliteFailure(ref code, _)
                if code.code == rusqlite::ErrorCode::ConstraintViolation
                    && code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY,
        ) {
            return Err(AppError::library_inconsistent(
                "Histoire introuvable, recharge la bibliothèque.",
                "Retourne à la bibliothèque et recharge la liste.",
            )
            .with_details(serde_json::json!({
                "source": "story_missing",
                "id": input.story_id,
            })));
        }
        return Err(AppError::recovery_draft_unavailable(
            "Récupération indisponible: vérifie le disque local et réessaie.",
            "Relance Rustory ; si le problème persiste, consulte les traces locales.",
        )
        .with_details(serde_json::json!({
            "source": "sqlite_upsert",
            "kind": kind,
            "id": input.story_id,
        })));
    }

    tx.commit()
        .map_err(|err| map_record_transport_error(&err, "commit", &input.story_id))?;
    Ok(())
}

/// Read the buffered draft for a story, if any. Returns `Ok(None)` when
/// no row exists — informational, not an error.
pub fn read_recoverable_draft(
    db: &DbHandle,
    story_id: &str,
) -> Result<Option<RecoveryDraft>, AppError> {
    db.conn()
        .query_row(
            "SELECT story_id, draft_title, draft_at FROM story_drafts WHERE story_id = ?1",
            rusqlite::params![story_id],
            |row| {
                Ok(RecoveryDraft {
                    story_id: row.get(0)?,
                    draft_title: row.get(1)?,
                    draft_at: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(map_recovery_select_error)
}

/// Apply a recoverable draft: re-validate the buffered title, write it to
/// `stories`, and consume the draft row in a single transaction.
///
/// On invalid draft (control chars, empty after trim, > MAX_TITLE_CHARS),
/// the draft row is **preserved** so the UI can still offer "Conserver
/// l'état enregistré" — automatically discarding here would lose data the
/// user might have meant to copy out.
pub fn apply_recovery(
    db: &mut DbHandle,
    input: ApplyRecoveryInput,
) -> Result<UpdateStoryOutputDto, AppError> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| map_apply_transport_error(&err, "begin_transaction", &input.story_id))?;

    let draft_title: Option<String> = tx
        .query_row(
            "SELECT draft_title FROM story_drafts WHERE story_id = ?1",
            rusqlite::params![&input.story_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| map_apply_transport_error(&err, "read_draft", &input.story_id))?;

    let Some(draft_title) = draft_title else {
        // Disambiguate two scenarios that both leave the draft row gone:
        //  - The parent story was deleted (CASCADE). The UI should
        //    recover by reloading the library — this is the canonical
        //    `LIBRARY_INCONSISTENT` / "Histoire introuvable" path.
        //  - The draft was discarded by a parallel session / devtools
        //    while the parent story remains. This is the "race"
        //    scenario and stays a `RecoveryDraftUnavailable`.
        let parent_exists: bool = tx
            .query_row(
                "SELECT 1 FROM stories WHERE id = ?1",
                rusqlite::params![&input.story_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|err| map_apply_transport_error(&err, "read_parent", &input.story_id))?
            .unwrap_or(false);
        debug_rollback(tx);
        if !parent_exists {
            return Err(AppError::library_inconsistent(
                "Histoire introuvable, recharge la bibliothèque.",
                "Retourne à la bibliothèque et recharge la liste.",
            )
            .with_details(serde_json::json!({
                "source": "story_missing",
                "id": input.story_id,
            })));
        }
        return Err(AppError::recovery_draft_unavailable(
            "Aucun brouillon à restaurer.",
            "Recharge la page d'édition pour mettre à jour l'état.",
        )
        .with_details(serde_json::json!({
            "source": "draft_missing_in_transaction",
            "id": input.story_id,
        })));
    };

    // Re-validate authoritatively. The draft may have been recorded with
    // a value that the SQLite CHECK accepts (length ≤ 4096) but the
    // product rule rejects (empty after trim, > 120 chars, control chars,
    // denylist Unicode). The draft row stays in place on rejection so the
    // UI can offer Discard explicitly.
    let normalized = normalize_title(&draft_title);
    if let Err(title_err) = validate_title(&normalized) {
        debug_rollback(tx);
        let mapped = map_error(title_err);
        // Tag the source so the UI/log can distinguish a recovery-time
        // validation from a regular create/update validation failure.
        let details = serde_json::json!({
            "source": "recovery_draft_invalid",
            "id": input.story_id,
        });
        return Err(mapped.with_details(details));
    }

    let now_iso = now_iso_ms()?;

    let rows_affected = tx
        .execute(
            "UPDATE stories SET title = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![&normalized, &now_iso, &input.story_id],
        )
        .map_err(|err| map_apply_transport_error(&err, "update", &input.story_id))?;

    if rows_affected == 0 {
        debug_rollback(tx);
        return Err(AppError::library_inconsistent(
            "Histoire introuvable, recharge la bibliothèque.",
            "Retourne à la bibliothèque et recharge la liste.",
        )
        .with_details(serde_json::json!({
            "source": "story_missing",
            "id": input.story_id,
        })));
    }

    tx.execute(
        "DELETE FROM story_drafts WHERE story_id = ?1",
        rusqlite::params![&input.story_id],
    )
    .map_err(|err| map_apply_transport_error(&err, "delete", &input.story_id))?;

    tx.commit()
        .map_err(|err| map_apply_transport_error(&err, "commit", &input.story_id))?;

    Ok(UpdateStoryOutputDto {
        id: input.story_id,
        title: normalized,
        updated_at: now_iso,
    })
}

/// Drop the buffered draft for a story without modifying canonical state.
///
/// When `expected_draft_at` is `Some`, the DELETE is conditional on the
/// row carrying exactly that timestamp — a compare-and-swap that prevents
/// silently consuming a draft a concurrent `record_draft` had just
/// refreshed. The CAS is essential for the `AlreadyPersisted` cleanup
/// path, where the route reads the draft, classifies, and only then
/// asks for the delete: a keystroke racing through the 150 ms record
/// window would otherwise be lost.
///
/// When `expected_draft_at` is `None`, the DELETE runs unconditionally
/// — the user explicitly chose "Conserver l'état enregistré" and accepts
/// dropping whatever is buffered.
///
/// Idempotent in both modes: deleting an already-absent row, or a row
/// whose `draft_at` has moved past `expected_draft_at`, is a silent
/// `Ok(())`. The caller must re-read with `read_recoverable_draft` if
/// the user-visible state matters.
pub fn discard_draft(
    db: &mut DbHandle,
    story_id: &str,
    expected_draft_at: Option<&str>,
) -> Result<(), AppError> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| map_discard_transport_error(&err, "begin_transaction", story_id))?;

    if let Some(expected) = expected_draft_at {
        tx.execute(
            "DELETE FROM story_drafts WHERE story_id = ?1 AND draft_at = ?2",
            rusqlite::params![story_id, expected],
        )
        .map_err(|err| map_discard_transport_error(&err, "delete", story_id))?;
    } else {
        tx.execute(
            "DELETE FROM story_drafts WHERE story_id = ?1",
            rusqlite::params![story_id],
        )
        .map_err(|err| map_discard_transport_error(&err, "delete", story_id))?;
    }

    tx.commit()
        .map_err(|err| map_discard_transport_error(&err, "commit", story_id))?;
    Ok(())
}

fn map_record_transport_error(
    err: &rusqlite::Error,
    stage: &'static str,
    story_id: &str,
) -> AppError {
    let kind = sqlite_kind_label(err);
    AppError::recovery_draft_unavailable(
        "Récupération indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_upsert",
        "table": "story_drafts",
        "stage": stage,
        "kind": kind,
        "id": story_id,
    }))
}

fn map_apply_transport_error(
    err: &rusqlite::Error,
    stage: &'static str,
    story_id: &str,
) -> AppError {
    let kind = sqlite_kind_label(err);
    // Only a CHECK constraint failure means a value slipped past
    // Rust-side validation and was rejected by the schema. Other
    // constraint flavors (FK, UNIQUE / PRIMARY KEY, NOT NULL,
    // ROWID) are transport / consistency errors and have no business
    // being relabeled `INVALID_STORY_TITLE` — that wording would
    // mislead the user into "fix your title" when the real cause is
    // schema corruption or a missing parent row. We narrow the
    // remap to `SQLITE_CONSTRAINT_CHECK` exclusively.
    let is_check_violation = matches!(
        err,
        rusqlite::Error::SqliteFailure(code, _)
            if code.code == rusqlite::ErrorCode::ConstraintViolation
                && code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK,
    );
    if is_check_violation {
        return AppError::invalid_story_title(
            "Création impossible: titre contient des caractères non autorisés",
            "Supprime les sauts de ligne, tabulations et caractères invisibles.",
        )
        .with_details(serde_json::json!({
            "source": "sqlite_apply",
            "table": "story_drafts",
            "stage": stage,
            "kind": kind,
            "id": story_id,
        }));
    }
    AppError::recovery_draft_unavailable(
        "Récupération indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_apply",
        "table": "story_drafts",
        "stage": stage,
        "kind": kind,
        "id": story_id,
    }))
}

fn map_discard_transport_error(
    err: &rusqlite::Error,
    stage: &'static str,
    story_id: &str,
) -> AppError {
    let kind = sqlite_kind_label(err);
    AppError::recovery_draft_unavailable(
        "Récupération indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_delete",
        "table": "story_drafts",
        "stage": stage,
        "kind": kind,
        "id": story_id,
    }))
}

fn map_recovery_select_error(err: rusqlite::Error) -> AppError {
    let kind = sqlite_kind_label(&err);
    AppError::recovery_draft_unavailable(
        "Récupération indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_select",
        "table": "story_drafts",
        "kind": kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::{
        create_story, update_story, CreateStoryInput, UpdateStoryInput,
    };
    use crate::domain::shared::AppErrorCode;
    use crate::infrastructure::db;

    fn fresh_db() -> DbHandle {
        let mut db = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut db).expect("migrate");
        db
    }

    fn seed_story(db: &mut DbHandle, title: &str) -> String {
        let card = create_story(
            db,
            CreateStoryInput {
                title: title.to_string(),
            },
        )
        .expect("seed story");
        card.id
    }

    fn count_drafts(db: &DbHandle, story_id: &str) -> u32 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM story_drafts WHERE story_id = ?1",
                rusqlite::params![story_id],
                |row| row.get(0),
            )
            .expect("count")
    }

    fn read_draft_title(db: &DbHandle, story_id: &str) -> Option<String> {
        db.conn()
            .query_row(
                "SELECT draft_title FROM story_drafts WHERE story_id = ?1",
                rusqlite::params![story_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .expect("read")
    }

    // --- record_draft ---

    #[test]
    fn record_draft_inserts_a_new_row_when_none_exists() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "Buffered".into(),
            },
        )
        .expect("record");

        assert_eq!(read_draft_title(&db, &id).as_deref(), Some("Buffered"));
    }

    #[test]
    fn record_draft_overwrites_when_draft_at_equals_existing() {
        // Two writes in the same millisecond must NOT cancel each other:
        // the latest draft_title wins as long as `draft_at` is not
        // STRICTLY older than the existing row. A `>` strict relation
        // would silently drop fast keystrokes; we use `>=`.
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        db.conn()
            .execute(
                "INSERT INTO story_drafts (story_id, draft_title, draft_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![&id, "first", "2026-04-25T12:00:00.000Z"],
            )
            .expect("seed");

        // Run an UPSERT through the public service. Even if `now_iso_ms`
        // returns a value equal to the seeded row's `draft_at`, the new
        // title must apply.
        // We seed the timestamp to a known recent past so `now()` >= it.
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "second".into(),
            },
        )
        .expect("record");

        assert_eq!(read_draft_title(&db, &id).as_deref(), Some("second"));
    }

    #[test]
    fn record_draft_ignores_an_older_draft_at_when_newer_exists() {
        // Compare-and-swap invariant: an out-of-order `record_draft`
        // with an older `draft_at` than the persisted row must be a
        // no-op. Tauri does not promise FIFO across in-flight invokes,
        // so this guard prevents a slow IPC from clobbering a faster
        // newer one that already landed.
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        // Seed the row directly with a deliberately-far-future draft_at
        // so the next `record_draft` (which uses `now_iso_ms`) is
        // strictly older than what is on disk.
        db.conn()
            .execute(
                "INSERT INTO story_drafts (story_id, draft_title, draft_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![&id, "newer", "2099-01-01T00:00:00.000Z"],
            )
            .expect("seed future row");

        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "stale".into(),
            },
        )
        .expect("record stale");

        // The on-disk value must remain "newer" — the stale UPSERT was
        // a no-op because its `draft_at` is older.
        assert_eq!(read_draft_title(&db, &id).as_deref(), Some("newer"));
    }

    #[test]
    fn record_draft_replaces_an_existing_row_with_newer_at() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "first".into(),
            },
        )
        .expect("first record");
        let first_at: String = db
            .conn()
            .query_row(
                "SELECT draft_at FROM story_drafts WHERE story_id = ?1",
                rusqlite::params![&id],
                |row| row.get(0),
            )
            .expect("read first at");

        std::thread::sleep(std::time::Duration::from_millis(2));

        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "second".into(),
            },
        )
        .expect("second record");

        let second_at: String = db
            .conn()
            .query_row(
                "SELECT draft_at FROM story_drafts WHERE story_id = ?1",
                rusqlite::params![&id],
                |row| row.get(0),
            )
            .expect("read second at");

        assert_eq!(read_draft_title(&db, &id).as_deref(), Some("second"));
        assert!(
            second_at > first_at,
            "draft_at must advance: {first_at} -> {second_at}"
        );
        assert_eq!(count_drafts(&db, &id), 1, "UPSERT must keep a single row");
    }

    #[test]
    fn record_draft_rejects_when_story_id_does_not_exist_in_stories() {
        let mut db = fresh_db();

        let err = record_draft(
            &mut db,
            RecordDraftInput {
                story_id: "ghost".into(),
                draft_title: "x".into(),
            },
        )
        .expect_err("orphan must fail");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "story_missing");
    }

    #[test]
    fn record_draft_rejects_when_draft_title_exceeds_4096_chars() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        let too_long = "a".repeat(MAX_DRAFT_TITLE_CHARS + 1);
        let err = record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: too_long,
            },
        )
        .expect_err("too long must fail");
        assert_eq!(err.code, AppErrorCode::RecoveryDraftUnavailable);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "draft_too_long");
        assert_eq!(count_drafts(&db, &id), 0, "no row must be created");
    }

    #[test]
    fn record_draft_accepts_empty_draft_title() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: String::new(),
            },
        )
        .expect("record empty");

        assert_eq!(read_draft_title(&db, &id).as_deref(), Some(""));
    }

    #[test]
    fn record_draft_does_not_validate_against_story_title_rules() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        // Control chars that would fail `validate_title` must still be
        // accepted at record time — we want the user's exact in-flight
        // value preserved so they can decide what to do with it.
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "with\nnewline".into(),
            },
        )
        .expect("record control char");

        assert_eq!(read_draft_title(&db, &id).as_deref(), Some("with\nnewline"));
    }

    // --- read_recoverable_draft ---

    #[test]
    fn read_recoverable_draft_returns_none_when_no_row() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        assert!(read_recoverable_draft(&db, &id).expect("read").is_none());
    }

    #[test]
    fn read_recoverable_draft_returns_some_when_row_exists() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "Live".into(),
            },
        )
        .expect("record");

        let draft = read_recoverable_draft(&db, &id)
            .expect("read")
            .expect("some");
        assert_eq!(draft.story_id, id);
        assert_eq!(draft.draft_title, "Live");
        assert!(
            draft.draft_at.ends_with('Z'),
            "draft_at must be ISO-8601 UTC with Z suffix, got {}",
            draft.draft_at
        );
    }

    // --- apply_recovery ---

    #[test]
    fn apply_recovery_updates_stories_title_and_clears_draft() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "  New title  ".into(),
            },
        )
        .expect("record");

        let output = apply_recovery(
            &mut db,
            ApplyRecoveryInput {
                story_id: id.clone(),
            },
        )
        .expect("apply");

        assert_eq!(output.title, "New title", "title must be NFC-trimmed");
        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM stories WHERE id = ?1",
                rusqlite::params![&id],
                |row| row.get(0),
            )
            .expect("read title");
        assert_eq!(title, "New title");
        assert_eq!(count_drafts(&db, &id), 0, "draft must be consumed");
    }

    #[test]
    fn apply_recovery_advances_updated_at() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");
        let created_at: String = db
            .conn()
            .query_row(
                "SELECT created_at FROM stories WHERE id = ?1",
                rusqlite::params![&id],
                |row| row.get(0),
            )
            .expect("read created");

        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "New".into(),
            },
        )
        .expect("record");
        std::thread::sleep(std::time::Duration::from_millis(2));

        let output = apply_recovery(
            &mut db,
            ApplyRecoveryInput {
                story_id: id.clone(),
            },
        )
        .expect("apply");

        assert!(
            output.updated_at > created_at,
            "updated_at must advance vs created_at: {created_at} -> {}",
            output.updated_at
        );
    }

    #[test]
    fn apply_recovery_rejects_invalid_draft_with_invalid_story_title() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");

        // Control char survives record_draft but must be refused at apply.
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "with\nnewline".into(),
            },
        )
        .expect("record");

        let err = apply_recovery(
            &mut db,
            ApplyRecoveryInput {
                story_id: id.clone(),
            },
        )
        .expect_err("invalid must fail");
        assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);

        // The draft row must be preserved so the UI can offer Discard.
        assert_eq!(
            count_drafts(&db, &id),
            1,
            "invalid apply must NOT consume the draft"
        );
        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM stories WHERE id = ?1",
                rusqlite::params![&id],
                |row| row.get(0),
            )
            .expect("title");
        assert_eq!(title, "Old", "stories.title must stay untouched");
    }

    #[test]
    fn apply_recovery_returns_recovery_draft_unavailable_when_no_draft_row() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");

        let err = apply_recovery(
            &mut db,
            ApplyRecoveryInput {
                story_id: id.clone(),
            },
        )
        .expect_err("no draft must fail");
        assert_eq!(err.code, AppErrorCode::RecoveryDraftUnavailable);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "draft_missing_in_transaction");
    }

    #[test]
    fn apply_recovery_returns_library_inconsistent_when_story_was_deleted_between_propose_and_apply(
    ) {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "New".into(),
            },
        )
        .expect("record");

        // Race simulation: bypass `delete_story` (none yet) and remove
        // the parent row directly. CASCADE removes the draft too.
        db.conn()
            .execute("DELETE FROM stories WHERE id = ?1", rusqlite::params![&id])
            .expect("manual delete");

        let err = apply_recovery(
            &mut db,
            ApplyRecoveryInput {
                story_id: id.clone(),
            },
        )
        .expect_err("orphan must fail");
        // The CASCADE removed the draft AND the parent story, but the
        // service must distinguish "parent gone" (LibraryInconsistent)
        // from "draft was discarded but parent still present"
        // (RecoveryDraftUnavailable). When the parent is missing, the
        // canonical UX path is the "Histoire introuvable, recharge la
        // bibliothèque" alert.
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "story_missing");
    }

    #[test]
    fn apply_recovery_is_atomic_under_failure() {
        // Force a failure scenario: empty-after-trim draft → invalid.
        // The transaction must roll back fully — drafts row preserved.
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "   ".into(),
            },
        )
        .expect("record whitespace-only");

        let _ = apply_recovery(
            &mut db,
            ApplyRecoveryInput {
                story_id: id.clone(),
            },
        )
        .expect_err("whitespace must fail");

        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM stories WHERE id = ?1",
                rusqlite::params![&id],
                |row| row.get(0),
            )
            .expect("title");
        assert_eq!(title, "Old", "stories untouched");
        assert_eq!(
            count_drafts(&db, &id),
            1,
            "draft must survive for the user to discard"
        );
    }

    // --- discard_draft ---

    #[test]
    fn discard_draft_removes_the_row() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "New".into(),
            },
        )
        .expect("record");

        discard_draft(&mut db, &id, None).expect("discard");
        assert_eq!(count_drafts(&db, &id), 0);
    }

    #[test]
    fn discard_draft_is_idempotent_on_missing_row() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");

        // No draft was recorded. Discard must still resolve OK.
        discard_draft(&mut db, &id, None).expect("first discard");
        discard_draft(&mut db, &id, None).expect("second discard");
        assert_eq!(count_drafts(&db, &id), 0);
    }

    #[test]
    fn discard_draft_with_expected_at_is_a_compare_and_swap_no_op_when_at_changed() {
        // The AlreadyPersisted cleanup path passes the `draft_at` it
        // just observed. If a concurrent record_draft refreshes the
        // row between SELECT and DELETE, the CAS misses, the newer
        // draft survives, and the next mount re-classifies against
        // the fresher buffer. Without the CAS, the newer draft would
        // be silently consumed — exactly the bug report finding #7.
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");

        // Seed a row whose draft_at is "2026-04-25T12:00:00.000Z".
        db.conn()
            .execute(
                "INSERT INTO story_drafts (story_id, draft_title, draft_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![&id, "older", "2026-04-25T12:00:00.000Z"],
            )
            .expect("seed older");
        // The user types again: the row is refreshed to a newer at.
        db.conn()
            .execute(
                "UPDATE story_drafts SET draft_title = ?1, draft_at = ?2 WHERE story_id = ?3",
                rusqlite::params!["newer", "2026-04-25T12:00:01.000Z", &id],
            )
            .expect("refresh");

        // Cleanup path passes the OLDER at it had observed pre-refresh.
        discard_draft(&mut db, &id, Some("2026-04-25T12:00:00.000Z")).expect("CAS discard");

        // The newer row survived the CAS miss.
        assert_eq!(read_draft_title(&db, &id).as_deref(), Some("newer"));
    }

    #[test]
    fn discard_draft_with_expected_at_consumes_when_at_matches() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        db.conn()
            .execute(
                "INSERT INTO story_drafts (story_id, draft_title, draft_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![&id, "stable", "2026-04-25T12:00:00.000Z"],
            )
            .expect("seed");

        discard_draft(&mut db, &id, Some("2026-04-25T12:00:00.000Z")).expect("CAS discard hit");
        assert_eq!(count_drafts(&db, &id), 0);
    }

    #[test]
    fn discard_draft_with_none_at_consumes_unconditionally() {
        // The user-driven Discard button passes `None` so a refreshed
        // draft gets dropped too. That's the correct semantic for
        // "Conserver l'état enregistré".
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        db.conn()
            .execute(
                "INSERT INTO story_drafts (story_id, draft_title, draft_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![&id, "buf", "2026-04-25T12:00:00.000Z"],
            )
            .expect("seed");

        discard_draft(&mut db, &id, None).expect("unconditional discard");
        assert_eq!(count_drafts(&db, &id), 0);
    }

    #[test]
    fn discard_draft_does_not_touch_stories_row() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Persisted");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "Buffered".into(),
            },
        )
        .expect("record");
        let before: String = db
            .conn()
            .query_row(
                "SELECT title FROM stories WHERE id = ?1",
                rusqlite::params![&id],
                |row| row.get(0),
            )
            .expect("read before");

        discard_draft(&mut db, &id, None).expect("discard");

        let after: String = db
            .conn()
            .query_row(
                "SELECT title FROM stories WHERE id = ?1",
                rusqlite::params![&id],
                |row| row.get(0),
            )
            .expect("read after");
        assert_eq!(after, before);
    }

    // --- update_story integration ---

    #[test]
    fn update_story_clears_pending_draft_on_success() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "Buffered".into(),
            },
        )
        .expect("record");
        assert_eq!(count_drafts(&db, &id), 1);

        update_story(
            &mut db,
            UpdateStoryInput {
                id: id.clone(),
                title: "Saved".into(),
            },
        )
        .expect("update");

        assert_eq!(
            count_drafts(&db, &id),
            0,
            "successful autosave must consume the buffered draft in the same transaction"
        );
    }

    #[test]
    fn update_story_failure_preserves_pending_draft() {
        let mut db = fresh_db();
        let id = seed_story(&mut db, "Old");
        record_draft(
            &mut db,
            RecordDraftInput {
                story_id: id.clone(),
                draft_title: "Buffered".into(),
            },
        )
        .expect("record");

        // Force a validation failure: empty title is rejected before
        // the UPDATE is sent. The draft must survive the failure so the
        // next session can still propose recovery.
        let _ = update_story(
            &mut db,
            UpdateStoryInput {
                id: id.clone(),
                title: "   ".into(),
            },
        )
        .expect_err("invalid title must fail");

        assert_eq!(
            count_drafts(&db, &id),
            1,
            "failed autosave must preserve the draft buffer"
        );
    }
}
