// Integration tests root — Cargo only auto-discovers `tests/*.rs`; this file
// wires up `tests/integration/*.rs` modules so they compile and run.
#[path = "integration/storage_init.rs"]
mod storage_init;

#[path = "integration/story_persistence.rs"]
mod story_persistence;

#[path = "integration/story_export.rs"]
mod story_export;

#[path = "integration/story_drafts_migration.rs"]
mod story_drafts_migration;

#[path = "integration/story_imports_migration.rs"]
mod story_imports_migration;

#[path = "integration/story_local_imports_migration.rs"]
mod story_local_imports_migration;

#[path = "integration/node_content_migration.rs"]
mod node_content_migration;

#[path = "integration/multi_node_structure_migration.rs"]
mod multi_node_structure_migration;

#[path = "integration/import_review_resolution_migration.rs"]
mod import_review_resolution_migration;

#[path = "integration/story_edit_scope.rs"]
mod story_edit_scope;

#[path = "integration/story_drafts_recovery.rs"]
mod story_drafts_recovery;

#[path = "integration/recovery_log.rs"]
mod recovery_log;

#[path = "integration/device_scan.rs"]
mod device_scan;

#[path = "integration/device_log.rs"]
mod device_log;

#[path = "integration/device_command.rs"]
mod device_command;

#[path = "integration/device_library.rs"]
mod device_library;

#[path = "integration/device_import.rs"]
mod device_import;

#[path = "integration/device_titles.rs"]
mod device_titles;

#[path = "integration/transfer_preview.rs"]
mod transfer_preview;

#[path = "integration/story_validation.rs"]
mod story_validation;

#[path = "integration/story_preparation.rs"]
mod story_preparation;

#[path = "integration/story_transfer.rs"]
mod story_transfer;

#[path = "integration/transfer_outcome.rs"]
mod transfer_outcome;
