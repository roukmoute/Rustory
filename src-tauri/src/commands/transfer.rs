//! Transfer command layer (story preparation + device-write transfer).
//!
//! The thin Tauri boundary for both long-running flows. `start_prepare_story`
//! and `start_transfer_story` validate the input, generate a `job_id`, kick the
//! work onto a background task and return an acceptance IMMEDIATELY; progress is
//! reported through the typed `job:*` events. `read_preparation_state` and
//! `read_transfer_state` are the authoritative re-reads.
//!
//! The runtime lives HERE: the [`TauriJobEmitter`] is the only place that
//! constructs event payloads and calls `AppHandle::emit`; the application
//! services only ever see the
//! [`PreparationEventEmitter`](crate::application::transfer::PreparationEventEmitter)
//! trait (reused by both flows — its phase enum already carries `Transfer`). The
//! DB mutex is never held across the scan or the write (the services scope it),
//! the `WriteStory` gate is checked BEFORE any device mutation, and no
//! `mount_path` crosses the IPC boundary.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::application::transfer::{
    discard_transfer_outcome as discard_transfer_outcome_service,
    read_transfer_outcome as read_transfer_outcome_service,
    read_transfer_state as read_transfer_state_service, record_transfer_outcome, transfer_story,
    PreparationEventEmitter, PreparationOutcome, TransferOutcome,
};
use crate::commands::device::{DEVICE_LIBRARY_READ_BUDGET, IMPORT_DEVICE_STORY_BUDGET};
use crate::domain::shared::AppError;
use crate::domain::transfer::{
    PersistedTransferOutcome, PreparationPhase, TransferCompleteness, TransferFailureCause,
    VerifiedSummary, VerifyVerdict,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::diagnostics::transfer as transfer_log;
use crate::ipc::dto::{
    DiscardTransferOutcomeInputDto, PreparationStateDto, ReadPreparationStateInputDto,
    ReadTransferOutcomeInputDto, ReadTransferStateInputDto, StartPreparationAcceptedDto,
    StartPrepareStoryInputDto, StartTransferAcceptedDto, StartTransferStoryInputDto,
    TransferOutcomeDto, TransferStateDto,
};
use crate::ipc::events::{
    JobCompletedEvent, JobCompletedSummary, JobFailedEvent, JobProgressEvent, EVENT_JOB_COMPLETED,
    EVENT_JOB_FAILED, EVENT_JOB_PROGRESS, JOB_TYPE_PREPARE_STORY, JOB_TYPE_TRANSFER_STORY,
    PREPARATION_FAILED_CODE, TRANSFER_FAILED_CODE,
};
use crate::AppState;

/// Durable-memory context the TRANSFER emitter persists each terminal into BEFORE
/// emitting its event. `None` for the preparation flow (which has no `transfer_jobs`
/// row). Holding it on the emitter is what closes the `Abandonner` race: the row is
/// written under the same blocking worker that emits the terminal, so the UI can
/// never `discard` a row that does not exist yet.
struct TransferMemory {
    db: Arc<Mutex<DbHandle>>,
    device_identifier: String,
}

/// Tauri implementation of the job event sink, shared by the preparation and the
/// transfer flows. Closes over the correlation identifiers + the flow's
/// `job_type` / failure `error_code`; the application passes only the phase /
/// sequence / message. `emit` is best-effort (a dropped event surfaces via the
/// authoritative re-read).
struct TauriJobEmitter {
    app: AppHandle,
    job_id: String,
    story_id: String,
    job_type: &'static str,
    error_code: &'static str,
    /// Durable cross-session memory — `Some` for the transfer flow only.
    transfer_memory: Option<TransferMemory>,
}

impl PreparationEventEmitter for TauriJobEmitter {
    fn progress(&self, phase: PreparationPhase, progress: Option<f32>, sequence: u64) {
        let _ = self.app.emit(
            EVENT_JOB_PROGRESS,
            JobProgressEvent {
                job_id: self.job_id.clone(),
                job_type: self.job_type.to_string(),
                target_story_id: self.story_id.clone(),
                phase: phase.wire_tag().to_string(),
                progress,
                sequence,
                message: None,
            },
        );
    }

    fn completed(&self, sequence: u64) {
        let _ = self.app.emit(
            EVENT_JOB_COMPLETED,
            JobCompletedEvent {
                job_id: self.job_id.clone(),
                job_type: self.job_type.to_string(),
                target_story_id: self.story_id.clone(),
                sequence,
                summary: None,
            },
        );
    }

    fn completed_verified(&self, changed: &str, unchanged: &str, sequence: u64) {
        // Persist the verified terminal to the durable memory BEFORE emitting (F5:
        // the row exists before the UI can `Abandonner`). Then carry the AC2 summary
        // ON the terminal so the UI renders the success straight from the event —
        // never via a re-read with the now-stale pre-write device identifier (F1).
        self.persist_terminal(PersistedTransferOutcome::from_verified(VerifiedSummary {
            changed: changed.to_string(),
            unchanged: unchanged.to_string(),
        }));
        let _ = self.app.emit(
            EVENT_JOB_COMPLETED,
            JobCompletedEvent {
                job_id: self.job_id.clone(),
                job_type: self.job_type.to_string(),
                target_story_id: self.story_id.clone(),
                sequence,
                summary: Some(JobCompletedSummary {
                    changed: changed.to_string(),
                    unchanged: unchanged.to_string(),
                }),
            },
        );
    }

    fn failed(&self, message: &str, user_action: &str, sequence: u64) {
        self.emit_failed(message, user_action, None, None, None, sequence);
    }

    fn failed_with_completeness(
        &self,
        message: &str,
        user_action: &str,
        completeness: Option<&str>,
        cause: Option<&str>,
        sequence: u64,
    ) {
        // Persist the write-phase terminal BEFORE emitting (F5: race-free vs
        // Abandonner). The terminal kind / copy are re-derived from the closed tags.
        if let Some(outcome) = persisted_write_terminal(completeness, cause) {
            self.persist_terminal(outcome);
        }
        self.emit_failed(
            message,
            user_action,
            completeness.map(str::to_string),
            cause.map(str::to_string),
            None,
            sequence,
        );
    }

    fn failed_verify(&self, message: &str, user_action: &str, verdict: &str, sequence: u64) {
        // Persist the verify terminal BEFORE emitting (F5). A verify terminal carries
        // ONLY the verify verdict — no write-phase completeness/cause (those describe
        // how a WRITE ended, not a re-read).
        if let Some(outcome) = VerifyVerdict::from_diagnostic_tag(verdict)
            .and_then(PersistedTransferOutcome::from_verify_verdict)
        {
            self.persist_terminal(outcome);
        }
        self.emit_failed(
            message,
            user_action,
            None,
            None,
            Some(verdict.to_string()),
            sequence,
        );
    }
}

impl TauriJobEmitter {
    /// Persist a terminal to the durable `transfer_jobs` memory and trace the result.
    /// Called BEFORE the terminal event is emitted (F5: the row exists before the UI
    /// can `Abandonner`). Best-effort + transfer-only (a no-op when `transfer_memory`
    /// is `None`): a persistence failure is LOGGED (F3) and never blocks the terminal.
    fn persist_terminal(&self, outcome: PersistedTransferOutcome) {
        let Some(memory) = &self.transfer_memory else {
            return;
        };
        let terminal_kind = outcome.terminal_kind.wire_tag();
        let result = {
            let mut db = memory
                .db
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            record_transfer_outcome(
                &mut db,
                &self.story_id,
                &self.job_id,
                Some(&memory.device_identifier),
                &outcome,
            )
        };
        let story_ref = transfer_log::story_ref(&self.story_id);
        let event = match &result {
            Ok(()) => transfer_log::Event::TransferOutcomeRecorded {
                story_ref,
                terminal_kind,
            },
            Err(err) => transfer_log::Event::TransferOutcomeUnavailable {
                story_ref,
                source: outcome_unavailable_source(err),
            },
        };
        let _ = transfer_log::record_event(&self.app, event);
    }

    /// Build + emit the `job:failed` payload (best-effort). `completeness` is set
    /// only for the transfer flow (`"failed"` / `"incomplete"`); preparation
    /// passes `None`, so the field is omitted on the wire.
    fn emit_failed(
        &self,
        message: &str,
        user_action: &str,
        completeness: Option<String>,
        cause: Option<String>,
        verify_verdict: Option<String>,
        sequence: u64,
    ) {
        let _ = self.app.emit(
            EVENT_JOB_FAILED,
            JobFailedEvent {
                job_id: self.job_id.clone(),
                job_type: self.job_type.to_string(),
                target_story_id: self.story_id.clone(),
                sequence,
                error_code: self.error_code.to_string(),
                error_message: message.to_string(),
                user_action: user_action.to_string(),
                completeness,
                cause,
                verify_verdict,
            },
        );
    }
}

/// Start preparing the LOCAL story `storyId` for the device `deviceIdentifier`.
///
/// Returns an acceptance immediately; the job runs in the background and reports
/// `job:progress` → `job:completed` / `job:failed`. Preparation is LOCAL and
/// orthogonal to `WriteStory`: a successful `prepared` never enables the send.
#[tauri::command]
pub async fn start_prepare_story(
    app: AppHandle,
    state: State<'_, AppState>,
    input: StartPrepareStoryInputDto,
) -> Result<StartPreparationAcceptedDto, AppError> {
    // Both identifiers normally originate from Rust DTOs (selection + detection);
    // a malformed value is a frontend bug, refused explicitly.
    crate::commands::shared::validate_story_id(&input.story_id)?;
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_preparation_device_identifier());
    }

    // Resolve the local store home BEFORE accepting the job — a transport
    // failure here cannot even produce a terminal job state (PreparationFailed).
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| preparation_app_data_unavailable())?;

    let job_id = uuid::Uuid::now_v7().to_string();
    let story_id = input.story_id;
    let requested = input.device_identifier;

    let db = state.db.clone();
    let scanner = state.device_scanner.clone();
    let library_reader = state.library_reader.clone();
    let artifact_source = state.artifact_source.clone();

    let _ = transfer_log::record_event(
        &app,
        transfer_log::Event::PreparationStarted {
            story_ref: transfer_log::story_ref(&story_id),
        },
    );

    let task_app = app.clone();
    let emitter_job_id = job_id.clone();
    let emitter_story_id = story_id.clone();

    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let emitter = TauriJobEmitter {
            app: task_app.clone(),
            job_id: emitter_job_id.clone(),
            story_id: emitter_story_id.clone(),
            job_type: JOB_TYPE_PREPARE_STORY,
            error_code: PREPARATION_FAILED_CODE,
            // Preparation has no durable cross-session memory.
            transfer_memory: None,
        };
        let worker_story_id = emitter_story_id.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            crate::application::transfer::prepare_story(
                &db,
                scanner.as_ref(),
                library_reader.as_ref(),
                artifact_source.as_ref(),
                &app_data_dir,
                &worker_story_id,
                &requested,
                DEVICE_LIBRARY_READ_BUDGET,
                IMPORT_DEVICE_STORY_BUDGET,
                &emitter,
            )
        })
        .await;

        let elapsed_ms = started.elapsed().as_millis() as u64;
        let story_ref = transfer_log::story_ref(&emitter_story_id);
        let trace = match &outcome {
            Ok(PreparationOutcome::Prepared { .. }) => transfer_log::Event::PreparationCompleted {
                story_ref,
                elapsed_ms,
            },
            Ok(PreparationOutcome::Retryable { cause }) => transfer_log::Event::PreparationFailed {
                story_ref,
                cause: cause.diagnostic_tag(),
                elapsed_ms,
            },
            Ok(PreparationOutcome::Transport { .. }) => transfer_log::Event::PreparationFailed {
                story_ref,
                cause: "transport",
                elapsed_ms,
            },
            Err(_join) => {
                // The blocking worker panicked / was cancelled, so the service
                // never emitted a terminal event. Emit a terminal failure with
                // an always-winning sequence so the UI does not hang in
                // "preparing" (idempotent consumers keep the highest sequence).
                // Defensive: the service is panic-free by construction.
                let _ = task_app.emit(
                    EVENT_JOB_FAILED,
                    JobFailedEvent {
                        job_id: emitter_job_id.clone(),
                        job_type: JOB_TYPE_PREPARE_STORY.to_string(),
                        target_story_id: emitter_story_id.clone(),
                        sequence: u64::MAX,
                        error_code: PREPARATION_FAILED_CODE.to_string(),
                        error_message: "Préparation interrompue avant la fin.".to_string(),
                        user_action: "Relance la préparation.".to_string(),
                        completeness: None,
                        cause: None,
                        verify_verdict: None,
                    },
                );
                transfer_log::Event::PreparationFailed {
                    story_ref,
                    cause: "interrupted",
                    elapsed_ms,
                }
            }
        };
        let _ = transfer_log::record_event(&task_app, trace);
    });

    Ok(StartPreparationAcceptedDto { job_id, story_id })
}

