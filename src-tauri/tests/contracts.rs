// Contract tests root — see `tests/integration.rs` for the same pattern.
#[path = "contracts/import_export.rs"]
mod import_export;

#[path = "contracts/rss_creation.rs"]
mod rss_creation;

#[path = "contracts/content_source_policy.rs"]
mod content_source_policy;

#[path = "contracts/support_profile.rs"]
mod support_profile;

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

#[path = "contracts/story_preparation.rs"]
mod story_preparation;

#[path = "contracts/story_transfer.rs"]
mod story_transfer;
