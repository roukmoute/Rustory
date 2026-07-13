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
    /// Stable, OS-provided volume serial when available; falls back to
    /// `None` on platforms or filesystems where it cannot be queried.
    pub volume_serial: Option<String>,
    /// Per-family facts probed at the volume root. The sum makes a
    /// bi-family candidate UNREPRESENTABLE by construction — no runtime
    /// validation to forget. Family precedence is decided by the probe
    /// (`.md` present ⇒ Lunii, even when `.mdf` coexists).
    pub facts: CandidateFacts,
}

/// Family-tagged facts collected by the volume probe. Each variant
/// carries exactly what that family's classifier consumes — nothing
/// cross-family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateFacts {
    Lunii {
        /// Raw `.md` payload (≤ MAX_METADATA_FILE_BYTES).
        metadata_payload: Vec<u8>,
        /// Raw `.pi` payload, used to compute device_identifier.
        pi_payload: Vec<u8>,
        /// `.bt` presence (content not needed at this stage).
        has_bt: bool,
    },
    Flam {
        /// Raw `.mdf` payload (≤ MAX_METADATA_FILE_BYTES), read
        /// no-follow. Hashed into the device identifier, never parsed.
        mdf_payload: Vec<u8>,
        /// `str/` present as a REAL directory (no-follow).
        has_str_dir: bool,
        /// `etc/` present as a REAL directory (no-follow).
        has_etc_dir: bool,
    },
}
