//! Device-library read application service.
//!
//! Reads the installed-pack inventory of an already-detected supported
//! Lunii. The flow is an authoritative re-read: it re-scans the live
//! device (so a unplug between detection and this call surfaces as a
//! recoverable failure), confirms the live `device_identifier` still
//! matches the one the UI asked about, passes the fail-closed capability
//! gate (`ReadLibrary`), then reads the inventory at the supported
//! candidate's mount path.
//!
//! Stays Tauri-free: tests inject a [`MockDeviceScanner`] and a
//! [`MockDeviceLibraryReader`] and exercise the full pipeline without a
//! runtime or a real mount.

use std::time::{Duration, Instant};

use crate::domain::device::{DeviceLibrary, SupportedOperation, UnsupportedReason};
use crate::domain::shared::AppError;
use crate::infrastructure::device::{DeviceLibraryReader, DeviceScanner};

use super::{check_operation_allowed, resolve_connected_lunii, ConnectedLuniiOutcome};

/// Result of [`read_device_library`]. Mapped 1-to-1 by the IPC layer to
/// `DeviceLibraryDto`. Recoverable failures (device unplugged mid-read,
/// FS error, identity changed) propagate as `Err(AppError)` rather than
/// a variant here — they are transport failures, not library states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceLibraryOutcome {
    /// No supported device is connected anymore.
    None,
    /// A device is present but its profile is not in the allow-list, or
    /// more than one supported device is connected — any families —
    /// (cannot bind the read).
    Unsupported {
        reason: UnsupportedReason,
        firmware_hint: Option<String>,
    },
    /// The inventory was read. `entries` may be empty (a valid empty
    /// Lunii) — that is NOT an error.
    Readable {
        device_identifier: String,
        library: DeviceLibrary,
    },
}

/// Read the device-side library for the supported Lunii whose identifier
/// equals `requested_identifier`.
pub fn read_device_library(
    scanner: &dyn DeviceScanner,
    reader: &dyn DeviceLibraryReader,
    requested_identifier: &str,
    budget: Duration,
) -> Result<DeviceLibraryOutcome, AppError> {
    let started = Instant::now();
    let resolved = resolve_connected_lunii(scanner, budget)?;

    match resolved.outcome {
        ConnectedLuniiOutcome::None => Ok(DeviceLibraryOutcome::None),
        ConnectedLuniiOutcome::Unsupported {
            reason,
            firmware_hint,
        } => Ok(DeviceLibraryOutcome::Unsupported {
            reason,
            firmware_hint,
        }),
        ConnectedLuniiOutcome::Ambiguous { candidate_count } => {
            // More than one supported device (any families — two Lunii,
            // or a Lunii + a recognized FLAM): we cannot bind the read
            // to the requested device unambiguously. Surface the
            // detection's `MultipleCandidates` reason rather than
            // guessing.
            Ok(DeviceLibraryOutcome::Unsupported {
                reason: UnsupportedReason::MultipleCandidates,
                firmware_hint: Some(format!("count_{candidate_count}")),
            })
        }
        ConnectedLuniiOutcome::Supported(profile) => {
            // Authoritative re-read guard: the live device must be the
            // one the UI asked about. A mismatch means the device was
            // swapped or unplugged-and-replaced between detection and
            // this call — recoverable, never a silent read of the wrong
            // device's library.
            if profile.device_identifier != requested_identifier {
                return Err(device_changed_error());
            }
            // Fail-closed gate BEFORE any read (NFR17 + NFR18). Read is
            // allowed for every supported cohort, but the gate must still
            // be consulted so the policy stays enforced in one place.
            check_operation_allowed(&profile, SupportedOperation::ReadLibrary)?;

            let mount_path = resolved
                .supported_mount_path
                .ok_or_else(mount_unavailable_error)?;
            // Charge the remaining budget to the read so the total stays
            // bounded even after a slow scan.
            let remaining = budget.saturating_sub(started.elapsed());
            let library = reader.read_library(&mount_path, remaining)?;

            Ok(DeviceLibraryOutcome::Readable {
                device_identifier: profile.device_identifier,
                library,
            })
        }
    }
}

