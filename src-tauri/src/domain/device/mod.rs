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
pub mod pack_transcode;
pub mod profile;
pub mod support_matrix;
pub mod title;

pub use family::{DeviceFamily, FirmwareCohort, FlamFirmwareCohort, LuniiFirmwareCohort};
pub use library::{
    format_pack_uuid, is_canonical_pack_uuid, pack_short_id, parse_canonical_pack_uuid,
    parse_flam_library_index, parse_pack_index, DeviceLibrary, DeviceStoryEntry, PackIndex,
    LUNII_PACK_UUID_BYTES, MAX_PACK_INDEX_BYTES,
};
pub use markers::{
    FLAM_CONFIG_DIR, FLAM_HIDDEN_LIBRARY_INDEX_REL, FLAM_HIDDEN_STORY_DIR, FLAM_LIBRARY_INDEX_REL,
    FLAM_PRIMARY_MARKER, FLAM_STORY_DIR, LUNII_BINARY_TOKEN_MARKER, LUNII_CONTENT_DIR,
    LUNII_DEVICE_ID_MARKER, LUNII_HIDDEN_INDEX_MARKER, LUNII_LIB_INFO_MARKER, LUNII_PRIMARY_MARKER,
    LUNII_ROM_INFO_MARKER, MAX_METADATA_FILE_BYTES,
};
pub use operations::{SupportedOperation, SupportedOperations};
pub use pack::{
    imported_story_title, is_os_cruft, validate_pack_inventory, PackEntry, PackEntryKind, PackFile,
    PackManifest, PackValidationIssue, MAX_IMPORT_PACK_BYTES, MAX_IMPORT_PACK_FILES,
    MAX_PACK_ASSET_DEPTH, OPTIONAL_PACK_FILES, OS_CRUFT_NAMES, OS_CRUFT_PREFIX, PACK_ASSET_DIRS,
    REQUIRED_PACK_FILES,
};
pub use pack_transcode::{transcode_pack, StudioStoryPack, TranscodeError, TranscodedPack};
pub use profile::{
    classify_flam, classify_lunii, DeviceProfile, DeviceProfileClassification, UnsupportedReason,
};
pub use support_matrix::{
    official_device_support_matrix, supported_operations_for, supported_operations_in,
    DeviceOperationsSupport, DeviceSupportLine, OperationSupport, ALL_FIRMWARE_COHORTS,
};
pub use title::{PackTitle, PackTitleCandidates, PackTitleSource, TitleValue};
