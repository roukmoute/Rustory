use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, State};

use crate::application::device::import::ImportDeviceStoryRequest;
use crate::application::device::library::DeviceLibraryOutcome;
use crate::application::device::title::{resolve_local_truth, set_user_title, LocalTruth};
use crate::application::device::{self, ConnectedLuniiOutcome};
use crate::domain::device::is_canonical_pack_uuid;
use crate::domain::device::title::PackTitle;
use crate::domain::shared::AppError;
use crate::infrastructure::device::{MountAttempt, MountOutcome};
use crate::infrastructure::diagnostics::device_log;
use crate::ipc::dto::{
    ConnectedDeviceDto, DeviceLibraryDto, DeviceStoryTitleDto, ImportDeviceStoryInputDto,
    ImportDeviceStoryOutcomeDto, ReadStoryValidationInputDto, ReadTransferPreviewInputDto,
    SetDeviceStoryTitleInputDto, StoryValidationDto, TransferPreviewDto,
};
use crate::AppState;

/// Wall-clock budget for the device scan. Sized below the NFR4 budget
/// of 5 s with a safety margin so the IPC marshalling and the front-end
/// timer (≈ 4500 ms) cooperate without flapping.
pub const DEVICE_SCAN_BUDGET: Duration = Duration::from_millis(4000);

/// Wall-clock budget for the device-library read. Covers the
/// authoritative re-scan (auto-mount + sysinfo) AND the inventory read
/// of `.pi` / `.pi.hidden` / `.content` at the mount path. Sized with a
/// margin under the front-end timer so the two cooperate without
/// flapping.
pub const DEVICE_LIBRARY_READ_BUDGET: Duration = Duration::from_millis(5000);

/// Wall-clock budget for a device-story import. A pack can weigh
/// hundreds of MB on a slow USB bus, so this budget is deliberately
/// orders of magnitude above the read budgets. The frontend sets NO
/// timer of its own (Rust owns the bound, like the export flow); the
/// deadline is re-checked between files and between copy chunks.
pub const IMPORT_DEVICE_STORY_BUDGET: Duration = Duration::from_secs(300);

/// Read the currently-connected supported device (Lunii, MVP).
///
/// Async by design: the underlying filesystem scan can take seconds on
/// adversarial mounts and would freeze a sync handler. The actual
/// blocking work (D-Bus auto-mount + sysinfo enumeration + per-mount
/// FS reads) runs on a `tauri::async_runtime::spawn_blocking` worker
/// so the async runtime stays free for other IPC traffic and the UI
/// keeps painting. The DB mutex is NOT held during the scan —
/// autosave/export keep working in parallel.
#[tauri::command]
pub async fn read_connected_lunii(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ConnectedDeviceDto, AppError> {
    // Clone the Arc into the closure so the blocking worker owns its
    // own handle for the whole call without borrowing from `state`.
    let scanner = state.device_scanner.clone();
    let started = Instant::now();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        device::read_connected_lunii_with_attempts(scanner.as_ref(), DEVICE_SCAN_BUDGET)
    })
    .await
    .map_err(|_| {
        AppError::device_scan_failed(
            "Détection indisponible: tâche d'analyse interrompue.",
            "Réessaie la détection ; si le problème persiste, redémarre Rustory.",
        )
        .with_details(serde_json::json!({
            "source": "spawn_blocking_join",
        }))
    })?;

    // Surface every Mounted / Failed auto-mount attempt in the device
    // log so support can correlate "Lunii was plugged in but the
    // scanner reported nothing" with "we tried to mount it and the OS
    // refused". Skipped attempts (volume already mounted or filtered
    // out) are intentionally NOT logged — they would drown the signal
    // on every poll iteration.
    if let Ok((_, ref attempts)) = outcome {
        for attempt in attempts {
            if let Some(ev) = automount_event_for(attempt) {
                let _ = device_log::record_event(&app, ev);
            }
        }
    }

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match &outcome {
        Ok((ConnectedLuniiOutcome::None, _)) => {
            Some(device_log::Event::DeviceAbsent { elapsed_ms })
        }
        Ok((ConnectedLuniiOutcome::Supported(profile), _)) => {
            Some(device_log::Event::DeviceDetectedSupported {
                device_identifier: profile.device_identifier.clone(),
                firmware_cohort: profile.firmware_cohort.diagnostic_tag(),
                metadata_format_version: profile.metadata_format_version,
                elapsed_ms,
            })
        }
        Ok((
            ConnectedLuniiOutcome::Unsupported {
                reason,
                firmware_hint,
            },
            _,
        )) => Some(device_log::Event::DeviceDetectedUnsupported {
            reason: reason.diagnostic_tag(),
            firmware_hint: firmware_hint.clone(),
            elapsed_ms,
        }),
        Ok((ConnectedLuniiOutcome::Ambiguous { candidate_count }, _)) => {
            Some(device_log::Event::DeviceDetectedUnsupported {
                reason: "multiple_candidates",
                firmware_hint: Some(format!("count_{candidate_count}")),
                elapsed_ms,
            })
        }
        Err(err) => Some(device_log::Event::DeviceScanFailed {
            source: scan_failure_source(err),
            kind: scan_failure_kind(err),
            elapsed_ms,
        }),
    };
    if let Some(ev) = event {
        let _ = device_log::record_event(&app, ev);
    }

    outcome.map(|(o, _)| ConnectedDeviceDto::from_outcome(o))
}

