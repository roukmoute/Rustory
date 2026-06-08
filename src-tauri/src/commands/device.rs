use std::time::{Duration, Instant};

use tauri::{AppHandle, State};

use crate::application::device::{self, ConnectedLuniiOutcome};
use crate::domain::shared::AppError;
use crate::infrastructure::device::{MountAttempt, MountOutcome};
use crate::infrastructure::diagnostics::device_log;
use crate::ipc::dto::ConnectedDeviceDto;
use crate::AppState;

/// Wall-clock budget for the device scan. Sized below the NFR4 budget
/// of 5 s with a safety margin so the IPC marshalling and the front-end
/// timer (≈ 4500 ms) cooperate without flapping.
pub const DEVICE_SCAN_BUDGET: Duration = Duration::from_millis(4000);

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