/// Re-read the authoritative preparation state for `storyId` (re-derived on
/// demand — nothing is persisted). The frontend calls this on a terminal event
/// rather than reconstructing truth from the events alone.
#[tauri::command]
pub async fn read_preparation_state(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ReadPreparationStateInputDto,
) -> Result<PreparationStateDto, AppError> {
    crate::commands::shared::validate_story_id(&input.story_id)?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| preparation_app_data_unavailable())?;

    let db = state.db.clone();
    let scanner = state.device_scanner.clone();
    let library_reader = state.library_reader.clone();
    let artifact_source = state.artifact_source.clone();
    let story_id = input.story_id;

    let view = tauri::async_runtime::spawn_blocking(move || {
        crate::application::transfer::read_preparation_state(
            &db,
            scanner.as_ref(),
            library_reader.as_ref(),
            artifact_source.as_ref(),
            &app_data_dir,
            &story_id,
            DEVICE_LIBRARY_READ_BUDGET,
            IMPORT_DEVICE_STORY_BUDGET,
        )
    })
    .await
    .map_err(|_| preparation_join_error())?;

    view.map(PreparationStateDto::from_view)
}

/// Start transferring (WRITING) the prepared LOCAL story `storyId` to the device
/// `deviceIdentifier`.
///
/// Returns an acceptance immediately; the job runs in the background and reports
/// `job:progress` → `job:completed` / `job:failed`. The `WriteStory` gate is
/// checked BEFORE any device mutation (fail-closed); a successful terminal is the
/// HONEST non-success "écriture effectuée — vérification à venir", never a
/// verified success.
#[tauri::command]
pub async fn start_transfer_story(
    app: AppHandle,
    state: State<'_, AppState>,
    input: StartTransferStoryInputDto,
) -> Result<StartTransferAcceptedDto, AppError> {
    crate::commands::shared::validate_story_id(&input.story_id)?;
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_transfer_device_identifier());
    }

    // Resolve the local store home BEFORE accepting the job — a transport failure
    // here cannot even produce a terminal job state (TransferFailed).
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| transfer_app_data_unavailable())?;

    let job_id = uuid::Uuid::now_v7().to_string();
    let story_id = input.story_id;
    let requested = input.device_identifier;

    let db = state.db.clone();
    let scanner = state.device_scanner.clone();
    let library_reader = state.library_reader.clone();
    let artifact_source = state.artifact_source.clone();
    let pack_writer = state.pack_writer.clone();
    // The emitter persists each terminal into this handle BEFORE emitting (F5); a
    // separate handle covers the defensive `incomplete` of a panicked worker (the
    // blocking worker moves its own `db` clone, and the emitter is consumed by it).
    let persist_db = state.db.clone();
    let join_db = state.db.clone();

    let _ = transfer_log::record_event(
        &app,
        transfer_log::Event::TransferStarted {
            story_ref: transfer_log::story_ref(&story_id),
        },
    );

    let task_app = app.clone();
    let emitter_job_id = job_id.clone();
    let emitter_story_id = story_id.clone();

    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        // The targeted device id, kept for the durable-memory row (traceability only
        // — it is stale by construction once a write mutates `.pi`).
        let emitter_device_id = requested.clone();
        let join_device_id = requested.clone();
        let emitter = TauriJobEmitter {
            app: task_app.clone(),
            job_id: emitter_job_id.clone(),
            story_id: emitter_story_id.clone(),
            job_type: JOB_TYPE_TRANSFER_STORY,
            error_code: TRANSFER_FAILED_CODE,
            // The emitter persists each terminal into this memory BEFORE emitting,
            // closing the `Abandonner` race (F5).
            transfer_memory: Some(TransferMemory {
                db: persist_db,
                device_identifier: emitter_device_id,
            }),
        };
        let worker_story_id = emitter_story_id.clone();
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            transfer_story(
                &db,
                scanner.as_ref(),
                library_reader.as_ref(),
                artifact_source.as_ref(),
                pack_writer.as_ref(),
                &app_data_dir,
                &worker_story_id,
                &requested,
                DEVICE_LIBRARY_READ_BUDGET,
                IMPORT_DEVICE_STORY_BUDGET,
                &emitter,
            )
        })
        .await;

        let elapsed_ms = started.elapsed().as_millis() as u64;
        let story_ref = transfer_log::story_ref(&emitter_story_id);
        let trace = match &outcome {
            Ok(TransferOutcome::Verified { .. }) => transfer_log::Event::TransferCompleted {
                story_ref,
                verify_verdict: "verified",
                elapsed_ms,
            },
            // A verify terminal records the verdict, not a write-phase cause.
            Ok(TransferOutcome::Unverified { verdict }) => transfer_log::Event::TransferFailed {
                story_ref,
                cause: None,
                completeness: None,
                verify_verdict: Some(verdict.diagnostic_tag()),
                elapsed_ms,
            },
            Ok(TransferOutcome::Retryable {
                cause,
                completeness,
            }) => transfer_log::Event::TransferFailed {
                story_ref,
                cause: Some(cause.diagnostic_tag()),
                completeness: Some(completeness.diagnostic_tag()),
                verify_verdict: None,
                elapsed_ms,
            },
            Ok(TransferOutcome::Transport { .. }) => transfer_log::Event::TransferFailed {
                story_ref,
                cause: Some("transport"),
                completeness: Some("failed"),
                verify_verdict: None,
                elapsed_ms,
            },
            Err(_join) => {
                // The blocking worker panicked / was cancelled, so the service
                // never emitted a terminal event. Emit a terminal failure with an
                // always-winning sequence so the UI does not hang in "en
                // transfert" (idempotent consumers keep the highest sequence).
                // Defensive: the service is panic-free by construction.
                // A panicked / cancelled worker is NON-CLASSIFIABLE: the write may
                // have begun mutating the device, so we must NOT claim it stayed
                // intact. Surface the honest `incomplete` (AC2) — a relance (full
                // cycle) restores a safe state — never a device-intact `échoué`.
                let _ = task_app.emit(
                    EVENT_JOB_FAILED,
                    JobFailedEvent {
                        job_id: emitter_job_id.clone(),
                        job_type: JOB_TYPE_TRANSFER_STORY.to_string(),
                        target_story_id: emitter_story_id.clone(),
                        sequence: u64::MAX,
                        error_code: TRANSFER_FAILED_CODE.to_string(),
                        error_message:
                            "Envoi interrompu : l'appareil peut contenir une copie partielle."
                                .to_string(),
                        user_action: "Relance l'envoi pour rétablir un état sûr.".to_string(),
                        completeness: Some("incomplete".to_string()),
                        cause: None,
                        verify_verdict: None,
                    },
                );
                // Defensive persist (inline, best-effort): the panicked worker never
                // ran the emitter, so mirror the emitted `incomplete` here so a restart
                // still re-offers a safe relaunch (AC2). The DB lock is held only for
                // the single-row UPSERT (no await), then released before the trace.
                let incomplete = PersistedTransferOutcome::from_write_terminal(
                    TransferFailureCause::Interrupted,
                    TransferCompleteness::Incomplete,
                );
                let persisted = {
                    let mut db = join_db
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    record_transfer_outcome(
                        &mut db,
                        &emitter_story_id,
                        &emitter_job_id,
                        Some(&join_device_id),
                        &incomplete,
                    )
                };
                let _ = transfer_log::record_event(
                    &task_app,
                    match &persisted {
                        Ok(()) => transfer_log::Event::TransferOutcomeRecorded {
                            story_ref: transfer_log::story_ref(&emitter_story_id),
                            terminal_kind: incomplete.terminal_kind.wire_tag(),
                        },
                        Err(err) => transfer_log::Event::TransferOutcomeUnavailable {
                            story_ref: transfer_log::story_ref(&emitter_story_id),
                            source: outcome_unavailable_source(err),
                        },
                    },
                );
                transfer_log::Event::TransferFailed {
                    story_ref,
                    cause: Some("interrupted"),
                    completeness: Some("incomplete"),
                    verify_verdict: None,
                    elapsed_ms,
                }
            }
        };
        let _ = transfer_log::record_event(&task_app, trace);
    });

    Ok(StartTransferAcceptedDto { job_id, story_id })
}

