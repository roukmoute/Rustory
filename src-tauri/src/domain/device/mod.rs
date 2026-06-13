//! Device domain layer.
//!
//! Canonical, framework-free types describing the device families Rustory
//! officially knows about, the per-profile operation allow-list, and the
//! filesystem markers used to recognize a candidate volume. Strictly
//! independent of `infrastructure/` and of `tauri::*`: the IPC layer
//! converts these types into wire DTOs at the boundary.

pub mod family;
pub mod library;
pub mod markers;
pub mod operations;
pub mod pack;
pub mod profile;

pub use family::{DeviceFamily, LuniiFirmwareCohort};
pub use library::{
    format_pack_uuid, pack_short_id, parse_pack_index, DeviceLibrary, DeviceStoryEntry, PackIndex,
    LUNII_PACK_UUID_BYTES, MAX_PACK_INDEX_BYTES,
};
pub use markers::{
    LUNII_BINARY_TOKEN_MARKER, LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER,
    LUNII_HIDDEN_INDEX_MARKER, LUNII_LIB_INFO_MARKER, LUNII_PRIMARY_MARKER, LUNII_ROM_INFO_MARKER,
    MAX_METADATA_FILE_BYTES,
};
pub use operations::{SupportedOperation, SupportedOperations};
pub use pack::{
    imported_story_title, is_os_cruft, validate_pack_inventory, PackEntry, PackEntryKind, PackFile,
    PackManifest, PackValidationIssue, MAX_IMPORT_PACK_BYTES, MAX_IMPORT_PACK_FILES,
    MAX_PACK_ASSET_DEPTH, OPTIONAL_PACK_FILES, OS_CRUFT_NAMES, OS_CRUFT_PREFIX, PACK_ASSET_DIRS,
    REQUIRED_PACK_FILES,
};
pub use profile::{classify_lunii, DeviceProfile, DeviceProfileClassification, UnsupportedReason};
