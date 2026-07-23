//! Device-story delete application service.
//!
//! Owns "Supprimer de l'appareil":
//!
//! 1. authoritative re-scan at engagement time (identity + capability gate —
//!    the inspection snapshot is never trusted),
//! 2. delist the pack from `.pi` then remove its content folder, via the
//!    [`DevicePackDeleter`] (INDEX FIRST, CONTENT SECOND).
//!
//! The LOCAL library is never touched — deleting a story from the DEVICE is
//! independent of any local copy. The device mount is written ONLY after the
//! capability gate proves the profile may delete (fail-closed, NFR17/NFR18).
//! Synchronous by design: the command layer hands it to `spawn_blocking` whole.

use std::time::{Duration, Instant};

use crate::domain::device::{DeviceFamily, FirmwareCohort, SupportedOperation};
use crate::domain::shared::AppError;
use crate::domain::transfer::TransferFailureCause;
use crate::infrastructure::device::{DeleteOutcome, DevicePackDeleter, DeviceScanner};

use super::{check_operation_allowed, resolve_connected_lunii, ConnectedLuniiOutcome};

/// Input of [`delete_device_story`]. Both identifiers are validated at the IPC
/// boundary (32-hex device id, canonical lowercase pack UUID).
#[derive(Debug, Clone)]
pub struct DeleteDeviceStoryRequest {
    pub device_identifier: String,
    pub pack_uuid: String,
}

/// Result of a settled delete, echoed to the UI. `was_present` is `false` when
/// the pack was already absent (a re-issued delete, or a stale selection) — an
/// idempotent no-op success, not an error. `family` / `firmware_cohort` come
/// from the re-scanned profile and feed the diagnostic event only (they never
/// cross the wire — the outcome DTO stays family-neutral).
#[derive(Debug, Clone)]
pub struct DeletedDeviceStory {
    pub pack_uuid: String,
    pub was_present: bool,
    pub family: DeviceFamily,
    pub firmware_cohort: FirmwareCohort,
}

/// Run the full delete sequence. See the module doc for the ordering contract.
pub fn delete_device_story(
    scanner: &dyn DeviceScanner,
    deleter: &dyn DevicePackDeleter,
    request: &DeleteDeviceStoryRequest,
    budget: Duration,
) -> Result<DeletedDeviceStory, AppError> {
    let started = Instant::now();
    let remaining = |started: Instant| budget.saturating_sub(started.elapsed());

    // 1. Authoritative re-scan: identity + capability re-proven against the live
    //    device, never the inspection snapshot the user clicked.
    let resolved = resolve_connected_lunii(scanner, remaining(started))?;
    let (profile, mount_path) = match resolved.outcome {
        ConnectedLuniiOutcome::Supported(profile) => {
            if profile.device_identifier != request.device_identifier {
                return Err(device_changed_error("identifier_mismatch"));
            }
            let mount = resolved
                .supported_mount_path
                .ok_or_else(|| device_changed_error("mount_unavailable"))?;
            (profile, mount)
        }
        ConnectedLuniiOutcome::None => return Err(device_changed_error("device_absent")),
        ConnectedLuniiOutcome::Unsupported { .. } => {
            return Err(device_changed_error("device_unsupported"))
        }
        ConnectedLuniiOutcome::Ambiguous { .. } => {
            return Err(device_changed_error("multiple_candidates"))
        }
    };

    // 2. Fail-closed gate BEFORE any device mutation. A profile that may not
    //    delete (e.g. FLAM) refuses here with DEVICE_UNSUPPORTED / capability_gate.
    check_operation_allowed(&profile, SupportedOperation::DeleteStory)?;

    // 3. Delist + remove content (INDEX FIRST). Idempotent: an unlisted UUID is
    //    a no-op success (`was_present = false`).
    match deleter.delete_pack(&mount_path, &request.pack_uuid) {
        Ok(outcome) => Ok(DeletedDeviceStory {
            pack_uuid: request.pack_uuid.clone(),
            was_present: matches!(outcome, DeleteOutcome::Deleted),
            family: profile.family,
            firmware_cohort: profile.firmware_cohort,
        }),
        Err(failure) => Err(delete_rejected_error(failure.cause)),
    }
}