/// Re-read the authoritative transfer state for `storyId` (re-derived on demand —
/// nothing is persisted; the device is the truth). Returns `transferred` only
/// when the pack is present on the connected writable device, else `idle`.
#[tauri::command]
pub async fn read_transfer_state(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ReadTransferStateInputDto,
) -> Result<TransferStateDto, AppError> {
    crate::commands::shared::validate_story_id(&input.story_id)?;
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_transfer_device_identifier());
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| transfer_app_data_unavailable())?;

    let db = state.db.clone();
    let scanner = state.device_scanner.clone();
    let library_reader = state.library_reader.clone();
    let artifact_source = state.artifact_source.clone();
    let story_id = input.story_id;
    let requested = input.device_identifier;

    let view = tauri::async_runtime::spawn_blocking(move || {
        read_transfer_state_service(
            &db,
            scanner.as_ref(),
            library_reader.as_ref(),
            artifact_source.as_ref(),
            &app_data_dir,
            &story_id,
            &requested,
            DEVICE_LIBRARY_READ_BUDGET,
            IMPORT_DEVICE_STORY_BUDGET,
        )
    })
    .await
    .map_err(|_| transfer_join_error())?;

    view.map(TransferStateDto::from_view)
}

/// Read the durable transfer outcome remembered for `storyId` (the Transfer Resume
/// Contract). Read-only and BEST-EFFORT (§6): returns `null` when there is no memory
/// AND when a transport failure degrades the read — the degradation is LOGGED to
/// `transfer.jsonl` (so support sees it) but never rejects, so the panel is never
/// blocked. The hook re-hydrates its sticky non-success state from it on mount,
/// reconciled against the live `read_transfer_state` (the live `Verified` always wins).
#[tauri::command]
pub async fn read_transfer_outcome(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ReadTransferOutcomeInputDto,
) -> Result<Option<TransferOutcomeDto>, AppError> {
    crate::commands::shared::validate_story_id(&input.story_id)?;

    let db = state.db.clone();
    let story_id = input.story_id;
    let read_story_id = story_id.clone();

    let read = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        read_transfer_outcome_service(&db, &read_story_id)
    })
    .await;

    // §6: trace the degradation, then treat it as "no memory". Both a service
    // transport error (`sqlite_select`) and a worker join error (`spawn_blocking_join`)
    // resolve to `null` so a mount read never blocks the panel.
    let stored = match read {
        Ok(Ok(stored)) => stored,
        Ok(Err(err)) => {
            log_outcome_degraded(&app, &story_id, outcome_unavailable_source(&err));
            None
        }
        Err(_join) => {
            log_outcome_degraded(&app, &story_id, "spawn_blocking_join");
            None
        }
    };

    Ok(stored.map(|stored| TransferOutcomeDto::from_stored(story_id, stored)))
}

