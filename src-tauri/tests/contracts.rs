// Contract tests root — see `tests/integration.rs` for the same pattern.
#[path = "contracts/import_export.rs"]
mod import_export;

#[path = "contracts/library_overview.rs"]
mod library_overview;

#[path = "contracts/story.rs"]
mod story;

#[path = "contracts/device.rs"]
mod device;

#[path = "contracts/device_library.rs"]
mod device_library;

#[path = "contracts/device_import.rs"]
mod device_import;

#[path = "contracts/transfer_preview.rs"]
mod transfer_preview;

#[path = "contracts/story_validation.rs"]
mod story_validation;