fn automount_event_for(attempt: &MountAttempt) -> Option<device_log::Event> {
    let device_class = classify_device_path(&attempt.device);
    match &attempt.outcome {
        MountOutcome::Mounted { .. } => Some(device_log::Event::DeviceAutomounted { device_class }),
        MountOutcome::Failed { reason } => Some(device_log::Event::DeviceAutomountFailed {
            device_class,
            reason,
        }),
        // AlreadyMounted and Skipped are not surfaced — they fire on
        // every poll and would crowd out the signal.
        MountOutcome::AlreadyMounted | MountOutcome::Skipped { .. } => None,
    }
}

/// PII-free bucketing of a raw `/dev/<name>` path into a closed-set
/// device class token. Strips trailing partition digits so a hotplug
/// that lands on a different partition number still groups under the
/// same class. `unknown` is the catch-all for anything that does not
/// look like a Linux block device path.
fn classify_device_path(path: &str) -> &'static str {
    let Some(stripped) = path.strip_prefix("/dev/") else {
        return "unknown";
    };
    let base: String = stripped
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    match base.as_str() {
        "sd" => "sd_block",
        "nvme" => "nvme_block",
        "mmcblk" => "mmc_block",
        "loop" => "loop_block",
        _ if base.is_empty() => "unknown",
        _ => "other_block",
    }
}

fn scan_failure_source(err: &AppError) -> &'static str {
    err.details
        .as_ref()
        .and_then(|d| d.get("source").and_then(|s| s.as_str()))
        .map(|s| match s {
            "scan_timeout" => "scan_timeout",
            "fs_read" => "fs_read",
            "os_enum" => "os_enum",
            "spawn_blocking_join" => "spawn_blocking_join",
            _ => "other",
        })
        .unwrap_or("other")
}

/// Preserve the upstream `details.kind` token (closed-set: e.g.
/// `permission_denied`, `timeout`) so support can triage a scan
/// failure without parsing the user-facing message. Returns `None`
/// when the upstream payload did not carry a `kind`.
fn scan_failure_kind(err: &AppError) -> Option<String> {
    err.details
        .as_ref()?
        .get("kind")?
        .as_str()
        .map(str::to_string)
}