/// Best-effort `transfer.jsonl` trace of a durable-memory read/write degradation.
fn log_outcome_degraded(app: &AppHandle, story_id: &str, source: &'static str) {
    let _ = transfer_log::record_event(
        app,
        transfer_log::Event::TransferOutcomeUnavailable {
            story_ref: transfer_log::story_ref(story_id),
            source,
        },
    );
}

/// Purge the durable transfer outcome for `storyId` (the `Abandonner` gesture).
/// Idempotent; never touches canonical state. A purge failure IS surfaced (unlike
/// the best-effort read) so the user learns the memory could not be cleared.
#[tauri::command]
pub async fn discard_transfer_outcome(
    app: AppHandle,
    state: State<'_, AppState>,
    input: DiscardTransferOutcomeInputDto,
) -> Result<(), AppError> {
    crate::commands::shared::validate_story_id(&input.story_id)?;

    let db = state.db.clone();
    let story_id = input.story_id;
    let discard_story_id = story_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let mut db = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        discard_transfer_outcome_service(&mut db, &discard_story_id)
    })
    .await
    .map_err(|_| transfer_outcome_join_error())??;

    let _ = transfer_log::record_event(
        &app,
        transfer_log::Event::TransferOutcomeAbandoned {
            story_ref: transfer_log::story_ref(&story_id),
        },
    );
    Ok(())
}

