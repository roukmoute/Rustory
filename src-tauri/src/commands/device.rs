use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::application::device::delete::DeleteDeviceStoryRequest;
use crate::application::device::import::ImportDeviceStoryRequest;
use crate::application::device::library::DeviceLibraryOutcome;
use crate::application::device::reorder::ReorderDeviceStoriesRequest;
use crate::application::device::send::{SendArchiveRequest, SendStoryPackRequest};
use crate::application::device::story_pack::plan_story_pack;
use crate::application::device::title::{resolve_local_truth, set_user_title, LocalTruth};
use crate::application::device::{self, ConnectedLuniiOutcome};
use crate::domain::device::is_canonical_pack_uuid;
use crate::domain::device::title::PackTitle;
use crate::domain::shared::AppError;
use crate::infrastructure::device::{MountAttempt, MountOutcome};
use crate::infrastructure::diagnostics::device_log;
use crate::ipc::dto::{
    ConnectedDeviceDto, DeleteDeviceStoryInputDto, DeleteDeviceStoryOutcomeDto, DeviceLibraryDto,
    DeviceStoryTitleDto, ImportDeviceStoryInputDto, ImportDeviceStoryOutcomeDto,
    ReadStoryValidationInputDto, ReadTransferPreviewInputDto, ReorderDeviceStoriesInputDto,
    ReorderDeviceStoriesOutcomeDto, SendPackToDeviceInputDto, SendPackToDeviceOutcomeDto,
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

/// Wall-clock budget for a device-story delete. A delete is a small `.pi`
/// rewrite plus a content-folder removal (no bulk copy), so a tighter bound
/// than the import budget is ample; it still covers a slow USB unlink of a
/// large pack folder.
pub const DELETE_DEVICE_STORY_BUDGET: Duration = Duration::from_secs(60);

/// Budget of a device reorder: a re-scan plus one small index rewrite.
pub const REORDER_DEVICE_STORIES_BUDGET: Duration = Duration::from_secs(60);

/// Upper bound on the packs of one reorder (a `.pi` holds a few dozen).
const MAX_REORDER_PACKS: usize = 4096;

/// Wall-clock budget for a pack-archive send. A pack can weigh hundreds of
/// MB and the whole pipeline (archive read, transcode, cipher, staged copy
/// to a slow USB bus, fsync, promotion) runs inside it — same order of
/// magnitude as the import budget, and Rust owns the bound (the frontend
/// sets no timer of its own).
pub const SEND_PACK_TO_DEVICE_BUDGET: Duration = Duration::from_secs(300);

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
            family,
            firmware_cohort,
            library,
        }) => Some(device_log::Event::DeviceLibraryRead {
            device_identifier: device_identifier.clone(),
            family: family.diagnostic_tag(),
            firmware_cohort: firmware_cohort.diagnostic_tag(),
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
            family: imported.family.diagnostic_tag(),
            firmware_cohort: imported.firmware_cohort.diagnostic_tag(),
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

/// Delete the device story identified by `packUuid` from the connected
/// supported device identified by `deviceIdentifier` ("Supprimer de
/// l'appareil").
///
/// A DEVICE MUTATION, so the same discipline as the import/transfer commands:
/// async + `spawn_blocking`, an authoritative re-scan + capability gate BEFORE
/// any byte is touched, and exactly two identifiers across the boundary (Rust
/// re-resolves the mount path and the content folder itself — no path crosses
/// IPC). The gate here is the `delete_story` capability, distinct from
/// `write_story`: a V3 may delete even though it may not (yet) be written to.
#[tauri::command]
pub async fn delete_device_story(
    app: AppHandle,
    state: State<'_, AppState>,
    input: DeleteDeviceStoryInputDto,
) -> Result<DeleteDeviceStoryOutcomeDto, AppError> {
    validate_delete_input(&input)?;

    let scanner = state.device_scanner.clone();
    let deleter = state.pack_deleter.clone();
    let request = DeleteDeviceStoryRequest {
        device_identifier: input.device_identifier,
        pack_uuid: input.pack_uuid,
    };
    let started = Instant::now();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        device::delete::delete_device_story(
            scanner.as_ref(),
            deleter.as_ref(),
            &request,
            DELETE_DEVICE_STORY_BUDGET,
        )
    })
    .await
    .map_err(|_| delete_join_error())?;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match &outcome {
        Ok(deleted) => device_log::Event::DeviceStoryDeleted {
            family: deleted.family.diagnostic_tag(),
            firmware_cohort: deleted.firmware_cohort.diagnostic_tag(),
            was_present: deleted.was_present,
            elapsed_ms,
        },
        Err(err) => device_log::Event::DeviceStoryDeleteFailed {
            source: delete_failure_source(err),
            elapsed_ms,
        },
    };
    let _ = device_log::record_event(&app, event);

    outcome.map(DeleteDeviceStoryOutcomeDto::from_outcome)
}

