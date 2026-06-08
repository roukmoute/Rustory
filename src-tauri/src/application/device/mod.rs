//! Device application service.
//!
//! Orchestrates the scan → metadata parse → device-id hash → profile
//! classification pipeline and exposes the [`ConnectedLuniiOutcome`]
//! that the IPC layer maps to a wire DTO.
//!
//! Stays Tauri-free: tests inject a [`MockDeviceScanner`] and exercise
//! the full pipeline without any runtime dependency.

pub mod library;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::domain::device::{
    classify_lunii, DeviceFamily, DeviceProfile, DeviceProfileClassification, SupportedOperation,
    UnsupportedReason,
};
use crate::domain::shared::AppError;
use crate::infrastructure::device::{
    compute_device_identifier, parse_metadata_version, try_automount_lunii_candidates,
    DeviceScanner, MetadataParseError, MountAttempt,
};

/// Result of `read_connected_lunii`. Mapped 1-to-1 by the IPC layer to
/// [`ConnectedDeviceDto`](crate::ipc::dto::device::ConnectedDeviceDto).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectedLuniiOutcome {
    None,
    Supported(DeviceProfile),
    Unsupported {
        reason: UnsupportedReason,
        firmware_hint: Option<String>,
    },
    Ambiguous {
        candidate_count: u32,
    },
}

/// Run the full scan + classification + aggregation pipeline.
///
/// On Linux, the function first asks udisks2 (via D-Bus) to mount any
/// plugged Lunii whose volume the desktop session did not auto-mount,
/// then runs the regular filesystem scan. The auto-mount attempts are
/// returned to the caller via [`read_connected_lunii_with_attempts`]
/// for diagnostic logging; this entry point discards them to preserve
/// the existing call sites.
pub fn read_connected_lunii(
    scanner: &dyn DeviceScanner,
    budget: Duration,
) -> Result<ConnectedLuniiOutcome, AppError> {
    let (outcome, _attempts) = read_connected_lunii_with_attempts(scanner, budget)?;
    Ok(outcome)
}

/// Variant of [`read_connected_lunii`] that also returns the
/// per-volume auto-mount attempts performed before the scan. The IPC
/// command layer uses these to feed the device diagnostic log.
pub fn read_connected_lunii_with_attempts(
    scanner: &dyn DeviceScanner,
    budget: Duration,
) -> Result<(ConnectedLuniiOutcome, Vec<MountAttempt>), AppError> {
    let resolved = resolve_connected_lunii(scanner, budget)?;
    Ok((resolved.outcome, resolved.mount_attempts))
}

/// Richer internal result of the scan pipeline. Retains the `mount_path`
/// of the single supported candidate — the device-library read path
/// ([`library::read_device_library`]) needs it to open `.pi` and
/// `.content` at the volume root. The public `read_connected_lunii*`
/// entry points deliberately drop it so the OS mount path never leaks
/// past the application boundary into a wire DTO.
pub(crate) struct ResolvedScan {
    pub(crate) outcome: ConnectedLuniiOutcome,
    /// Present ONLY when `outcome` is `Supported` with exactly one
    /// candidate.
    pub(crate) supported_mount_path: Option<PathBuf>,
    pub(crate) mount_attempts: Vec<MountAttempt>,
}