/// Read the installed-pack inventory of the connected supported Lunii
/// identified by `device_identifier`.
///
/// Async + `spawn_blocking` like [`read_connected_lunii`]: the work
/// re-scans the device (D-Bus auto-mount + sysinfo enumeration) and reads
/// the index files at the mount path — all blocking I/O kept off the
/// async runtime so the UI keeps painting. The DB mutex is NOT held, so
/// autosave/export keep working while the read is in flight.
///
/// The `device_identifier` is validated Rust-side: the live re-scan must
/// resolve to a supported Lunii whose identifier matches, otherwise a
/// recoverable `DEVICE_SCAN_FAILED` is returned (device swapped/unplugged).
#[tauri::command]
pub async fn read_device_library(
    app: AppHandle,
    state: State<'_, AppState>,
    device_identifier: String,
) -> Result<DeviceLibraryDto, AppError> {
    let scanner = state.device_scanner.clone();
    let reader = state.library_reader.clone();
    let started = Instant::now();
    let requested = device_identifier;

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        device::library::read_device_library(
            scanner.as_ref(),
            reader.as_ref(),
            &requested,
            DEVICE_LIBRARY_READ_BUDGET,
        )
    })
    .await
    .map_err(|_| {
        AppError::device_scan_failed(
            "Lecture de la bibliothèque appareil indisponible: tâche interrompue.",
            "Réessaie la lecture ; si le problème persiste, redémarre Rustory.",
        )
        .with_details(serde_json::json!({
            "source": "spawn_blocking_join",
        }))
    })?;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match &outcome {
        Ok(DeviceLibraryOutcome::Readable {
            device_identifier,
            library,
        }) => Some(device_log::Event::DeviceLibraryRead {
            device_identifier: device_identifier.clone(),
            story_count: library.entries.len() as u32,
            hidden_count: library.entries.iter().filter(|e| e.hidden).count() as u32,
            elapsed_ms,
        }),
        // None / Unsupported are rare here (the UI only calls this for a
        // supported device) and the detection poll already surfaces them;
        // skip logging to keep the diagnostic stream quiet.
        Ok(DeviceLibraryOutcome::None) | Ok(DeviceLibraryOutcome::Unsupported { .. }) => None,
        Err(err) => Some(device_log::Event::DeviceLibraryReadFailed {
            source: library_failure_source(err),
            kind: scan_failure_kind(err),
            elapsed_ms,
        }),
    };
    if let Some(ev) = event {
        let _ = device_log::record_event(&app, ev);
    }

    let outcome = outcome?;

    // Compose local truth onto the device inventory AFTER the device I/O:
    // which packs are already imported, and the recognized title + provenance
    // of each. The scoped DB lock is taken here, once the device read has
    // returned — never held across the I/O — and the query is bounded to the
    // device's own pack UUIDs. Fail-closed: a local-store read failure
    // surfaces a recoverable error rather than lying about local truth (which
    // would invite a duplicate copy and hide the user's own stories).
    let local_truth = match &outcome {
        DeviceLibraryOutcome::Readable { library, .. } => {
            let uuids: Vec<String> = library.entries.iter().map(|e| e.uuid.clone()).collect();
            resolve_device_local_truth(&state, &uuids)?
        }
        DeviceLibraryOutcome::None | DeviceLibraryOutcome::Unsupported { .. } => {
            LocalTruth::default()
        }
    };

    Ok(DeviceLibraryDto::from_outcome(
        outcome,
        &local_truth.imported,
        &local_truth.titles,
    ))
}

/// Resolve the already-imported set and the recognized titles for the given
/// device pack UUIDs under a scoped DB lock. Thin wrapper that owns the lock
/// so the application service stays Tauri-free and testable.
fn resolve_device_local_truth(
    state: &State<'_, AppState>,
    uuids: &[String],
) -> Result<LocalTruth, AppError> {
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    resolve_local_truth(&db, uuids)
}

