use std::path::PathBuf;
use std::time::Duration;

use crate::domain::shared::AppError;

/// Filesystem-level USB Mass Storage scanner.
///
/// Returns the list of volumes that COULD be a Lunii based on root-marker
/// presence; profile classification (`domain/device/profile.rs`) happens
/// at the application layer using the content of those marker files.
///
/// Why a trait : the application layer must be testable without a real
/// USB device or a real OS mount. The mock impl in `mock.rs` lets
/// integration tests assemble fixtures (TempDir + simulated marker
/// files) and exercise the full scan→classify→DTO pipeline.
pub trait DeviceScanner: Send + Sync + 'static {
    /// Probe the system for candidate Lunii volumes. MUST respect the
    /// `budget` wall-clock — partial results are acceptable on timeout
    /// and SHOULD be returned with a `truncated_due_to_timeout` flag.
    fn scan(&self, budget: Duration) -> Result<DeviceScanReport, AppError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceScanReport {
    pub candidates: Vec<DeviceCandidate>,
    pub elapsed: Duration,
    pub truncated_due_to_timeout: bool,
}

impl DeviceScanReport {
    pub fn empty(elapsed: Duration) -> Self {
        Self {
            candidates: Vec::new(),
            elapsed,
            truncated_due_to_timeout: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCandidate {
    /// OS mount path (kept Rust-side only — never crosses IPC).
    pub mount_path: PathBuf,
    /// Raw `.md` payload (≤ MAX_METADATA_FILE_BYTES).
    pub metadata_payload: Vec<u8>,
    /// Raw `.pi` payload, used to compute device_identifier.
    pub pi_payload: Vec<u8>,
    /// `.bt` presence (content not needed at this stage).
    pub has_bt: bool,
    /// Stable, OS-provided volume serial when available; falls back to
    /// `None` on platforms or filesystems where it cannot be queried.
    pub volume_serial: Option<String>,
}