/// Run the full automount + scan + classify + aggregate pipeline,
/// retaining the supported candidate's mount path. Shared by the
/// detection command (which discards the path) and the library-read
/// command (which needs it).
pub(crate) fn resolve_connected_lunii(
    scanner: &dyn DeviceScanner,
    budget: Duration,
) -> Result<ResolvedScan, AppError> {
    let started = Instant::now();
    let mount_attempts = try_automount_lunii_candidates();
    // The wall-clock budget covers BOTH the automount D-Bus round-trips
    // and the filesystem scan. Charging only the scan would let a
    // misbehaving udisks2 hang the command past NFR4 even when the scan
    // itself was instant. Deduct the automount elapsed time from the
    // remaining budget; a zero remainder still lets the scanner produce
    // a truncated empty report (no candidates, no panic) which the
    // timeout branch below maps to `DeviceScanFailed`.
    let scan_budget = budget.saturating_sub(started.elapsed());
    let report = scanner.scan(scan_budget)?;

    if report.truncated_due_to_timeout {
        // A truncated scan means we did not probe every mount root —
        // returning a `Supported` outcome here would hide a second
        // Lunii (Ambiguous) that lives behind a slow mount, and
        // returning `None` would falsely advertise "no device" when
        // the truth is "we ran out of budget". Surface a typed
        // `DEVICE_SCAN_FAILED` instead; the silent polling will rerun
        // the scan in 3 s and re-converge on a definitive outcome
        // once the slow mount answers.
        return Err(scan_timeout_error(started.elapsed()));
    }

    if report.candidates.is_empty() {
        return Ok(ResolvedScan {
            outcome: ConnectedLuniiOutcome::None,
            supported_mount_path: None,
            mount_attempts,
        });
    }

    let mut supported: Vec<(DeviceProfile, PathBuf)> = Vec::new();
    let mut unsupported: Vec<(UnsupportedReason, Option<String>)> = Vec::new();

    for candidate in report.candidates {
        // A candidate must carry a non-empty `.pi` payload: hashing an
        // empty payload would still produce a valid 32-hex identifier,
        // collapsing every plug of an empty-`.pi` volume to the same
        // device_identifier and exposing a path to silently accept a
        // corrupted Lunii. Treat empty `.pi` as `MetadataCorrupt`
        // upstream of the version parse so the user receives the
        // "marqueurs appareil incomplets" copy.
        if candidate.pi_payload.is_empty() {
            unsupported.push((UnsupportedReason::MetadataCorrupt, None));
            continue;
        }
        let metadata_version = match parse_metadata_version(&candidate.metadata_payload) {
            Ok(v) => v,
            Err(MetadataParseError::Empty) | Err(MetadataParseError::OutOfRange(_)) => {
                unsupported.push((UnsupportedReason::MetadataCorrupt, None));
                continue;
            }
        };
        let identifier =
            compute_device_identifier(&candidate.pi_payload, candidate.volume_serial.as_deref());
        match classify_lunii(metadata_version, true, candidate.has_bt, &identifier) {
            DeviceProfileClassification::Supported(profile) => {
                supported.push((profile, candidate.mount_path))
            }
            DeviceProfileClassification::Unsupported {
                reason,
                firmware_hint,
                ..
            } => unsupported.push((reason, firmware_hint)),
        }
    }

    if supported.len() > 1 {
        return Ok(ResolvedScan {
            outcome: ConnectedLuniiOutcome::Ambiguous {
                candidate_count: supported.len() as u32,
            },
            supported_mount_path: None,
            mount_attempts,
        });
    }
    if let Some((profile, mount_path)) = supported.into_iter().next() {
        return Ok(ResolvedScan {
            outcome: ConnectedLuniiOutcome::Supported(profile),
            supported_mount_path: Some(mount_path),
            mount_attempts,
        });
    }
    if let Some((reason, hint)) = pick_priority_unsupported(unsupported) {
        return Ok(ResolvedScan {
            outcome: ConnectedLuniiOutcome::Unsupported {
                reason,
                firmware_hint: hint,
            },
            supported_mount_path: None,
            mount_attempts,
        });
    }
    Ok(ResolvedScan {
        outcome: ConnectedLuniiOutcome::None,
        supported_mount_path: None,
        mount_attempts,
    })
}

