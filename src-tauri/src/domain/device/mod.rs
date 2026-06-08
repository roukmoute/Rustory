//! Device domain layer.
//!
//! Canonical, framework-free types describing the device families Rustory
//! officially knows about, the per-profile operation allow-list, and the
//! filesystem markers used to recognize a candidate volume. Strictly
//! independent of `infrastructure/` and of `tauri::*`: the IPC layer
//! converts these types into wire DTOs at the boundary.

pub mod family;
pub mod markers;
pub mod operations;
pub mod profile;

pub use family::{DeviceFamily, LuniiFirmwareCohort};
pub use markers::{
    LUNII_BINARY_TOKEN_MARKER, LUNII_DEVICE_ID_MARKER, LUNII_LIB_INFO_MARKER, LUNII_PRIMARY_MARKER,
    LUNII_ROM_INFO_MARKER, MAX_METADATA_FILE_BYTES,
};
pub use operations::{SupportedOperation, SupportedOperations};
pub use profile::{classify_lunii, DeviceProfile, DeviceProfileClassification, UnsupportedReason};
