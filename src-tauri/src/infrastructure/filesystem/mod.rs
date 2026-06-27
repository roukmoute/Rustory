pub mod app_paths;
pub mod catalog_covers;
pub mod import_store;
pub mod node_media;
pub mod transfer_artifacts;

pub use app_paths::{ensure_app_data_dir, ensure_dir_writable, resolve_db_path, DB_FILENAME};
pub use catalog_covers::{
    clear_catalog_covers, ensure_catalog_covers_dir, read_catalog_cover,
    resolve_catalog_covers_dir, write_catalog_cover, MAX_COVER_BYTES,
};
pub use import_store::{
    ensure_import_store, resolve_import_story_dir, resolve_imports_dir,
    resolve_imports_staging_dir, IMPORTS_DIR_NAME, IMPORTS_STAGING_DIR_NAME,
};
pub use node_media::{
    ensure_node_media_store, read_media, resolve_node_media_dir, resolve_node_media_staging_dir,
    sniff_media, store_media, sweep_node_media_staging, MediaKind, NodeMediaError, StoredMedia,
    MAX_MEDIA_BYTES, NODE_MEDIA_DIR_NAME,
};
pub use transfer_artifacts::{
    AssemblyPlan, AssemblySource, SystemTransferArtifactSource, TransferArtifactSource,
};

#[cfg(test)]
pub use transfer_artifacts::MockTransferArtifactSource;

/// Short, PII-free `details.kind` tag for filesystem I/O failures crossing
/// the IPC boundary. Single source of truth shared by the export flow and
/// the device-import flow so the wire taxonomy never drifts between the
/// two (`docs/architecture/ui-states.md` documents this exact closed set).
pub fn io_error_kind_tag(err: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match err.kind() {
        PermissionDenied => "permission_denied",
        StorageFull => "no_space",
        ReadOnlyFilesystem => "read_only_filesystem",
        NotFound => "not_found",
        AlreadyExists => "already_exists",
        _ => "io",
    }
}
