//! Device infrastructure layer.
//!
//! Owns the Lunii filesystem detection mechanism: enumerate mounted USB
//! Mass Storage volumes, look for the canonical marker set
//! (`.md` + `.pi` required, `.bt`/`.ri`/`.li` informational only —
//! V3 firmware 3.3.2 was observed without `.bt`), read bounded
//! payloads from those markers, and surface the result as a typed
//! [`DeviceScanReport`]. Domain classification (cohort, supported
//! operations) lives in `domain/device/profile.rs` and consumes these
//! reports.

pub mod automount;
pub mod library_reader;
pub mod parser;
pub mod scanner;
pub mod system;

#[cfg(test)]
pub mod fixtures;
#[cfg(test)]
pub mod mock;

pub use automount::{
    looks_like_lunii_candidate, try_automount_lunii_candidates, MountAttempt, MountOutcome,
};
pub use library_reader::{DeviceLibraryReader, SystemDeviceLibraryReader};
pub use parser::{compute_device_identifier, parse_metadata_version, MetadataParseError};
pub use scanner::{DeviceCandidate, DeviceScanReport, DeviceScanner};
pub use system::{SystemDeviceScanner, EXTRA_MOUNT_ROOTS_ENV, SYSTEM_SCANNER_DEFAULT};

#[cfg(test)]
pub use mock::{MockDeviceLibraryReader, MockDeviceScanner};