/// Reorder the stories of the connected supported device identified by
/// `deviceIdentifier` — the wheel order — to `orderedPackUuids`, the
/// COMPLETE list of its visible packs in the new order.
///
/// A DEVICE MUTATION of the index only, with the delete command's
/// discipline: async + `spawn_blocking`, authoritative re-scan + the
/// `reorder_stories` capability gate BEFORE any byte is touched; the order
/// must match exactly what the device lists (a stale list is refused with a
/// re-read hint, never guessed around).
#[tauri::command]
pub async fn reorder_device_stories(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ReorderDeviceStoriesInputDto,
) -> Result<ReorderDeviceStoriesOutcomeDto, AppError> {
    validate_reorder_input(&input)?;

    let scanner = state.device_scanner.clone();
    let reorderer = state.pack_reorderer.clone();
    let request = ReorderDeviceStoriesRequest {
        device_identifier: input.device_identifier,
        ordered_pack_uuids: input.ordered_pack_uuids,
    };
    let started = Instant::now();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        device::reorder::reorder_device_stories(
            scanner.as_ref(),
            reorderer.as_ref(),
            &request,
            REORDER_DEVICE_STORIES_BUDGET,
        )
    })
    .await
    .map_err(|_| reorder_join_error())?;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match &outcome {
        Ok(reordered) => device_log::Event::DeviceStoriesReordered {
            family: reordered.family.diagnostic_tag(),
            firmware_cohort: reordered.firmware_cohort.diagnostic_tag(),
            count: reordered.count as u32,
            changed: reordered.changed,
            elapsed_ms,
        },
        Err(err) => device_log::Event::DeviceStoriesReorderFailed {
            source: reorder_failure_source(err),
            elapsed_ms,
        },
    };
    let _ = device_log::record_event(&app, event);

    outcome.map(ReorderDeviceStoriesOutcomeDto::from_outcome)
}

/// Where the pack of a send comes from (resolved by Rust, never by the UI).
enum PackSource {
    /// The story's retained source archive.
    Archive(std::path::PathBuf),
    /// A pack synthesized from the story's structure, its assets in the
    /// node-media store.
    Story {
        pack: crate::domain::device::StudioStoryPack,
        media_dir: std::path::PathBuf,
    },
}

