use tauri::{AppHandle, State};

use crate::application::story::recovery::{
    self, ApplyRecoveryInput as RecoveryApplyInput, RecordDraftInput,
};
use crate::application::story::{self, CreateStoryInput, UpdateStoryInput};
use crate::commands::shared::validate_story_id;
use crate::domain::shared::AppError;
use crate::domain::story::RecoveryDraftDelta;
use crate::infrastructure::diagnostics::recovery_log;
use crate::ipc::dto::{
    ApplyRecoveryInputDto, CreateStoryInputDto, DiscardDraftInputDto, RecordDraftInputDto,
    RecoverableDraftDto, StoryCardDto, StoryDetailDto, UpdateStoryInputDto, UpdateStoryOutputDto,
};
use crate::AppState;

/// Create a new story draft and return its library card projection.
///
/// Thin command: locks the shared database handle, delegates to the
/// application service, and lets the normalized [`AppError`] bubble up
/// untouched.
#[tauri::command]
pub fn create_story(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: CreateStoryInputDto,
) -> Result<StoryCardDto, AppError> {
    // See `commands::library::get_library_overview` for the rationale on
    // mutex-poison recovery — the same reasoning applies here.
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    story::create_story(&mut db, CreateStoryInput { title: input.title })
}

/// Update an existing story's metadata and return the freshly persisted
/// values so the UI can reconcile its draft with the source of truth.
#[tauri::command]
pub fn update_story(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: UpdateStoryInputDto,
) -> Result<UpdateStoryOutputDto, AppError> {
    validate_story_id(&input.id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    story::update_story(
        &mut db,
        UpdateStoryInput {
            id: input.id,
            title: input.title,
        },
    )
}

/// Read a single story detail by id for the edit surface. Returns `null`
/// when the row is missing — the UI treats that case as "Histoire
/// introuvable" without needing to parse an error.
#[tauri::command]
pub fn get_story_detail(
    _app: AppHandle,
    state: State<'_, AppState>,
    story_id: String,
) -> Result<Option<StoryDetailDto>, AppError> {
    validate_story_id(&story_id)?;
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    story::get_story_detail(&db, &story_id)
}

/// Persist a buffered keystroke value for a story (recovery flow).
///
/// Best-effort logging: a successful record is intentionally NOT logged
/// (one event per keystroke would drown the log). Only failures emit a
/// `recovery_draft_unavailable` event with the operation source so
/// support can correlate.
#[tauri::command]
pub fn record_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    input: RecordDraftInputDto,
) -> Result<(), AppError> {
    validate_story_id(&input.story_id)?;
    let story_id_for_log = input.story_id.clone();
    let outcome = {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recovery::record_draft(
            &mut db,
            RecordDraftInput {
                story_id: input.story_id,
                draft_title: input.draft_title,
            },
        )
    };
    if outcome.is_err() {
        let _ = recovery_log::record_event(
            &app,
            recovery_log::Event::RecoveryDraftUnavailable {
                story_id: story_id_for_log,
                source: "record_draft",
            },
        );
    }
    outcome
}

