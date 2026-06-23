use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::domain::device::pack::{PackFile, PackManifest};
use crate::domain::device::{DeviceLibrary, DeviceStoryEntry};
use crate::domain::shared::AppError;

use super::catalog_source::OfficialCatalogSource;
use super::library_reader::DeviceLibraryReader;
use super::pack_reader::{AcquiredPack, DevicePackReader};
use super::scanner::{DeviceCandidate, DeviceScanReport, DeviceScanner};
use super::writer::{DevicePackWriter, WriteFailure, WriteProgress};
use crate::domain::transfer::{PackWritePlan, TransferFailureCause};

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

    /// A supported Lunii at a DISTINCT mount path + volume serial — models a device
    /// SWAPPED for another supported Lunii (e.g. between a write and the `verify`
    /// re-scan), so callers can prove the continuity check refuses it.
    pub fn enqueue_supported_lunii_swapped(&self, metadata_version: u8) {
        let report = DeviceScanReport {
            candidates: vec![DeviceCandidate {
                mount_path: std::path::PathBuf::from("/mock/lunii_other"),
                metadata_payload: vec![metadata_version],
                pi_payload: b"OTHER_PI".to_vec(),
                has_bt: true,
                volume_serial: Some("OTHER_SERIAL".into()),
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

/// Programmable mock for the official-catalog network source. Pops the next
/// queued raw-body result (FIFO); an empty queue returns an empty JSON
/// object (parses to zero entries) so a forgetful test fails on assertion,
/// not panic. Also records how many times `fetch` was called — used to prove
/// the read path NEVER hits the network (offline-first).
#[derive(Clone, Default)]
pub struct MockOfficialCatalogSource {
    queue: Arc<Mutex<Vec<Result<String, AppError>>>>,
    calls: Arc<Mutex<u32>>,
    cover_calls: Arc<Mutex<u32>>,
    fail_covers: Arc<Mutex<bool>>,
}

/// Smallest valid PNG header the cover store's magic-sniff accepts.
const MOCK_PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

impl MockOfficialCatalogSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue_body(&self, body: impl Into<String>) {
        self.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Ok(body.into()));
    }

    pub fn enqueue_failure(&self, err: AppError) {
        self.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Err(err));
    }

    /// Number of times `fetch` was invoked — `0` proves no network call.
    pub fn fetch_count(&self) -> u32 {
        *self.calls.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Number of times `fetch_cover` was invoked.
    pub fn cover_fetch_count(&self) -> u32 {
        *self.cover_calls.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Make every subsequent `fetch_cover` fail — exercises the best-effort
    /// "a failed cover never sinks the catalog" path.
    pub fn fail_all_covers(&self) {
        *self.fail_covers.lock().unwrap_or_else(|p| p.into_inner()) = true;
    }
}

impl OfficialCatalogSource for MockOfficialCatalogSource {
    fn fetch(&self, _locale: &str, _budget: Duration) -> Result<String, AppError> {
        *self.calls.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_empty() {
            Ok("{}".to_string())
        } else {
            g.remove(0)
        }
    }

    fn fetch_cover(&self, _url: &str, _budget: Duration) -> Result<Vec<u8>, AppError> {
        *self.cover_calls.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        if *self.fail_covers.lock().unwrap_or_else(|p| p.into_inner()) {
            return Err(AppError::official_catalog_unavailable(
                "cover offline",
                "retry",
            ));
        }
        Ok(MOCK_PNG.to_vec())
    }
}

/// One scripted outcome of [`MockDevicePackWriter`]. A success can drive the
/// progress reporter (so the service's `job:progress` emission is testable); a
/// failure carries whether the device was already mutated (for the
/// `Failed`/`Incomplete` distinction).
enum WriteScript {
    Success { progress: Vec<WriteProgress> },
    Failure(WriteFailure),
}

/// Programmable mock for the device write path. Records call count and returns
/// the next scripted outcome (FIFO); an empty queue succeeds. Lets the transfer
/// service prove its orchestration (gate-before-mutation, event sequence +
/// progress, terminal mapping incl. `Failed` vs `Incomplete`) without a real
/// volume. The recorded call count proves the writer is NEVER reached on an
/// unauthorized profile ("block before mutation").
#[derive(Clone, Default)]
pub struct MockDevicePackWriter {
    queue: Arc<Mutex<Vec<WriteScript>>>,
    calls: Arc<Mutex<u32>>,
}

impl MockDevicePackWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// A plain success (no progress reported).
    pub fn enqueue_success(&self) {
        self.push(WriteScript::Success {
            progress: Vec::new(),
        });
    }

    /// A success that drives the progress reporter with a monotone two-step
    /// fraction, so the service's `job:progress { phase: Transfer, progress }`
    /// emission can be asserted.
    pub fn enqueue_success_with_progress(&self) {
        self.push(WriteScript::Success {
            progress: vec![
                WriteProgress {
                    bytes_done: 1,
                    bytes_total: 2,
                },
                WriteProgress {
                    bytes_done: 2,
                    bytes_total: 2,
                },
            ],
        });
    }

    /// A failure BEFORE any device mutation (`Failed`): the existing content is
    /// intact — the realistic writer outcome for refusals + interruptions.
    pub fn enqueue_failure(&self, cause: TransferFailureCause) {
        self.push(WriteScript::Failure(WriteFailure {
            cause,
            reached_device_mutation: false,
        }));
    }

    /// A failure AFTER the device mutation began (`Incomplete`): content promoted
    /// but the durability/index step failed — a possible partial copy.
    pub fn enqueue_failure_after_mutation(&self, cause: TransferFailureCause) {
        self.push(WriteScript::Failure(WriteFailure {
            cause,
            reached_device_mutation: true,
        }));
    }

    /// Number of times `write_pack` was invoked — `0` proves the capability gate
    /// blocked the write before any device mutation.
    pub fn call_count(&self) -> u32 {
        *self.calls.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn push(&self, script: WriteScript) {
        self.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(script);
    }
}

impl DevicePackWriter for MockDevicePackWriter {
    fn write_pack(
        &self,
        _mount_path: &Path,
        _source_pack_dir: &Path,
        _pack_uuid: &str,
        _plan: &PackWritePlan,
        _budget: Duration,
        progress: &dyn Fn(WriteProgress),
    ) -> Result<(), WriteFailure> {
        *self.calls.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        let script = {
            let mut g = self.queue.lock().unwrap_or_else(|p| p.into_inner());
            if g.is_empty() {
                WriteScript::Success {
                    progress: Vec::new(),
                }
            } else {
                g.remove(0)
            }
        };
        match script {
            WriteScript::Success { progress: steps } => {
                for step in steps {
                    progress(step);
                }
                Ok(())
            }
            WriteScript::Failure(failure) => Err(failure),
        }
    }
}