/// Rebuild the persisted WRITE-phase terminal from the closed wire tags the emitter
/// carries (`completeness` = `"failed"` / `"incomplete"`, `cause` = the camelCase
/// `wire_cause`). `None` when either tag is absent / drifts — defensive, since the
/// service always passes both for a write-phase failure. The re-derived copy matches
/// the emitted message (both come from `failure_copy`).
fn persisted_write_terminal(
    completeness: Option<&str>,
    cause: Option<&str>,
) -> Option<PersistedTransferOutcome> {
    let completeness = TransferCompleteness::from_diagnostic_tag(completeness?)?;
    let cause = TransferFailureCause::from_wire_cause(cause?)?;
    Some(PersistedTransferOutcome::from_write_terminal(
        cause,
        completeness,
    ))
}

/// Extract the closed `details.source` of a durable-memory transport error for a
/// PII-free diagnostics tag (`sqlite_upsert` / `sqlite_select` / `sqlite_delete` /
/// `story_missing` / `other`).
fn outcome_unavailable_source(err: &AppError) -> &'static str {
    match err
        .details
        .as_ref()
        .and_then(|details| details.get("source"))
        .and_then(|source| source.as_str())
    {
        Some("sqlite_upsert") => "sqlite_upsert",
        Some("sqlite_select") => "sqlite_select",
        Some("sqlite_delete") => "sqlite_delete",
        Some("story_missing") => "story_missing",
        _ => "other",
    }
}