/// Among the unsupported candidates, surface the one whose root cause is
/// the most fundamental for the user to act on. Order:
/// `MetadataCorrupt` > `MetadataUnsupported` > `FirmwareUnsupported`
/// > everything else (encountered in this order).
fn pick_priority_unsupported(
    candidates: Vec<(UnsupportedReason, Option<String>)>,
) -> Option<(UnsupportedReason, Option<String>)> {
    let order = |r: &UnsupportedReason| match r {
        UnsupportedReason::MetadataCorrupt => 0,
        UnsupportedReason::MetadataUnsupported => 1,
        UnsupportedReason::FirmwareUnsupported => 2,
        UnsupportedReason::FamilyUnknown => 3,
        UnsupportedReason::OperationNotAuthorized => 4,
        UnsupportedReason::MultipleCandidates => 5,
    };
    candidates.into_iter().min_by_key(|(r, _)| order(r))
}

fn scan_timeout_error(elapsed: Duration) -> AppError {
    AppError::device_scan_failed(
        "Détection indisponible: vérifie que la Lunii est branchée et réessaie.",
        "Réessaie la détection ; si le problème persiste, consulte le profil de support.",
    )
    .with_details(serde_json::json!({
        "source": "scan_timeout",
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

/// Capability gate. MUST be called BEFORE any device write attempt.
/// Returns `Ok(())` only when the profile's `supported_operations.{op}`
/// is `true`. Any other case returns a typed
/// `AppError::device_unsupported(...)` with the operation tag in
/// `details`. NFR17 + NFR18 fail-closed enforcement.
pub fn check_operation_allowed(
    profile: &DeviceProfile,
    operation: SupportedOperation,
) -> Result<(), AppError> {
    if profile.supported_operations.allows(operation) {
        return Ok(());
    }
    Err(AppError::device_unsupported(
        "Opération non autorisée pour ce profil d'appareil.",
        "Consulte le profil de support pour comprendre ce qui est permis.",
    )
    .with_details(serde_json::json!({
        "source": "capability_gate",
        "operation": operation.diagnostic_tag(),
        "family": match profile.family {
            DeviceFamily::Lunii => "lunii",
        },
        "firmware_cohort": profile.firmware_cohort.diagnostic_tag(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::LuniiFirmwareCohort;
    use crate::infrastructure::device::MockDeviceScanner;

    fn budget() -> Duration {
        Duration::from_millis(500)
    }

    #[test]
    fn read_connected_lunii_returns_none_when_scanner_returns_empty() {
        let m = MockDeviceScanner::new();
        m.enqueue_no_device();
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        assert_eq!(outcome, ConnectedLuniiOutcome::None);
    }

    #[test]
    fn read_connected_lunii_returns_supported_origine_when_marker_v3() {
        let m = MockDeviceScanner::new();
        m.enqueue_supported_lunii(3);
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        match outcome {
            ConnectedLuniiOutcome::Supported(p) => {
                assert_eq!(p.firmware_cohort, LuniiFirmwareCohort::OrigineV1);
                assert_eq!(p.metadata_format_version, 3);
                assert_eq!(p.device_identifier.len(), 32);
                assert!(p.supported_operations.read_library);
                assert!(p.supported_operations.import_story);
                assert!(!p.supported_operations.write_story);
            }
            other => panic!("expected Supported, got {other:?}"),
        }
    }

    #[test]
    fn read_connected_lunii_returns_supported_v3_when_marker_v7() {
        let m = MockDeviceScanner::new();
        m.enqueue_supported_lunii(7);
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        match outcome {
            ConnectedLuniiOutcome::Supported(p) => {
                assert_eq!(p.firmware_cohort, LuniiFirmwareCohort::V3);
                assert!(!p.supported_operations.import_story);
                assert!(!p.supported_operations.write_story);
            }
            other => panic!("expected Supported, got {other:?}"),
        }
    }

    #[test]
    fn read_connected_lunii_returns_unsupported_metadata_when_v99() {
        let m = MockDeviceScanner::new();
        m.enqueue_unsupported_metadata(99);
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        match outcome {
            ConnectedLuniiOutcome::Unsupported {
                reason,
                firmware_hint,
            } => {
                assert_eq!(reason, UnsupportedReason::MetadataUnsupported);
                assert_eq!(firmware_hint.as_deref(), Some("metadata_v99"));
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn read_connected_lunii_returns_unsupported_metadata_corrupt_when_pi_payload_empty() {
        let m = MockDeviceScanner::new();
        // Empty `.pi` payload — would otherwise hash to a constant
        // device_identifier shared by every empty-`.pi` volume.
        let report = crate::infrastructure::device::DeviceScanReport {
            candidates: vec![crate::infrastructure::device::DeviceCandidate {
                mount_path: std::path::PathBuf::from("/mock/no-pi"),
                metadata_payload: vec![3],
                pi_payload: Vec::new(),
                has_bt: true,
                volume_serial: None,
            }],
            elapsed: Duration::from_millis(1),
            truncated_due_to_timeout: false,
        };
        m.enqueue(Ok(report));
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        match outcome {
            ConnectedLuniiOutcome::Unsupported { reason, .. } => {
                assert_eq!(reason, UnsupportedReason::MetadataCorrupt);
            }
            other => panic!("expected Unsupported(MetadataCorrupt), got {other:?}"),
        }
    }

    #[test]
    fn read_connected_lunii_returns_unsupported_metadata_corrupt_when_md_empty() {
        let m = MockDeviceScanner::new();
        // Manually enqueue a candidate with an empty .md payload.
        let report = crate::infrastructure::device::DeviceScanReport {
            candidates: vec![crate::infrastructure::device::DeviceCandidate {
                mount_path: std::path::PathBuf::from("/mock/empty"),
                metadata_payload: Vec::new(),
                pi_payload: b"PI".to_vec(),
                has_bt: true,
                volume_serial: None,
            }],
            elapsed: Duration::from_millis(1),
            truncated_due_to_timeout: false,
        };
        m.enqueue(Ok(report));
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        match outcome {
            ConnectedLuniiOutcome::Unsupported { reason, .. } => {
                assert_eq!(reason, UnsupportedReason::MetadataCorrupt);
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn read_connected_lunii_accepts_supported_lunii_even_when_bt_missing() {
        // Real-world Lunii V3 fw 3.3.2 ships without `.bt`. The
        // classifier treats `.bt` as informational only; classification
        // must succeed on `.md` + `.pi` alone.
        let m = MockDeviceScanner::new();
        m.enqueue_corrupt_missing_bt();
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        match outcome {
            ConnectedLuniiOutcome::Supported(p) => {
                assert!(p.supported_operations.read_library);
                assert!(!p.supported_operations.write_story);
            }
            other => panic!("expected Supported, got {other:?}"),
        }
    }

    #[test]
    fn read_connected_lunii_returns_ambiguous_when_two_supported_candidates() {
        let m = MockDeviceScanner::new();
        m.enqueue_multiple_candidates();
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        assert_eq!(
            outcome,
            ConnectedLuniiOutcome::Ambiguous { candidate_count: 2 }
        );
    }

    #[test]
    fn read_connected_lunii_prioritizes_metadata_corrupt_over_metadata_unsupported() {
        let m = MockDeviceScanner::new();
        // Two candidates: one supported v3 + one with both unsupported
        // metadata AND a missing `.pi` (= corrupt). Two `Supported`
        // would surface `Ambiguous`; here only one supports, the other
        // corrupts → the priority logic returns `MetadataCorrupt` only
        // when no `Supported` exists, so we shape it that way.
        let report = crate::infrastructure::device::DeviceScanReport {
            candidates: vec![
                crate::infrastructure::device::DeviceCandidate {
                    mount_path: std::path::PathBuf::from("/a"),
                    metadata_payload: vec![99],
                    pi_payload: b"PI".to_vec(),
                    has_bt: true,
                    volume_serial: None,
                },
                crate::infrastructure::device::DeviceCandidate {
                    mount_path: std::path::PathBuf::from("/b"),
                    metadata_payload: Vec::new(), // empty → parse error → MetadataCorrupt
                    pi_payload: b"PI".to_vec(),
                    has_bt: true,
                    volume_serial: None,
                },
            ],
            elapsed: Duration::from_millis(1),
            truncated_due_to_timeout: false,
        };
        m.enqueue(Ok(report));
        let outcome = read_connected_lunii(&m, budget()).expect("scan");
        match outcome {
            ConnectedLuniiOutcome::Unsupported { reason, .. } => {
                assert_eq!(reason, UnsupportedReason::MetadataCorrupt);
            }
            other => panic!("expected Unsupported(MetadataCorrupt), got {other:?}"),
        }
    }

    #[test]
    fn read_connected_lunii_propagates_scan_timeout_as_device_scan_failed() {
        let m = MockDeviceScanner::new();
        m.enqueue_timeout_truncated();
        let err = read_connected_lunii(&m, budget()).expect_err("expect timeout");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "scan_timeout");
    }

    #[test]
    fn read_connected_lunii_propagates_permission_denied_as_device_scan_failed() {
        let m = MockDeviceScanner::new();
        m.enqueue_permission_denied();
        let err = read_connected_lunii(&m, budget()).expect_err("expect perm");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["kind"], "permission_denied");
    }

    #[test]
    fn read_connected_lunii_does_not_consume_the_scanner_state_across_calls() {
        let m = MockDeviceScanner::new();
        m.enqueue_no_device();
        m.enqueue_supported_lunii(3);
        let first = read_connected_lunii(&m, budget()).expect("scan 1");
        assert_eq!(first, ConnectedLuniiOutcome::None);
        let second = read_connected_lunii(&m, budget()).expect("scan 2");
        assert!(matches!(second, ConnectedLuniiOutcome::Supported(_)));
    }

    fn build_profile(cohort: LuniiFirmwareCohort, version: u8) -> DeviceProfile {
        match crate::domain::device::classify_lunii(version, true, true, "id") {
            crate::domain::device::DeviceProfileClassification::Supported(mut p) => {
                p.firmware_cohort = cohort;
                p
            }
            other => panic!("expected Supported, got {other:?}"),
        }
    }

    #[test]
    fn check_operation_allowed_blocks_write_story_for_every_mvp_profile() {
        for (cohort, version) in [
            (LuniiFirmwareCohort::OrigineV1, 3u8),
            (LuniiFirmwareCohort::MidGenV2, 6),
            (LuniiFirmwareCohort::V3, 7),
        ] {
            let p = build_profile(cohort, version);
            let err = check_operation_allowed(&p, SupportedOperation::WriteStory)
                .expect_err("expect blocked");
            let v = serde_json::to_value(&err).expect("ser");
            assert_eq!(v["code"], "DEVICE_UNSUPPORTED");
            assert_eq!(v["details"]["operation"], "write_story");
        }
    }

    #[test]
    fn check_operation_allowed_authorizes_read_library_for_every_mvp_profile() {
        for (cohort, version) in [
            (LuniiFirmwareCohort::OrigineV1, 3u8),
            (LuniiFirmwareCohort::MidGenV2, 6),
            (LuniiFirmwareCohort::V3, 7),
        ] {
            let p = build_profile(cohort, version);
            check_operation_allowed(&p, SupportedOperation::ReadLibrary)
                .expect("read_library must be allowed");
        }
    }

    #[test]
    fn check_operation_allowed_blocks_import_story_for_v3_profile_only() {
        let v3 = build_profile(LuniiFirmwareCohort::V3, 7);
        check_operation_allowed(&v3, SupportedOperation::ImportStory)
            .expect_err("V3 import must be blocked");
        let origine = build_profile(LuniiFirmwareCohort::OrigineV1, 3);
        check_operation_allowed(&origine, SupportedOperation::ImportStory)
            .expect("OrigineV1 import must be allowed");
        let mid = build_profile(LuniiFirmwareCohort::MidGenV2, 6);
        check_operation_allowed(&mid, SupportedOperation::ImportStory)
            .expect("MidGenV2 import must be allowed");
    }
}
