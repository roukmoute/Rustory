//! Reorder the stories on a connected device — the wheel order. A DEVICE
//! MUTATION of the index only, with the delete flow's discipline: an
//! authoritative re-scan (identity + capability re-proven live), the
//! fail-closed `reorder_stories` gate, then the strict atomic index rewrite
//! ([`DevicePackReorderer`]). A stale order (the device's list changed
//! since the UI read it) is refused with a re-read hint, never guessed.

use std::time::{Duration, Instant};

use crate::domain::device::{DeviceFamily, FirmwareCohort, SupportedOperation};
use crate::domain::shared::AppError;
use crate::domain::transfer::TransferFailureCause;
use crate::infrastructure::device::{
    DevicePackReorderer, DeviceScanner, ReorderFailure, ReorderOutcome,
};

use super::{check_operation_allowed, resolve_connected_lunii, ConnectedLuniiOutcome};

/// Input of [`reorder_device_stories`]: the device and the COMPLETE new
/// order of its visible packs (every listed uuid, once, canonical).
#[derive(Debug, Clone)]
pub struct ReorderDeviceStoriesRequest {
    pub device_identifier: String,
    pub ordered_pack_uuids: Vec<String>,
}

/// Result of a settled reorder. `changed` is `false` when the device
/// already listed that order. Family/cohort feed the diagnostic event only.
#[derive(Debug, Clone)]
pub struct ReorderedDeviceStories {
    pub count: usize,
    pub changed: bool,
    pub family: DeviceFamily,
    pub firmware_cohort: FirmwareCohort,
}

pub fn reorder_device_stories(
    scanner: &dyn DeviceScanner,
    reorderer: &dyn DevicePackReorderer,
    request: &ReorderDeviceStoriesRequest,
    budget: Duration,
) -> Result<ReorderedDeviceStories, AppError> {
    let started = Instant::now();
    let remaining = budget.saturating_sub(started.elapsed());

    // 1. Authoritative re-scan: identity + capability re-proven live.
    let resolved = resolve_connected_lunii(scanner, remaining)?;
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

    // 2. Fail-closed gate BEFORE any device mutation.
    check_operation_allowed(&profile, SupportedOperation::ReorderStories)?;

    // 3. The strict atomic index rewrite.
    match reorderer.reorder_packs(&mount_path, &request.ordered_pack_uuids) {
        Ok(outcome) => Ok(ReorderedDeviceStories {
            count: request.ordered_pack_uuids.len(),
            changed: matches!(outcome, ReorderOutcome::Reordered),
            family: profile.family,
            firmware_cohort: profile.firmware_cohort,
        }),
        Err(ReorderFailure::Diverged) => Err(diverged_error()),
        Err(ReorderFailure::Rejected(cause)) => Err(rejected_error(cause)),
    }
}

fn device_changed_error(cause: &'static str) -> AppError {
    AppError::device_write_failed(
        "Réorganisation impossible: l'appareil connecté a changé.",
        "Rebranche l'appareil souhaité puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "device_changed", "cause": cause }))
}

/// The device's list changed under the UI (a pack added or removed): the
/// order on screen is stale, nothing was written.
fn diverged_error() -> AppError {
    AppError::device_write_failed(
        "Réorganisation impossible: la liste des histoires de l'appareil a changé entre-temps.",
        "Relance la lecture de l'appareil, puis déplace à nouveau l'histoire.",
    )
    .with_details(serde_json::json!({ "source": "reorder_diverged", "cause": "diverged" }))
}

fn rejected_error(cause: TransferFailureCause) -> AppError {
    let cause_tag = match cause {
        TransferFailureCause::WriteRejected => "write_rejected",
        TransferFailureCause::Interrupted => "interrupted",
        _ => "other",
    };
    AppError::device_write_failed(
        "Réorganisation impossible: l'appareil a refusé l'écriture.",
        "Vérifie que l'appareil est bien connecté puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "reorder_rejected", "cause": cause_tag }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::{
        compute_device_identifier, MockDevicePackReorderer, MockDeviceScanner,
    };

    const V3_METADATA_VERSION: u8 = 7;
    const FLAM_ORDER: [&str; 2] = [
        "abababab-abab-abab-abab-ababfac5562d",
        "cdcdcdcd-cdcd-cdcd-cdcd-cdcdfac5562e",
    ];

    fn mock_identifier() -> String {
        compute_device_identifier(b"MOCK_PI", Some("MOCK_SERIAL"))
    }

    fn request() -> ReorderDeviceStoriesRequest {
        ReorderDeviceStoriesRequest {
            device_identifier: mock_identifier(),
            ordered_pack_uuids: FLAM_ORDER.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn reorders_on_a_v3_after_the_gate_and_reports_the_change() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(V3_METADATA_VERSION);
        let reorderer = MockDevicePackReorderer::new();
        let out =
            reorder_device_stories(&scanner, &reorderer, &request(), Duration::from_millis(300))
                .expect("reorder");
        assert_eq!(out.count, 2);
        assert!(out.changed);
        assert_eq!(reorderer.orders(), vec![request().ordered_pack_uuids]);
    }

    #[test]
    fn a_flam_is_refused_at_the_gate_before_any_write() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_flam();
        let reorderer = MockDevicePackReorderer::new();
        let mut req = request();
        req.device_identifier = "ignored".into();
        let err = reorder_device_stories(&scanner, &reorderer, &req, Duration::from_millis(300))
            .expect_err("refused");
        let v = serde_json::to_value(&err).unwrap();
        // Either the identity check or the gate refuses — never a write.
        assert!(
            v["details"]["source"] == "device_changed"
                || v["details"]["source"] == "capability_gate"
        );
        assert_eq!(reorderer.call_count(), 0);
    }

    #[test]
    fn a_stale_order_and_a_refused_write_are_worded_distinctly() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(V3_METADATA_VERSION);
        scanner.enqueue_supported_lunii(V3_METADATA_VERSION);
        let reorderer = MockDevicePackReorderer::new();
        reorderer.enqueue(Err(ReorderFailure::Diverged));
        reorderer.enqueue(Err(ReorderFailure::Rejected(
            TransferFailureCause::WriteRejected,
        )));
        let stale =
            reorder_device_stories(&scanner, &reorderer, &request(), Duration::from_millis(300))
                .expect_err("stale");
        assert_eq!(
            serde_json::to_value(&stale).unwrap()["details"]["source"],
            "reorder_diverged"
        );
        assert!(stale
            .user_action
            .as_deref()
            .unwrap_or("")
            .contains("Relance la lecture"));
        let rejected =
            reorder_device_stories(&scanner, &reorderer, &request(), Duration::from_millis(300))
                .expect_err("rejected");
        assert_eq!(
            serde_json::to_value(&rejected).unwrap()["details"]["source"],
            "reorder_rejected"
        );
    }

    #[test]
    fn an_absent_device_refuses_before_any_write() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let reorderer = MockDevicePackReorderer::new();
        let err =
            reorder_device_stories(&scanner, &reorderer, &request(), Duration::from_millis(300))
                .expect_err("absent");
        assert_eq!(
            serde_json::to_value(&err).unwrap()["details"]["cause"],
            "device_absent"
        );
        assert_eq!(reorderer.call_count(), 0);
    }
}