/// Read the buffered draft for a story and combine it with the persisted
/// title to produce a UI-ready outcome.
///
/// `RecoverableDraftDto::None` covers three cases that the UI treats
/// identically: no draft exists, the draft already matches the persisted
/// title (silently consumed), or the parent story has disappeared.
#[tauri::command]
pub fn read_recoverable_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    story_id: String,
) -> Result<RecoverableDraftDto, AppError> {
    validate_story_id(&story_id)?;

    // We hold the SQLite lock continuously across the two reads AND
    // the AlreadyPersisted cleanup DELETE. The previous shape released
    // the read-lock and re-acquired a fresh write-lock for the DELETE,
    // which opened a race window where a concurrent `record_draft`
    // could refresh the row between read and DELETE. The CAS-on-`draft_at`
    // already protects the row state, but holding the lock once also
    // closes the diagnostic-log inconsistency window where the
    // `recovery_draft_proposed` event could log a draft that was
    // already gone. The `recovery_log::record_event` calls happen AFTER
    // the lock is released, so a slow disk for the diag write never
    // blocks SQLite writers.
    enum ReadOutcome {
        None,
        AlreadyPersisted,
        AlreadyPersistedCleanupFailed(AppError),
        Recoverable {
            story_id: String,
            draft_title: String,
            draft_at: String,
            persisted_title: String,
        },
    }

    let outcome: Result<ReadOutcome, AppError> = (|| {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let detail = story::get_story_detail(&db, &story_id)?;
        let draft = recovery::read_recoverable_draft(&db, &story_id)?;
        Ok(match (draft, detail) {
            (Some(d), Some(detail)) => {
                match RecoveryDraftDelta::classify(&detail.title, &d.draft_title) {
                    RecoveryDraftDelta::AlreadyPersisted => {
                        // CAS on `draft_at` so a concurrent record_draft
                        // that just refreshed the row survives.
                        match recovery::discard_draft(&mut db, &story_id, Some(&d.draft_at)) {
                            Ok(()) => ReadOutcome::AlreadyPersisted,
                            Err(err) => ReadOutcome::AlreadyPersistedCleanupFailed(err),
                        }
                    }
                    RecoveryDraftDelta::Recoverable {
                        persisted_title,
                        draft_title,
                    } => ReadOutcome::Recoverable {
                        story_id: story_id.clone(),
                        draft_title,
                        draft_at: d.draft_at,
                        persisted_title,
                    },
                }
            }
            // FK CASCADE makes a draft without a parent story
            // essentially impossible, but if it ever happens
            // (FK off, hand-crafted DB), the missing parent dominates.
            _ => ReadOutcome::None,
        })
    })();

    match outcome {
        Ok(ReadOutcome::None | ReadOutcome::AlreadyPersisted) => Ok(RecoverableDraftDto::None),
        Ok(ReadOutcome::AlreadyPersistedCleanupFailed(err)) => {
            // The draft was already-persisted but we failed to
            // consume the row. The user-visible state is correct
            // (no banner) but the row will be re-evaluated next mount,
            // which is a buffered keystroke leaking past the autosave
            // confirm — log the failure so support sees it.
            let _ = recovery_log::record_event(
                &app,
                recovery_log::Event::RecoveryDraftUnavailable {
                    story_id: story_id.clone(),
                    source: "read_recoverable_draft_cleanup",
                },
            );
            // The user is still better served with `None` than with
            // an error: the persisted title is the truth, the buffer
            // mismatch is a janitor concern, not a UX-visible failure.
            // We swallow the underlying error code but log it.
            let _ = err;
            Ok(RecoverableDraftDto::None)
        }
        Ok(ReadOutcome::Recoverable {
            story_id: sid,
            draft_title,
            draft_at,
            persisted_title,
        }) => {
            let _ = recovery_log::record_event(
                &app,
                recovery_log::Event::RecoveryDraftProposed {
                    story_id: sid.clone(),
                },
            );
            Ok(RecoverableDraftDto::Recoverable {
                story_id: sid,
                draft_title,
                draft_at,
                persisted_title,
            })
        }
        Err(err) => {
            // Failure on either get_story_detail or read_recoverable_draft.
            // P39/D4: wrap upstream LOCAL_STORAGE_UNAVAILABLE into a
            // RECOVERY_DRAFT_UNAVAILABLE that preserves the original
            // diagnostic discriminants in `details.cause` so support
            // can pinpoint the failure source.
            let wrapped = wrap_recovery_read_error(err, &story_id);
            let _ = recovery_log::record_event(
                &app,
                recovery_log::Event::RecoveryDraftUnavailable {
                    story_id: story_id.clone(),
                    source: "read_recoverable_draft",
                },
            );
            Err(wrapped)
        }
    }
}

/// Wrap an upstream `AppError` from `get_story_detail` /
/// `read_recoverable_draft` into a recovery-flow-tagged
/// `RECOVERY_DRAFT_UNAVAILABLE`. The original error code, source, and
/// details survive under `details.cause` so support keeps the
/// triage signal.
fn wrap_recovery_read_error(upstream: AppError, story_id: &str) -> AppError {
    let cause_code = serde_json::to_value(&upstream.code)
        .ok()
        .unwrap_or(serde_json::Value::Null);
    let cause_details = upstream.details.clone().unwrap_or(serde_json::Value::Null);
    AppError::recovery_draft_unavailable(
        "Récupération indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "read_recoverable_draft",
        "stage": "get_story_detail_or_read_draft",
        "id": story_id,
        "cause": {
            "code": cause_code,
            "details": cause_details,
        },
    }))
}

/// Apply the buffered draft authoritatively: re-validate, UPDATE the
/// canonical row, consume the draft — all in a single transaction.
#[tauri::command]
pub fn apply_recovery(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ApplyRecoveryInputDto,
) -> Result<UpdateStoryOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let story_id_for_log = input.story_id.clone();
    let outcome = {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recovery::apply_recovery(
            &mut db,
            RecoveryApplyInput {
                story_id: input.story_id,
            },
        )
    };
    let event = if outcome.is_ok() {
        recovery_log::Event::RecoveryDraftApplied {
            story_id: story_id_for_log,
        }
    } else {
        recovery_log::Event::RecoveryDraftUnavailable {
            story_id: story_id_for_log,
            source: "apply_recovery",
        }
    };
    let _ = recovery_log::record_event(&app, event);
    outcome
}

/// Drop the buffered draft without modifying canonical state. Idempotent
/// by design — a second call on an already-empty row is a silent no-op.
///
/// The optional `expected_draft_at` is forwarded as a compare-and-swap
/// guard: when the UI passes the timestamp it observed, a concurrent
/// `record_draft` that refreshed the row between observation and click
/// is preserved. When absent (auto-discard from `useStoryEditor` when
/// the user types back to the persisted value), the DELETE runs
/// unconditionally because the caller does not know which `draft_at`
/// to expect — the trade-off is acceptable on that path because the
/// canonical row already matches the persisted truth, so dropping
/// any buffered value is correct.
#[tauri::command]
pub fn discard_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    input: DiscardDraftInputDto,
) -> Result<(), AppError> {
    validate_story_id(&input.story_id)?;
    let story_id_for_log = input.story_id.clone();
    let outcome = {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recovery::discard_draft(&mut db, &input.story_id, input.expected_draft_at.as_deref())
    };
    let event = if outcome.is_ok() {
        recovery_log::Event::RecoveryDraftDiscarded {
            story_id: story_id_for_log,
        }
    } else {
        recovery_log::Event::RecoveryDraftUnavailable {
            story_id: story_id_for_log,
            source: "discard_draft",
        }
    };
    let _ = recovery_log::record_event(&app, event);
    outcome
}
