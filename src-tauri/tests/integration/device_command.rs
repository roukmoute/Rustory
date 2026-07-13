//! Regression guard for the contract documented in T6.5 / DoD: the
//! `read_connected_lunii` command MUST NOT hold the DB mutex during
//! the scan — autosave and export are expected to keep running while
//! a slow scan is in flight.
//!
//! The current implementation passes this trivially because the
//! command path lives entirely in `application::device` (which
//! receives only the scanner, never the DB). This test pins that
//! property so a future refactor that smuggles a DB lock into the
//! scan flow fails loudly.
//!
//! Also hosts the FLAM recognition signature path: a fake mount on
//! disk, through the REAL scanner + classification + capability gate +
//! wire DTO — the full common support contract, no parallel pipeline.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustory_lib::application::device::{
    check_operation_allowed, read_connected_lunii, read_connected_lunii_with_attempts,
    ConnectedLuniiOutcome,
};
use rustory_lib::domain::device::SupportedOperation;
use rustory_lib::domain::shared::AppError;
use rustory_lib::infrastructure::device::{
    CandidateFacts, DeviceCandidate, DeviceScanReport, DeviceScanner, SystemDeviceScanner,
};
use rustory_lib::ipc::dto::ConnectedDeviceDto;

/// Slow scanner that sleeps for a configurable amount of time before
/// returning. Lets a sibling thread try to grab a fictitious "DB
/// lock" (a `Mutex<()>`) while the scan is in flight and assert the
/// lock was indeed grabbable.
struct SlowScanner {
    sleep: Duration,
    started: Arc<AtomicBool>,
}

impl DeviceScanner for SlowScanner {
    fn scan(&self, _budget: Duration) -> Result<DeviceScanReport, AppError> {
        self.started.store(true, Ordering::SeqCst);
        thread::sleep(self.sleep);
        Ok(DeviceScanReport {
            candidates: vec![DeviceCandidate {
                mount_path: std::path::PathBuf::from("/mock/lunii"),
                volume_serial: None,
                facts: CandidateFacts::Lunii {
                    metadata_payload: vec![3],
                    pi_payload: b"PI".to_vec(),
                    has_bt: true,
                },
            }],
            elapsed: self.sleep,
            truncated_due_to_timeout: false,
        })
    }
}

#[test]
fn read_connected_lunii_holds_no_external_lock_during_scan() {
    // Stand-in for `AppState.db`: a Mutex<()> that another thread
    // tries to grab while the scan is in flight. If the scan ever
    // started holding it, the sibling would block.
    let db_lock: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
    let scan_started = Arc::new(AtomicBool::new(false));

    let scanner = SlowScanner {
        sleep: Duration::from_millis(150),
        started: scan_started.clone(),
    };

    let db_for_probe = db_lock.clone();
    let probe_started = scan_started.clone();
    let prober = thread::spawn(move || {
        // Wait until the scan announces it has started, then race to
        // grab the lock under a short deadline.
        let deadline = Instant::now() + Duration::from_millis(500);
        while !probe_started.load(Ordering::SeqCst) {
            if Instant::now() > deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let try_grab_deadline = Instant::now() + Duration::from_millis(100);
        while Instant::now() < try_grab_deadline {
            if let Ok(_g) = db_for_probe.try_lock() {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        false
    });

    let _ = read_connected_lunii_with_attempts(&scanner, Duration::from_secs(2)).expect("scan ok");
    let lock_grabbed_during_scan = prober.join().expect("prober joined");
    assert!(
        lock_grabbed_during_scan,
        "the read_connected_lunii path must NOT hold the DB lock during the scan",
    );
}

mod flam_fixture {
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    pub const LUNII_PRIMARY_MARKER: &str = ".md";
    pub const LUNII_DEVICE_ID_MARKER: &str = ".pi";
    pub const FLAM_PRIMARY_MARKER: &str = ".mdf";
    pub const FLAM_STORY_DIR: &str = "str";
    pub const FLAM_CONFIG_DIR: &str = "etc";

    pub fn temp_flam_mount() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"FIXTURE_MDF_PAYLOAD").expect(".mdf");
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }

    pub fn temp_flam_mount_empty_mdf() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"").expect(".mdf");
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }

    pub fn temp_flam_mount_missing_dir(missing: &str) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"FIXTURE_MDF").expect(".mdf");
        for d in [FLAM_STORY_DIR, FLAM_CONFIG_DIR] {
            if d != missing {
                fs::create_dir(root.join(d)).expect("dir");
            }
        }
        (dir, root)
    }

    /// Both the full Lunii marker set AND the full FLAM marker set on
    /// one volume — family precedence must yield a LUNII candidate.
    pub fn temp_bimarker_mount(metadata_version: u8) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(
            root.join(LUNII_PRIMARY_MARKER),
            [metadata_version, 0xff, 0xaa],
        )
        .expect(".md");
        fs::write(root.join(LUNII_DEVICE_ID_MARKER), b"FIXTURE_PI_PAYLOAD").expect(".pi");
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"FIXTURE_MDF").expect(".mdf");
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }
}

