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

#[path = "integration/story_drafts_recovery.rs"]
mod story_drafts_recovery;

#[path = "integration/recovery_log.rs"]
mod recovery_log;