/// Send the SELECTED local story identified by `storyId` to the connected
/// supported device identified by `deviceIdentifier` — the V3 branch of the
/// single "Envoyer vers la Lunii" gesture.
///
/// A DEVICE MUTATION with the same discipline as the delete command: async +
/// `spawn_blocking`, authoritative re-scan + capability gate BEFORE any byte is
/// touched. Exactly two identifiers cross the boundary; Rust resolves the
/// pack's SOURCE itself — no path crosses IPC, and no file picker:
///
/// - a story that RETAINED its source archive (`source-archives/<storyId>.zip`,
///   kept at import) is sent from that archive;
/// - any other story (created from a web page, an RSS feed, a folder or the
///   editor) has its pack SYNTHESIZED from its structure and node media
///   (sequential playback of its episodes) — planned under the SQLite lock,
///   which is RELEASED before any file or device I/O.
///
/// The gate is the DEDICATED `send_archive` capability, distinct from
/// `write_story`: a V3 receives packs through this engine (transcode +
/// re-cipher for the target `.md`) while the library round-trip stays closed.
/// A story that cannot become a pack (an episode without audio, a story with
/// choices, a device-copied pack) is refused with an actionable message BEFORE
/// any device touch — the same reason its library card announces.
#[tauri::command]
pub async fn send_pack_to_device(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SendPackToDeviceInputDto,
    // Streams the send's integer percent (0..99) so a big pack never looks
    // frozen. Progress is a SIGNAL only — the settled outcome is the return
    // value; a dropped channel never changes what was written.
    on_progress: Channel<u8>,
) -> Result<SendPackToDeviceOutcomeDto, AppError> {
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_send_input("invalid_device_identifier"));
    }
    crate::commands::shared::validate_story_id(&input.story_id)
        .map_err(|_| invalid_send_input("invalid_story_id"))?;

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| send_app_data_unavailable_error())?;
    let archive_path = crate::infrastructure::filesystem::resolve_source_archive_path(
        &app_data_dir,
        &input.story_id,
    );
    let source = if archive_path.is_file() {
        PackSource::Archive(archive_path)
    } else {
        // No retained archive: plan the synthesized pack from the library,
        // under the lock and without any other I/O; every refusal (no audio,
        // choices, device-copied pack) lands here, before any device touch.
        let pack = {
            let db = state
                .db
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            plan_story_pack(&db, &input.story_id)?
        };
        PackSource::Story {
            pack,
            media_dir: crate::infrastructure::filesystem::resolve_node_media_dir(&app_data_dir),
        }
    };

    let scanner = state.device_scanner.clone();
    let writer = state.pack_writer_v3.clone();
    let device_identifier = input.device_identifier;
    let started = Instant::now();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        // Forward only on an integer-percent CHANGE — the send already reports
        // integers, so this just dedups (bounded IPC, no per-file flood).
        let last = std::cell::Cell::new(-1i16);
        let forward = |pct: u8| {
            if i16::from(pct) != last.get() {
                last.set(i16::from(pct));
                let _ = on_progress.send(pct);
            }
        };
        match source {
            PackSource::Archive(archive_path) => device::send::send_archive_to_device(
                scanner.as_ref(),
                writer.as_ref(),
                &SendArchiveRequest {
                    device_identifier,
                    archive_path,
                },
                SEND_PACK_TO_DEVICE_BUDGET,
                &forward,
            ),
            PackSource::Story { pack, media_dir } => device::send::send_story_pack_to_device(
                scanner.as_ref(),
                writer.as_ref(),
                &SendStoryPackRequest {
                    device_identifier,
                    pack,
                    media_dir,
                },
                SEND_PACK_TO_DEVICE_BUDGET,
                &forward,
            ),
        }
    })
    .await
    .map_err(|_| send_join_error())?;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match &outcome {
        Ok(sent) => device_log::Event::DevicePackSent {
            family: sent.family.diagnostic_tag(),
            firmware_cohort: sent.firmware_cohort.diagnostic_tag(),
            image_count: sent.image_count as u32,
            audio_count: sent.audio_count as u32,
            elapsed_ms,
        },
        Err(err) => device_log::Event::DevicePackSendFailed {
            source: send_failure_source(err),
            elapsed_ms,
        },
    };
    let _ = device_log::record_event(&app, event);

    // Best-effort: remember the sent pack's title locally (keyed by its pack
    // UUID) so the device list recognizes it immediately — a custom pack is
    // in no official catalog and would otherwise render "Histoire non
    // reconnue" right after its own send. A title failure never reclassifies
    // a committed send.
    if let Ok(sent) = &outcome {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let title: Result<String, _> = db.conn().query_row(
            "SELECT title FROM stories WHERE id = ?1",
            rusqlite::params![&input.story_id],
            |row| row.get(0),
        );
        if let Ok(title) = title {
            let _ = set_user_title(&mut db, &sent.pack_uuid, &title);
        }
    }

    outcome.map(SendPackToDeviceOutcomeDto::from_outcome)
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

/// Strict boundary validation of the delete input — same discipline as the
/// import: both identifiers normally come from Rust's own DTOs, so a malformed
/// value is a frontend bug, refused explicitly before any device touch.
fn validate_delete_input(input: &DeleteDeviceStoryInputDto) -> Result<(), AppError> {
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_delete_input("invalid_device_identifier"));
    }
    if !is_canonical_pack_uuid(&input.pack_uuid) {
        return Err(invalid_delete_input("invalid_pack_uuid"));
    }
    Ok(())
}

fn invalid_delete_input(cause: &'static str) -> AppError {
    AppError::device_delete_failed(
        "Suppression impossible: requête invalide.",
        "Relance la lecture de la bibliothèque de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "kind": "invalid_input",
        "cause": cause,
    }))
}

fn delete_join_error() -> AppError {
    AppError::device_delete_failed(
        "Suppression impossible: tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "spawn_blocking_join",
    }))
}