fn device_changed_error(cause: &'static str) -> AppError {
    AppError::device_delete_failed(
        "Suppression impossible: l'appareil connecté a changé.",
        "Rebranche l'appareil souhaité puis réessaie la suppression.",
    )
    .with_details(serde_json::json!({
        "source": "device_changed",
        "cause": cause,
    }))
}

fn delete_rejected_error(cause: TransferFailureCause) -> AppError {
    // Coarse, PII-free cause tag (no path, no raw OS message).
    let cause_tag = match cause {
        TransferFailureCause::WriteRejected => "write_rejected",
        TransferFailureCause::Interrupted => "interrupted",
        _ => "other",
    };
    AppError::device_delete_failed(
        "Suppression impossible: l'appareil a refusé l'écriture.",
        "Vérifie que l'appareil est bien connecté puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "delete_rejected",
        "cause": cause_tag,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::{
        compute_device_identifier, DeleteOutcome, MockDevicePackDeleter, MockDeviceScanner,
    };

    /// Metadata version 7 classifies as Lunii V3 — the cohort whose matrix line
    /// opens `delete_story` while `import_story` / `write_story` stay closed.
    const V3_METADATA_VERSION: u8 = 7;
    const PACK_UUID: &str = "abababab-abab-abab-abab-ababfac5562d";

    fn budget() -> Duration {
        Duration::from_millis(500)
    }

    /// The identifier `enqueue_supported_lunii` synthesizes (`.pi` = MOCK_PI,
    /// serial = MOCK_SERIAL) — the value a matching request must carry.
    fn mock_identifier() -> String {
        compute_device_identifier(b"MOCK_PI", Some("MOCK_SERIAL"))
    }

    fn request() -> DeleteDeviceStoryRequest {
        DeleteDeviceStoryRequest {
            device_identifier: mock_identifier(),
            pack_uuid: PACK_UUID.to_string(),
        }
    }

    #[test]
    fn deletes_on_a_v3_device_which_may_delete_even_without_write() {
        // A V3 supported device: import/write closed, delete OPEN.
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(V3_METADATA_VERSION);
        let deleter = MockDevicePackDeleter::new();
        deleter.enqueue_success(DeleteOutcome::Deleted);

        let out = delete_device_story(&scanner, &deleter, &request(), budget()).expect("delete ok");
        assert!(out.was_present);
        assert_eq!(deleter.deleted_uuids(), vec![PACK_UUID.to_string()]);
    }

    #[test]
    fn a_re_issued_delete_of_an_absent_pack_is_an_idempotent_success() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(V3_METADATA_VERSION);
        let deleter = MockDevicePackDeleter::new();
        deleter.enqueue_success(DeleteOutcome::NotPresent);

        let out = delete_device_story(&scanner, &deleter, &request(), budget()).expect("delete ok");
        assert!(!out.was_present, "an absent pack is a no-op, not an error");
    }

    #[test]
    fn refuses_when_the_device_identifier_no_longer_matches() {
        // A DIFFERENT supported Lunii is now mounted (swapped `.pi`/serial) — its
        // identifier will not match the one the request pinned.
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii_swapped(V3_METADATA_VERSION);
        let deleter = MockDevicePackDeleter::new();

        let err = delete_device_story(&scanner, &deleter, &request(), budget())
            .expect_err("identity mismatch refuses");
        assert_eq!(
            serde_json::to_value(&err).unwrap()["details"]["cause"],
            "identifier_mismatch"
        );
        // The deleter is NEVER reached on a changed device.
        assert_eq!(deleter.call_count(), 0);
    }

    #[test]
    fn refuses_when_the_device_is_absent_without_touching_the_deleter() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let deleter = MockDevicePackDeleter::new();

        let err = delete_device_story(&scanner, &deleter, &request(), budget())
            .expect_err("absent device refuses");
        assert_eq!(
            serde_json::to_value(&err).unwrap()["code"],
            "DEVICE_DELETE_FAILED"
        );
        assert_eq!(deleter.call_count(), 0);
    }
}