/// Compose the read-only pre-transfer comparison for the selected local
/// story against the connected supported Lunii.
///
/// Async + `spawn_blocking` like every device command: it re-scans the
/// device, reads its inventory and composes the local↔device pack membership
/// — all blocking I/O kept off the async runtime so the UI keeps painting.
/// The DB mutex is taken in a SCOPED section INSIDE the service, AFTER the
/// device I/O, never held across it. Read-only: nothing is written, and no
/// `mount_path` crosses the IPC boundary.
#[tauri::command]
pub async fn read_transfer_preview(
    state: State<'_, AppState>,
    input: ReadTransferPreviewInputDto,
) -> Result<TransferPreviewDto, AppError> {
    // Both identifiers normally originate from Rust DTOs (selection + detection);
    // a malformed value is a frontend bug, refused explicitly rather than
    // "best-effort matched" against the device.
    crate::commands::shared::validate_story_id(&input.story_id)?;
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_transfer_preview_device_identifier());
    }

    let db = state.db.clone();
    let scanner = state.device_scanner.clone();
    let reader = state.library_reader.clone();
    let story_id = input.story_id;
    let requested = input.device_identifier;

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        device::transfer::read_transfer_preview(
            &db,
            scanner.as_ref(),
            reader.as_ref(),
            &story_id,
            &requested,
            DEVICE_LIBRARY_READ_BUDGET,
        )
    })
    .await
    .map_err(|_| transfer_preview_join_error())?;

    outcome.map(TransferPreviewDto::from_outcome)
}

/// The renderer sent a `device_identifier` that is not 32 lowercase hex — a
/// frontend bug (the value always originates from a Rust detection DTO).
/// Refused with the recoverable scan-failed category so the UI folds the
/// comparison and re-detects.
fn invalid_transfer_preview_device_identifier() -> AppError {
    AppError::device_scan_failed(
        "Comparaison impossible: identifiant d'appareil invalide.",
        "Relance la détection de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "kind": "invalid_input",
        "cause": "invalid_device_identifier",
    }))
}

/// The blocking comparison worker could not be joined (panicked or
/// cancelled). Mapped to the `spawn_blocking_join` source.
fn transfer_preview_join_error() -> AppError {
    AppError::device_scan_failed(
        "Comparaison indisponible: tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "spawn_blocking_join",
    }))
}

/// Compose the read-only pre-transfer validation verdict for the selected local
/// story against the connected supported Lunii.
///
/// Async + `spawn_blocking` like every device command: it re-scans the device,
/// reads the local canonical facts and composes the per-story verdict
/// (`présumée transférable` / `à corriger` / `bloquée`) — all blocking I/O kept
/// off the async runtime so the UI keeps painting. The DB mutex is taken in a
/// SCOPED section INSIDE the service, AFTER the device I/O, never held across
/// it. Read-only: nothing is written, no `validation_status` is persisted, and
/// no `mount_path` crosses the IPC boundary. The verdict is ORTHOGONAL to the
/// `WriteStory` gate — the send CTA stays disabled in MVP regardless.
#[tauri::command]
pub async fn read_story_validation(
    state: State<'_, AppState>,
    input: ReadStoryValidationInputDto,
) -> Result<StoryValidationDto, AppError> {
    // Both identifiers normally originate from Rust DTOs (selection + detection);
    // a malformed value is a frontend bug, refused explicitly rather than
    // "best-effort matched" against the device.
    crate::commands::shared::validate_story_id(&input.story_id)?;
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_story_validation_device_identifier());
    }

    let db = state.db.clone();
    let scanner = state.device_scanner.clone();
    let reader = state.library_reader.clone();
    let story_id = input.story_id;
    let requested = input.device_identifier;

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        device::preflight::read_story_validation(
            &db,
            scanner.as_ref(),
            reader.as_ref(),
            &story_id,
            &requested,
            DEVICE_LIBRARY_READ_BUDGET,
        )
    })
    .await
    .map_err(|_| story_validation_join_error())?;

    outcome.map(StoryValidationDto::from_outcome)
}

/// The renderer sent a `device_identifier` that is not 32 lowercase hex — a
/// frontend bug (the value always originates from a Rust detection DTO).
/// Refused with the recoverable scan-failed category so the UI folds the
/// validation and re-detects. Named (not inline) so the actionability
/// discipline test can assert its copy.
fn invalid_story_validation_device_identifier() -> AppError {
    AppError::device_scan_failed(
        "Validation impossible: identifiant d'appareil invalide.",
        "Relance la détection de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "kind": "invalid_input",
        "cause": "invalid_device_identifier",
    }))
}

