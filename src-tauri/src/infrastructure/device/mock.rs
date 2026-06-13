use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::domain::device::pack::{PackFile, PackManifest};
use crate::domain::device::{DeviceLibrary, DeviceStoryEntry};
use crate::domain::shared::AppError;

use super::library_reader::DeviceLibraryReader;
use super::pack_reader::{AcquiredPack, DevicePackReader};
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

/// Programmable mock for the device-library read path. Each
/// `read_library()` call pops the next queued outcome (FIFO); an empty
/// queue yields an empty library (no panic), mirroring
/// [`MockDeviceScanner`].
#[derive(Clone, Default)]
pub struct MockDeviceLibraryReader {
    queue: Arc<Mutex<Vec<Result<DeviceLibrary, AppError>>>>,
}

impl MockDeviceLibraryReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&self, outcome: Result<DeviceLibrary, AppError>) {
        let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        g.push(outcome);
    }

    /// Queue a library with `count` synthetic visible packs, all with a
    /// present `.content` folder. Short ids are derived from the index.
    pub fn enqueue_library_with(&self, count: u8) {
        let entries = (0..count)
            .map(|i| DeviceStoryEntry {
                uuid: format!("00000000-0000-0000-0000-0000000000{i:02x}"),
                short_id: format!("000000{i:02X}"),
                hidden: false,
                content_present: true,
            })
            .collect();
        self.enqueue(Ok(DeviceLibrary {
            entries,
            had_trailing_bytes: false,
        }));
    }

    pub fn enqueue_empty_library(&self) {
        self.enqueue(Ok(DeviceLibrary::default()));
    }

    /// Queue a recoverable read failure mimicking the device disappearing
    /// mid-read (AC #3).
    pub fn enqueue_disconnected_mid_read(&self) {
        self.enqueue(Err(AppError::device_scan_failed(
            "Lecture de la bibliothèque appareil indisponible: vérifie que la Lunii est branchée et réessaie.",
            "Vérifie la connexion de la Lunii puis réessaie la lecture de la bibliothèque.",
        )
        .with_details(serde_json::json!({
            "source": "fs_read",
            "kind": "not_found",
        }))));
    }
}

impl DeviceLibraryReader for MockDeviceLibraryReader {
    fn read_library(
        &self,
        _mount_path: &Path,
        _budget: Duration,
    ) -> Result<DeviceLibrary, AppError> {
        let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_empty() {
            Ok(DeviceLibrary::default())
        } else {
            g.remove(0)
        }
    }
}

/// One scripted outcome of [`MockDevicePackReader`]. Beyond the result,
/// each script can stage files into the caller-provided `staging_dir` so
/// the import service has something real to promote — or a partial
/// residue proving the cleanup paths (AC #3).
enum PackAcquisitionScript {
    /// Write a plausible staged pack and succeed.
    Success,
    /// Write a PARTIAL residue into the staging dir, then fail — models
    /// a device unplugged mid-copy.
    FailMidCopy,
    /// Fail without touching the staging dir (structural refusal).
    FailValidation(AppError),
}

/// Programmable mock for the pack-acquisition path. Each `acquire_pack()`
/// call pops the next scripted outcome (FIFO); an empty queue acts as
/// [`PackAcquisitionScript::Success`], mirroring the permissive defaults
/// of the sibling mocks.
#[derive(Clone, Default)]
pub struct MockDevicePackReader {
    queue: Arc<Mutex<Vec<PackAcquisitionScript>>>,
}

impl MockDevicePackReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue_success(&self) {
        let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        g.push(PackAcquisitionScript::Success);
    }

    /// Script a mid-copy interruption: a partial file lands in staging,
    /// then the acquisition fails with a recoverable `fs_read` error.
    pub fn enqueue_interrupted_mid_copy(&self) {
        let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        g.push(PackAcquisitionScript::FailMidCopy);
    }

    /// Script a structural refusal (`pack_invalid`), staging nothing.
    pub fn enqueue_pack_invalid(&self) {
        let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        g.push(PackAcquisitionScript::FailValidation(
            AppError::import_failed(
                "Copie impossible: le contenu de l'histoire n'est pas dans un format supporté.",
                "Consulte le profil de support pour les contenus pris en charge.",
            )
            .with_details(serde_json::json!({
                "source": "pack_invalid",
                "cause": "unknown_entry",
            })),
        ));
    }

    /// The deterministic staged shape produced by a `Success` script.
    pub fn staged_manifest() -> PackManifest {
        let files = vec![
            PackFile {
                rel_path: "li".into(),
                size: 2,
            },
            PackFile {
                rel_path: "ni".into(),
                size: 4,
            },
            PackFile {
                rel_path: "rf/000/AAAAAAAA".into(),
                size: 8,
            },
            PackFile {
                rel_path: "ri".into(),
                size: 2,
            },
            PackFile {
                rel_path: "si".into(),
                size: 2,
            },
        ];
        let total_bytes = files.iter().map(|f| f.size).sum();
        PackManifest { files, total_bytes }
    }

    fn stage_success_files(staging_dir: &Path) {
        std::fs::write(staging_dir.join("ni"), b"NINI").expect("stage ni");
        std::fs::write(staging_dir.join("li"), b"LI").expect("stage li");
        std::fs::write(staging_dir.join("ri"), b"RI").expect("stage ri");
        std::fs::write(staging_dir.join("si"), b"SI").expect("stage si");
        let rf = staging_dir.join("rf").join("000");
        std::fs::create_dir_all(&rf).expect("stage rf/000");
        std::fs::write(rf.join("AAAAAAAA"), b"ASSETDAT").expect("stage asset");
    }
}

impl DevicePackReader for MockDevicePackReader {
    fn acquire_pack(
        &self,
        _mount_path: &Path,
        _short_id: &str,
        staging_dir: &Path,
        _budget: Duration,
    ) -> Result<AcquiredPack, AppError> {
        let script = {
            let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
            if g.is_empty() {
                PackAcquisitionScript::Success
            } else {
                g.remove(0)
            }
        };
        match script {
            PackAcquisitionScript::Success => {
                Self::stage_success_files(staging_dir);
                Ok(AcquiredPack {
                    manifest: Self::staged_manifest(),
                    checksum: "ab".repeat(32),
                })
            }
            PackAcquisitionScript::FailMidCopy => {
                std::fs::write(staging_dir.join("ni"), b"PART").expect("stage partial");
                Err(AppError::import_failed(
                    "Copie impossible: lecture de l'appareil interrompue.",
                    "Vérifie la connexion de la Lunii puis réessaie la copie.",
                )
                .with_details(serde_json::json!({
                    "source": "fs_read",
                    "kind": "not_found",
                })))
            }
            PackAcquisitionScript::FailValidation(err) => Err(err),
        }
    }
}
