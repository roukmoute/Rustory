use std::collections::HashSet;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, State};

use crate::application::device::import::ImportDeviceStoryRequest;
use crate::application::device::library::DeviceLibraryOutcome;
use crate::application::device::{self, ConnectedLuniiOutcome};
use crate::domain::shared::AppError;
use crate::infrastructure::device::{MountAttempt, MountOutcome};
use crate::infrastructure::diagnostics::device_log;
use crate::ipc::dto::{
    ConnectedDeviceDto, DeviceLibraryDto, ImportDeviceStoryInputDto, ImportDeviceStoryOutcomeDto,
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

    // Local provenance truth, read under a SCOPED lock BEFORE the device
    // I/O and released immediately — the device read must never hold the
    // DB mutex. Rust composes local truth + device truth right here; the
    // frontend never recomposes `alreadyImported`.
    let imported_uuids = read_imported_pack_uuids(&state)?;

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

    outcome.map(|o| DeviceLibraryDto::from_outcome(o, &imported_uuids))
}

/// Read the set of already-imported pack UUIDs (`story_imports`) under a
/// scoped DB lock. Fail-closed: a provenance read failure surfaces as a
/// recoverable error rather than silently stamping `alreadyImported:
/// false` — lying about local truth would invite a duplicate copy flow.
fn read_imported_pack_uuids(state: &State<'_, AppState>) -> Result<HashSet<String>, AppError> {
    let provenance_unavailable = |stage: &'static str| {
        AppError::local_storage_unavailable(
            "Lecture de la bibliothèque appareil indisponible: vérifie le disque local et réessaie.",
            "Réessaie la lecture ; si le problème persiste, consulte les traces locales.",
        )
        .with_details(serde_json::json!({
            "source": "story_imports_read",
            "stage": stage,
        }))
    };
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut stmt = db
        .conn()
        .prepare("SELECT pack_uuid FROM story_imports")
        .map_err(|_| provenance_unavailable("prepare"))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| provenance_unavailable("query"))?;
    rows.collect::<Result<HashSet<_>, _>>()
        .map_err(|_| provenance_unavailable("collect"))
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
    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        AppError::import_failed(
            "Copie impossible: stockage local introuvable.",
            "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
        )
        .with_details(serde_json::json!({
            "source": "other",
            "cause": "app_data_dir",
        }))
    })?;
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
    .map_err(|_| {
        AppError::import_failed(
            "Copie impossible: tâche interrompue.",
            "Réessaie la copie ; si le problème persiste, redémarre Rustory.",
        )
        .with_details(serde_json::json!({
            "source": "spawn_blocking_join",
        }))
    })?;

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

/// Strict boundary validation of the import input. Both values normally
/// originate from Rust itself (detection + inventory DTOs), so a
/// malformed value is a frontend bug — refused explicitly, never
/// "best-effort matched" against the device.
fn validate_import_input(input: &ImportDeviceStoryInputDto) -> Result<(), AppError> {
    if !is_32_lowercase_hex(&input.device_identifier) {
        return Err(invalid_import_input("invalid_device_identifier"));
    }
    if !is_canonical_lowercase_uuid(&input.pack_uuid) {
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

fn is_32_lowercase_hex(value: &str) -> bool {
    value.len() == 32
        && value
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// Canonical lowercase hyphenated UUID (8-4-4-4-12), the exact shape
/// `format_pack_uuid` emits.
fn is_canonical_lowercase_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *b != b'-' {
                    return false;
                }
            }
            _ => {
                if !(b.is_ascii_digit() || (b'a'..=b'f').contains(b)) {
                    return false;
                }
            }
        }
    }
    true
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