/// The blocking validation worker could not be joined (panicked or cancelled).
/// Mapped to the `spawn_blocking_join` source. Named (not inline) so the
/// actionability discipline test can assert its copy.
fn story_validation_join_error() -> AppError {
    AppError::device_scan_failed(
        "Validation indisponible: tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "spawn_blocking_join",
    }))
}

/// Copy the device story identified by `packUuid` from the connected
/// supported Lunii identified by `deviceIdentifier` into the local
/// library ("Copier dans ma bibliothèque").
///
/// Async + `spawn_blocking` like every device command: the whole
/// acquisition sequence (re-scan, index re-read, bounded copy, atomic
/// promotion, canonical commit) runs on a blocking worker that owns
/// Arc handles — the DB mutex is locked in SCOPED sections inside the
/// service, never across device I/O, and never across an await.
///
/// The command receives exactly two identifiers; Rust re-resolves the
/// mount path, the short id and every other detail itself. No path
/// crosses the IPC boundary in either direction.
#[tauri::command]
pub async fn import_device_story(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ImportDeviceStoryInputDto,
) -> Result<ImportDeviceStoryOutcomeDto, AppError> {
    validate_import_input(&input)?;

    let db = state.db.clone();
    let scanner = state.device_scanner.clone();
    let library_reader = state.library_reader.clone();
    let pack_reader = state.pack_reader.clone();
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| import_app_data_unavailable_error())?;
    let request = ImportDeviceStoryRequest {
        device_identifier: input.device_identifier,
        pack_uuid: input.pack_uuid,
    };
    let started = Instant::now();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        device::import::import_device_story(
            &db,
            scanner.as_ref(),
            library_reader.as_ref(),
            pack_reader.as_ref(),
            &app_data_dir,
            &request,
            IMPORT_DEVICE_STORY_BUDGET,
        )
    })
    .await
    .map_err(|_| import_join_error())?;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match &outcome {
        Ok(imported) => device_log::Event::DeviceStoryImported {
            short_id: imported.pack_short_id.clone(),
            story_id: imported.story.id.clone(),
            elapsed_ms,
            bytes_copied: imported.pack_total_bytes,
            file_count: imported.pack_file_count,
        },
        Err(err) => device_log::Event::DeviceStoryImportFailed {
            source: import_failure_source(err),
            kind: scan_failure_kind(err),
            elapsed_ms,
        },
    };
    let _ = device_log::record_event(&app, event);

    outcome.map(ImportDeviceStoryOutcomeDto::from_outcome)
}

/// Name (or rename) a device story that no catalog recognizes.
///
/// Synchronous: a single bounded SQLite write, no device I/O. Reuses the
/// local-story title rules (NFC + trim + denylist + ≤120) and stores the
/// title with `source = User`, so the resolution order guarantees it is
/// never silently overwritten by a later official/community recognition.
/// The UI re-reads the device library afterwards to surface the new title
/// from the single Rust-owned resolution.
#[tauri::command]
pub fn set_device_story_title(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: SetDeviceStoryTitleInputDto,
) -> Result<DeviceStoryTitleDto, AppError> {
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let stored: PackTitle = set_user_title(&mut db, &input.pack_uuid, &input.title)?;
    Ok(DeviceStoryTitleDto::from_pack_title(stored))
}

/// Strict boundary validation of the import input. Both values normally
/// originate from Rust itself (detection + inventory DTOs), so a
/// malformed value is a frontend bug — refused explicitly, never
/// "best-effort matched" against the device.
fn validate_import_input(input: &ImportDeviceStoryInputDto) -> Result<(), AppError> {
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_import_input("invalid_device_identifier"));
    }
    if !is_canonical_pack_uuid(&input.pack_uuid) {
        return Err(invalid_import_input("invalid_pack_uuid"));
    }
    Ok(())
}

fn invalid_import_input(cause: &'static str) -> AppError {
    AppError::import_failed(
        "Copie impossible: requête invalide.",
        "Relance la lecture de la bibliothèque de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "kind": "invalid_input",
        "cause": cause,
    }))
}