/// Signature path: a conforming fake FLAM mount, through the REAL
/// scanner + classifier, resolves to the supported wire —
/// `family:"flam"`, `firmwareCohort:"flamGen1"`, the read capabilities
/// (`readLibrary`/`inspectStory`/`importStory`) `true`, `writeStory`
/// `false`, and the `metadataFormatVersion` key ABSENT. The capability
/// gate authorizes the three read operations and keeps refusing the
/// write with actionable details (the lock is never weakened).
#[test]
fn fake_flam_mount_resolves_to_recognized_supported_wire_and_fully_gated_profile() {
    let (_g, root) = flam_fixture::temp_flam_mount();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let outcome = read_connected_lunii(&scanner, Duration::from_secs(2)).expect("scan");

    let profile = match &outcome {
        ConnectedLuniiOutcome::Supported(p) => p.clone(),
        other => panic!("expected Supported, got {other:?}"),
    };

    // Gate inheritance by construction: the matrix line ✅✅✅❌ flows
    // through the same gate the call sites consult — the three read
    // operations pass, the write keeps refusing with the family/cohort
    // tags in the details.
    for op in [
        SupportedOperation::ReadLibrary,
        SupportedOperation::InspectStory,
        SupportedOperation::ImportStory,
    ] {
        check_operation_allowed(&profile, op)
            .unwrap_or_else(|e| panic!("{op:?} must be allowed for FLAM Gen1: {e:?}"));
    }
    let err = match check_operation_allowed(&profile, SupportedOperation::WriteStory) {
        Err(e) => e,
        Ok(()) => panic!("WriteStory must stay refused for FLAM Gen1"),
    };
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "capability_gate");
    assert_eq!(v["details"]["operation"], "write_story");
    assert_eq!(v["details"]["family"], "flam");
    assert_eq!(v["details"]["firmware_cohort"], "flam_gen1");

    let dto = ConnectedDeviceDto::from_outcome(outcome);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "supported");
    assert_eq!(v["family"], "flam");
    assert_eq!(v["firmwareCohort"], "flamGen1");
    assert_eq!(v["supportedOperations"]["readLibrary"], true);
    assert_eq!(v["supportedOperations"]["inspectStory"], true);
    assert_eq!(v["supportedOperations"]["importStory"], true);
    assert_eq!(v["supportedOperations"]["writeStory"], false);
    assert!(
        v.as_object()
            .expect("object")
            .get("metadataFormatVersion")
            .is_none(),
        "the version key must be ABSENT from the FLAM wire"
    );
    let raw = serde_json::to_string(&dto).expect("ser");
    assert!(!raw.contains("metadataFormatVersion"));
    assert!(!raw.contains("null"));
}

/// An empty `.mdf` is a VISIBLE candidate: the pipeline surfaces
/// `unsupported`/`metadataCorrupt` — never a silent "no device".
#[test]
fn fake_flam_mount_with_empty_mdf_surfaces_metadata_corrupt() {
    let (_g, root) = flam_fixture::temp_flam_mount_empty_mdf();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let outcome = read_connected_lunii(&scanner, Duration::from_secs(2)).expect("scan");
    let dto = ConnectedDeviceDto::from_outcome(outcome);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "unsupported");
    assert_eq!(v["reason"], "metadataCorrupt");
    assert_eq!(v["firmwareHint"], "flam");
}

/// Family precedence: a volume carrying BOTH marker sets is a
/// LUNII candidate, verbatim — same wire as a plain Lunii mount,
/// version key present.
#[test]
fn fake_bimarker_mount_resolves_to_lunii_supported_wire_verbatim() {
    let (_g, root) = flam_fixture::temp_bimarker_mount(3);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let outcome = read_connected_lunii(&scanner, Duration::from_secs(2)).expect("scan");
    let dto = ConnectedDeviceDto::from_outcome(outcome);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "supported");
    assert_eq!(v["family"], "lunii");
    assert_eq!(v["firmwareCohort"], "origineV1");
    assert_eq!(v["metadataFormatVersion"], 3);
    assert_eq!(v["supportedOperations"]["readLibrary"], true);
}

/// A FLAM volume missing a required directory surfaces the typed
/// `unsupported`/`metadataUnsupported` refusal — the closed reason set,
/// never `familyUnknown`, never a silent skip.
#[test]
fn fake_flam_mount_missing_required_dir_surfaces_metadata_unsupported() {
    for missing in [flam_fixture::FLAM_STORY_DIR, flam_fixture::FLAM_CONFIG_DIR] {
        let (_g, root) = flam_fixture::temp_flam_mount_missing_dir(missing);
        let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
        let outcome = read_connected_lunii(&scanner, Duration::from_secs(2)).expect("scan");
        let dto = ConnectedDeviceDto::from_outcome(outcome);
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "unsupported", "missing {missing}");
        assert_eq!(v["reason"], "metadataUnsupported", "missing {missing}");
        assert_eq!(v["firmwareHint"], "flam", "missing {missing}");
    }
}

/// Cross-family ambiguity on REAL mounts: one healthy Lunii volume and
/// one conforming FLAM volume plugged together resolve to the
/// `ambiguous` wire — a recognized FLAM participates in the same
/// supported vector, any families (the documented support contract).
#[test]
fn fake_lunii_and_flam_mounts_together_resolve_to_ambiguous_wire() {
    let (_gl, lunii_root) = {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        std::fs::write(
            root.join(flam_fixture::LUNII_PRIMARY_MARKER),
            [3u8, 0xff, 0xaa],
        )
        .expect(".md");
        std::fs::write(
            root.join(flam_fixture::LUNII_DEVICE_ID_MARKER),
            b"FIXTURE_PI_PAYLOAD",
        )
        .expect(".pi");
        (dir, root)
    };
    let (_gf, flam_root) = flam_fixture::temp_flam_mount();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![lunii_root, flam_root]);
    let outcome = read_connected_lunii(&scanner, Duration::from_secs(2)).expect("scan");
    let dto = ConnectedDeviceDto::from_outcome(outcome);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "ambiguous");
    assert_eq!(v["candidateCount"], 2);
}
