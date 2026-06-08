use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::domain::shared::AppError;

use super::scanner::{DeviceCandidate, DeviceScanReport, DeviceScanner};

/// Programmable mock for tests. Each `scan()` call pops the next queued
/// outcome (FIFO). When the queue is empty, returns an empty report — a
/// missing enqueue is treated as "no device", not a panic, so a test
/// that forgets to prime the mock fails on assertion clarity rather
/// than panic noise.
#[derive(Clone, Default)]
pub struct MockDeviceScanner {
    queue: Arc<Mutex<Vec<Result<DeviceScanReport, AppError>>>>,
}

impl MockDeviceScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&self, outcome: Result<DeviceScanReport, AppError>) {
        let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        g.push(outcome);
    }

    pub fn enqueue_no_device(&self) {
        self.enqueue(Ok(DeviceScanReport::empty(Duration::from_millis(1))));
    }

    pub fn enqueue_supported_lunii(&self, metadata_version: u8) {
        let report = DeviceScanReport {
            candidates: vec![DeviceCandidate {
                mount_path: std::path::PathBuf::from("/mock/lunii"),
                metadata_payload: vec![metadata_version],
                pi_payload: b"MOCK_PI".to_vec(),
                has_bt: true,
                volume_serial: Some("MOCK_SERIAL".into()),
            }],
            elapsed: Duration::from_millis(2),
            truncated_due_to_timeout: false,
        };
        self.enqueue(Ok(report));
    }

    pub fn enqueue_unsupported_metadata(&self, metadata_version: u8) {
        // Same shape as `enqueue_supported_lunii`; the application layer
        // is responsible for classifying based on version.
        self.enqueue_supported_lunii(metadata_version);
    }

    pub fn enqueue_corrupt_missing_pi(&self) {
        // A candidate without `.pi`. The system scanner would normally
        // filter this at the FS stage, but the mock allows surfacing it
        // to exercise downstream defenses if needed.
        let report = DeviceScanReport {
            candidates: vec![DeviceCandidate {
                mount_path: std::path::PathBuf::from("/mock/lunii_corrupt"),
                metadata_payload: vec![3],
                pi_payload: Vec::new(),
                has_bt: true,
                volume_serial: None,
            }],
            elapsed: Duration::from_millis(1),
            truncated_due_to_timeout: false,
        };
        self.enqueue(Ok(report));
    }

    pub fn enqueue_corrupt_missing_bt(&self) {
        let report = DeviceScanReport {
            candidates: vec![DeviceCandidate {
                mount_path: std::path::PathBuf::from("/mock/lunii_no_bt"),
                metadata_payload: vec![3],
                pi_payload: b"MOCK_PI".to_vec(),
                has_bt: false,
                volume_serial: None,
            }],
            elapsed: Duration::from_millis(1),
            truncated_due_to_timeout: false,
        };
        self.enqueue(Ok(report));
    }

    pub fn enqueue_multiple_candidates(&self) {
        let report = DeviceScanReport {
            candidates: vec![
                DeviceCandidate {
                    mount_path: std::path::PathBuf::from("/mock/lunii_a"),
                    metadata_payload: vec![3],
                    pi_payload: b"PI_A".to_vec(),
                    has_bt: true,
                    volume_serial: Some("SERIAL_A".into()),
                },
                DeviceCandidate {
                    mount_path: std::path::PathBuf::from("/mock/lunii_b"),
                    metadata_payload: vec![6],
                    pi_payload: b"PI_B".to_vec(),
                    has_bt: true,
                    volume_serial: Some("SERIAL_B".into()),
                },
            ],
            elapsed: Duration::from_millis(3),
            truncated_due_to_timeout: false,
        };
        self.enqueue(Ok(report));
    }

    pub fn enqueue_timeout_truncated(&self) {
        self.enqueue(Ok(DeviceScanReport {
            candidates: Vec::new(),
            elapsed: Duration::from_secs(4),
            truncated_due_to_timeout: true,
        }));
    }

    pub fn enqueue_permission_denied(&self) {
        self.enqueue(Err(AppError::device_scan_failed(
            "Détection indisponible: vérifie les permissions et réessaie.",
            "Vérifie les permissions du dossier puis relance la détection.",
        )
        .with_details(serde_json::json!({
            "source": "fs_read",
            "kind": "permission_denied",
        }))));
    }
}

impl DeviceScanner for MockDeviceScanner {
    fn scan(&self, _budget: Duration) -> Result<DeviceScanReport, AppError> {
        let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_empty() {
            Ok(DeviceScanReport::empty(Duration::from_millis(1)))
        } else {
            g.remove(0)
        }
    }
}