/// The blocking durable-memory worker could not be joined (panicked or cancelled).
fn transfer_outcome_join_error() -> AppError {
    AppError::transfer_outcome_unavailable(
        "Mémoire de transfert indisponible: tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "spawn_blocking_join",
    }))
}

fn is_32_lowercase_hex(value: &str) -> bool {
    value.len() == 32
        && value
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// The renderer sent a `device_identifier` that is not 32 lowercase hex — a
/// frontend bug. Refused with the recoverable scan-failed category so the UI
/// folds the preparation and re-detects.
fn invalid_preparation_device_identifier() -> AppError {
    AppError::device_scan_failed(
        "Préparation impossible: identifiant d'appareil invalide.",
        "Relance la détection de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "kind": "invalid_input",
        "cause": "invalid_device_identifier",
    }))
}

/// `app_data_dir` could not be resolved — the local store has no home, so the
/// preparation cannot even produce a terminal job state.
fn preparation_app_data_unavailable() -> AppError {
    AppError::preparation_failed(
        "Préparation impossible: stockage local introuvable.",
        "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "app_data_dir",
    }))
}

/// The blocking re-read worker could not be joined (panicked or cancelled).
fn preparation_join_error() -> AppError {
    AppError::preparation_failed(
        "Préparation indisponible: tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "spawn_blocking_join",
    }))
}

