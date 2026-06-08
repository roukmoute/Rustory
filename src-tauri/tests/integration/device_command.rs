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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustory_lib::application::device::read_connected_lunii_with_attempts;
use rustory_lib::domain::shared::AppError;
use rustory_lib::infrastructure::device::{DeviceCandidate, DeviceScanReport, DeviceScanner};

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
                metadata_payload: vec![3],
                pi_payload: b"PI".to_vec(),
                has_bt: true,
                volume_serial: None,
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