fn device_changed_error() -> AppError {
    AppError::device_scan_failed(
        "Lecture de la bibliothèque appareil indisponible: l'appareil connecté a changé.",
        "Rebranche la Lunii souhaitée puis réessaie la lecture de la bibliothèque.",
    )
    .with_details(serde_json::json!({
        "source": "device_changed",
    }))
}

fn mount_unavailable_error() -> AppError {
    // Defensive: a `Supported` outcome always carries a mount path in the
    // current pipeline. If that invariant ever breaks, fail recoverably
    // rather than panic.
    AppError::device_scan_failed(
        "Lecture de la bibliothèque appareil indisponible: point de montage introuvable.",
        "Rebranche la Lunii puis réessaie la lecture de la bibliothèque.",
    )
    .with_details(serde_json::json!({
        "source": "mount_unavailable",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::{
        compute_device_identifier, MockDeviceLibraryReader, MockDeviceScanner,
    };

    fn budget() -> Duration {
        Duration::from_secs(5)
    }

    /// The identifier the mock scanner's `enqueue_supported_lunii` volume
    /// hashes to (`.pi` = `MOCK_PI`, serial = `MOCK_SERIAL`).
    fn mock_identifier() -> String {
        compute_device_identifier(b"MOCK_PI", Some("MOCK_SERIAL"))
    }

    #[test]
    fn returns_readable_for_supported_device_with_matching_identifier() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(2);

        let outcome =
            read_device_library(&scanner, &reader, &mock_identifier(), budget()).expect("read");
        match outcome {
            DeviceLibraryOutcome::Readable {
                device_identifier,
                library,
            } => {
                assert_eq!(device_identifier, mock_identifier());
                assert_eq!(library.entries.len(), 2);
            }
            other => panic!("expected Readable, got {other:?}"),
        }
    }

    #[test]
    fn returns_readable_empty_library_when_device_has_no_packs() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(7);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_empty_library();

        let outcome =
            read_device_library(&scanner, &reader, &mock_identifier(), budget()).expect("read");
        match outcome {
            DeviceLibraryOutcome::Readable { library, .. } => assert!(library.entries.is_empty()),
            other => panic!("expected Readable(empty), got {other:?}"),
        }
    }

    #[test]
    fn returns_none_when_no_device_connected() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let reader = MockDeviceLibraryReader::new();

        let outcome =
            read_device_library(&scanner, &reader, &mock_identifier(), budget()).expect("read");
        assert_eq!(outcome, DeviceLibraryOutcome::None);
    }

    #[test]
    fn returns_unsupported_when_metadata_unsupported() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_unsupported_metadata(99);
        let reader = MockDeviceLibraryReader::new();

        let outcome = read_device_library(&scanner, &reader, "whatever", budget()).expect("read");
        match outcome {
            DeviceLibraryOutcome::Unsupported { reason, .. } => {
                assert_eq!(reason, UnsupportedReason::MetadataUnsupported);
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn returns_unsupported_multiple_candidates_when_two_supported() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_multiple_candidates();
        let reader = MockDeviceLibraryReader::new();

        let outcome = read_device_library(&scanner, &reader, "whatever", budget()).expect("read");
        match outcome {
            DeviceLibraryOutcome::Unsupported { reason, .. } => {
                assert_eq!(reason, UnsupportedReason::MultipleCandidates);
            }
            other => panic!("expected Unsupported(MultipleCandidates), got {other:?}"),
        }
    }

    #[test]
    fn rejects_identifier_mismatch_as_recoverable_device_changed() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1);

        // The UI asked for a different device than the one now present.
        let err = read_device_library(
            &scanner,
            &reader,
            "deadbeefdeadbeefdeadbeefdeadbeef",
            budget(),
        )
        .expect_err("identity mismatch must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "device_changed");
    }

    #[test]
    fn propagates_reader_failure_when_device_disconnects_mid_read() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_disconnected_mid_read();

        let err = read_device_library(&scanner, &reader, &mock_identifier(), budget())
            .expect_err("mid-read disconnect must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "fs_read");
        assert_eq!(v["details"]["kind"], "not_found");
    }

    #[test]
    fn propagates_scan_timeout_before_attempting_a_read() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_timeout_truncated();
        let reader = MockDeviceLibraryReader::new();

        let err = read_device_library(&scanner, &reader, &mock_identifier(), budget())
            .expect_err("scan timeout must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "scan_timeout");
    }
}