/// `app_data_dir` could not be resolved — the local store has no home to
/// copy into. Mapped to the `other` fallback source. Named (not inline)
/// so the actionability discipline test can assert its copy.
fn import_app_data_unavailable_error() -> AppError {
    AppError::import_failed(
        "Copie impossible: stockage local introuvable.",
        "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "cause": "app_data_dir",
    }))
}

/// The blocking acquisition worker could not be joined (panicked or
/// cancelled). Mapped to the `spawn_blocking_join` source. Named (not
/// inline) so the actionability discipline test can assert its copy.
fn import_join_error() -> AppError {
    AppError::import_failed(
        "Copie impossible: tâche interrompue.",
        "Réessaie la copie ; si le problème persiste, redémarre Rustory.",
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

/// Closed-set mapping of the import failure `source` for the diagnostic
/// event. Mirrors the wire taxonomy; anything unmapped folds to `other`.
fn import_failure_source(err: &AppError) -> &'static str {
    err.details
        .as_ref()
        .and_then(|d| d.get("source").and_then(|s| s.as_str()))
        .map(|s| match s {
            "already_imported" => "already_imported",
            "pack_missing" => "pack_missing",
            "pack_invalid" => "pack_invalid",
            "pack_oversize" => "pack_oversize",
            "device_changed" => "device_changed",
            "fs_read" => "fs_read",
            "staging_write" => "staging_write",
            "promote" => "promote",
            "db_commit" => "db_commit",
            "read_timeout" => "read_timeout",
            "capability_gate" => "capability_gate",
            "spawn_blocking_join" => "spawn_blocking_join",
            _ => "other",
        })
        .unwrap_or("other")
}

/// Closed-set mapping of the device-library read failure `source` so the
/// diagnostic event carries a stable, greppable token rather than the
/// localized message.
fn library_failure_source(err: &AppError) -> &'static str {
    err.details
        .as_ref()
        .and_then(|d| d.get("source").and_then(|s| s.as_str()))
        .map(|s| match s {
            "fs_read" => "fs_read",
            "pack_index" => "pack_index",
            "read_timeout" => "read_timeout",
            "device_changed" => "device_changed",
            "mount_unavailable" => "mount_unavailable",
            "scan_timeout" => "scan_timeout",
            "os_enum" => "os_enum",
            "capability_gate" => "capability_gate",
            "spawn_blocking_join" => "spawn_blocking_join",
            _ => "other",
        })
        .unwrap_or("other")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Discipline: the command-layer import-refusal fallbacks must be
    /// ACTIONABLE — a non-empty cause AND a non-empty next gesture — like
    /// every other refusal (AC1, ui-states.md → actionability rule). No
    /// new error code / `details.source` is introduced; this only locks
    /// the canonical fr copy the existing constructors carry.
    #[test]
    fn command_layer_import_refusals_are_actionable() {
        let refusals = [
            import_app_data_unavailable_error(),
            import_join_error(),
            invalid_import_input("invalid_pack_uuid"),
        ];
        for err in &refusals {
            assert_eq!(
                err.code,
                crate::domain::shared::AppErrorCode::ImportFailed,
                "{err:?}"
            );
            assert!(!err.message.is_empty(), "refusal needs a cause: {err:?}");
            let action = err.user_action.as_deref().unwrap_or("");
            assert!(!action.is_empty(), "refusal needs a next gesture: {err:?}");
        }
    }

    /// Same discipline for the story-validation command-layer refusals: a
    /// non-empty cause AND a non-empty next gesture, with the recoverable
    /// `DEVICE_SCAN_FAILED` category (no new error code introduced).
    #[test]
    fn command_layer_story_validation_refusals_are_actionable() {
        let refusals = [
            invalid_story_validation_device_identifier(),
            story_validation_join_error(),
        ];
        for err in &refusals {
            assert_eq!(
                err.code,
                crate::domain::shared::AppErrorCode::DeviceScanFailed,
                "{err:?}"
            );
            assert!(!err.message.is_empty(), "refusal needs a cause: {err:?}");
            let action = err.user_action.as_deref().unwrap_or("");
            assert!(!action.is_empty(), "refusal needs a next gesture: {err:?}");
        }
    }
}
