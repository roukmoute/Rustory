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

use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::application::transfer::{
    read_transfer_state as read_transfer_state_service, transfer_story, PreparationEventEmitter,
    PreparationOutcome, TransferOutcome,
};
use crate::commands::device::{DEVICE_LIBRARY_READ_BUDGET, IMPORT_DEVICE_STORY_BUDGET};
use crate::domain::shared::AppError;
use crate::domain::transfer::PreparationPhase;
use crate::infrastructure::diagnostics::transfer as transfer_log;
use crate::ipc::dto::{
    PreparationStateDto, ReadPreparationStateInputDto, ReadTransferStateInputDto,
    StartPreparationAcceptedDto, StartPrepareStoryInputDto, StartTransferAcceptedDto,
    StartTransferStoryInputDto, TransferStateDto,
};
use crate::ipc::events::{
    JobCompletedEvent, JobFailedEvent, JobProgressEvent, EVENT_JOB_COMPLETED, EVENT_JOB_FAILED,
    EVENT_JOB_PROGRESS, JOB_TYPE_PREPARE_STORY, JOB_TYPE_TRANSFER_STORY, PREPARATION_FAILED_CODE,
    TRANSFER_FAILED_CODE,
};
use crate::AppState;

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
            },
        );
    }

    fn failed(&self, message: &str, user_action: &str, sequence: u64) {
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
        let emitter = TauriJobEmitter {
            app: task_app.clone(),
            job_id: emitter_job_id.clone(),
            story_id: emitter_story_id.clone(),
            job_type: JOB_TYPE_TRANSFER_STORY,
            error_code: TRANSFER_FAILED_CODE,
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
            Ok(TransferOutcome::Transferred { .. }) => transfer_log::Event::TransferCompleted {
                story_ref,
                elapsed_ms,
            },
            Ok(TransferOutcome::Retryable { cause }) => transfer_log::Event::TransferFailed {
                story_ref,
                cause: cause.diagnostic_tag(),
                elapsed_ms,
            },
            Ok(TransferOutcome::Transport { .. }) => transfer_log::Event::TransferFailed {
                story_ref,
                cause: "transport",
                elapsed_ms,
            },
            Err(_join) => {
                // The blocking worker panicked / was cancelled, so the service
                // never emitted a terminal event. Emit a terminal failure with an
                // always-winning sequence so the UI does not hang in "en
                // transfert" (idempotent consumers keep the highest sequence).
                // Defensive: the service is panic-free by construction.
                let _ = task_app.emit(
                    EVENT_JOB_FAILED,
                    JobFailedEvent {
                        job_id: emitter_job_id.clone(),
                        job_type: JOB_TYPE_TRANSFER_STORY.to_string(),
                        target_story_id: emitter_story_id.clone(),
                        sequence: u64::MAX,
                        error_code: TRANSFER_FAILED_CODE.to_string(),
                        error_message: "Envoi interrompu avant la fin.".to_string(),
                        user_action: "Relance l'envoi.".to_string(),
                    },
                );
                transfer_log::Event::TransferFailed {
                    story_ref,
                    cause: "interrupted",
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