/// Map a delete AppError to the closed diagnostic `source` set for the event.
fn delete_failure_source(err: &AppError) -> &'static str {
    err.details
        .as_ref()
        .and_then(|d| d.get("source").and_then(|s| s.as_str()))
        .map(|s| match s {
            "device_changed" => "device_changed",
            "capability_gate" => "capability_gate",
            "delete_rejected" => "delete_rejected",
            "spawn_blocking_join" => "spawn_blocking_join",
            _ => "other",
        })
        .unwrap_or("other")
}

/// Strict boundary validation of the reorder input: the identifier shape,
/// a bounded, non-empty list of canonical pack uuids with no repeat.
fn validate_reorder_input(input: &ReorderDeviceStoriesInputDto) -> Result<(), AppError> {
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_reorder_input("invalid_device_identifier"));
    }
    if input.ordered_pack_uuids.is_empty() || input.ordered_pack_uuids.len() > MAX_REORDER_PACKS {
        return Err(invalid_reorder_input("invalid_count"));
    }
    let mut seen = std::collections::HashSet::new();
    for uuid in &input.ordered_pack_uuids {
        if !is_canonical_pack_uuid(uuid) {
            return Err(invalid_reorder_input("invalid_pack_uuid"));
        }
        if !seen.insert(uuid.as_str()) {
            return Err(invalid_reorder_input("repeated_pack_uuid"));
        }
    }
    Ok(())
}

fn invalid_reorder_input(cause: &'static str) -> AppError {
    AppError::device_write_failed(
        "Réorganisation impossible: requête invalide.",
        "Relance la lecture de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "kind": "invalid_input",
        "cause": cause,
    }))
}

fn reorder_join_error() -> AppError {
    AppError::device_write_failed(
        "Réorganisation impossible: tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({ "source": "spawn_blocking_join" }))
}

/// Map a reorder AppError to the closed diagnostic `source` set.
fn reorder_failure_source(err: &AppError) -> &'static str {
    err.details
        .as_ref()
        .and_then(|d| d.get("source").and_then(|s| s.as_str()))
        .map(|s| match s {
            "device_changed" => "device_changed",
            "capability_gate" => "capability_gate",
            "reorder_diverged" => "reorder_diverged",
            "reorder_rejected" => "reorder_rejected",
            "spawn_blocking_join" => "spawn_blocking_join",
            _ => "other",
        })
        .unwrap_or("other")
}

/// Strict boundary validation refusal of the send input — the identifier
/// normally comes from Rust's own detection DTO, so a malformed value is a
/// frontend bug, refused explicitly before the dialog even opens.
fn invalid_send_input(cause: &'static str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: requête invalide.",
        "Relance la détection de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "kind": "invalid_input",
        "cause": cause,
    }))
}

/// `app_data_dir` could not be resolved — the retained-archive store has no
/// home to read from.
fn send_app_data_unavailable_error() -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: stockage local introuvable.",
        "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
    )
    .with_details(serde_json::json!({ "source": "other", "cause": "app_data_dir" }))
}

/// The blocking send worker could not be joined (panicked or cancelled).
fn send_join_error() -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({
        "source": "spawn_blocking_join",
    }))
}

/// Map a send AppError to the closed diagnostic `source` set for the event.
fn send_failure_source(err: &AppError) -> &'static str {
    err.details
        .as_ref()
        .and_then(|d| d.get("source").and_then(|s| s.as_str()))
        .map(|s| match s {
            "device_changed" => "device_changed",
            "capability_gate" => "capability_gate",
            "archive" => "archive",
            "asset_convert" => "asset_convert",
            "device_write" => "device_write",
            "media_store" => "media_store",
            "story_pack" => "story_pack",
            "spawn_blocking_join" => "spawn_blocking_join",
            _ => "other",
        })
        .unwrap_or("other")
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

    /// Same discipline for the pack-send command-layer refusals: a non-empty
    /// cause AND a non-empty next gesture, all under the send flow's
    /// `DEVICE_WRITE_FAILED` category.
    #[test]
    fn command_layer_send_refusals_are_actionable() {
        let refusals = [
            invalid_send_input("invalid_device_identifier"),
            invalid_send_input("invalid_story_id"),
            send_app_data_unavailable_error(),
            send_join_error(),
            invalid_reorder_input("invalid_count"),
            reorder_join_error(),
        ];
        for err in &refusals {
            assert_eq!(
                err.code,
                crate::domain::shared::AppErrorCode::DeviceWriteFailed,
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