/// The renderer sent a `device_identifier` that is not 32 lowercase hex — a
/// frontend bug. Refused with the recoverable scan-failed category so the UI
/// folds the transfer and re-detects.
fn invalid_transfer_device_identifier() -> AppError {
    AppError::device_scan_failed(
        "Envoi impossible: identifiant d'appareil invalide.",
        "Relance la détection de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "kind": "invalid_input",
        "cause": "invalid_device_identifier",
    }))
}

/// `app_data_dir` could not be resolved — the local store has no home, so the
/// transfer cannot even produce a terminal job state.
fn transfer_app_data_unavailable() -> AppError {
    AppError::transfer_failed(
        "Envoi impossible: stockage local introuvable.",
        "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "app_data_dir",
    }))
}

/// The blocking re-read worker could not be joined (panicked or cancelled).
fn transfer_join_error() -> AppError {
    AppError::transfer_failed(
        "Envoi indisponible: tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "spawn_blocking_join",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_non_hex_device_identifier() {
        assert!(!is_32_lowercase_hex("not-hex")); // wrong length
        assert!(!is_32_lowercase_hex("ABCDEF0123456789ABCDEF0123456789")); // uppercase
                                                                           // Right length (32) but a non-hex character ('g') must still be refused.
        assert!(!is_32_lowercase_hex("0123456789abcdef0123456789abcdeg"));
        assert!(is_32_lowercase_hex("0123456789abcdef0123456789abcdef"));
    }

    /// Discipline: the command-layer preparation refusals must be ACTIONABLE — a
    /// non-empty cause AND a non-empty next gesture — like every other refusal.
    #[test]
    fn command_layer_preparation_refusals_are_actionable() {
        let refusals = [
            invalid_preparation_device_identifier(),
            preparation_app_data_unavailable(),
            preparation_join_error(),
            invalid_transfer_device_identifier(),
            transfer_app_data_unavailable(),
            transfer_join_error(),
        ];
        for err in &refusals {
            assert!(!err.message.is_empty(), "refusal needs a cause: {err:?}");
            let action = err.user_action.as_deref().unwrap_or("");
            assert!(!action.is_empty(), "refusal needs a next gesture: {err:?}");
        }
    }
}
