//! Local diagnostic traces for the transfer/preparation flow (NFR23).
//!
//! Append-only JSONL in `{app_data_dir}/diagnostics/transfer.jsonl`, factored on
//! the shared [`jsonl`] helper exactly like [`device_log`](super::device_log).
//! The event set is CLOSED and PII-free: the story is referenced by a SHORT HASH
//! (`story_ref`), never a raw id or path, and no device/title metadata leaks.
//! Rotation cap matches the other channels (10 MB).

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use super::jsonl::{self, JsonlError};
use crate::domain::shared::AppError;

/// Soft size cap before rotation — identical to the recovery / device logs.
pub const MAX_TRANSFER_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Closed, PII-free event set for the preparation flow. `cause` is the stable
/// diagnostic tag of [`PreparationFailureCause`](crate::domain::transfer::PreparationFailureCause).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum Event {
    /// A preparation job was accepted and started.
    PreparationStarted { story_ref: String },
    /// A preparation job reached the `prepared` state.
    PreparationCompleted { story_ref: String, elapsed_ms: u64 },
    /// A preparation job reached a `retryable` / transport failure.
    PreparationFailed {
        story_ref: String,
        cause: &'static str,
        elapsed_ms: u64,
    },
}

/// Short, stable, PII-free reference to a story id (first 16 hex of its
/// SHA-256). Never the raw id, never a path.
pub fn story_ref(story_id: &str) -> String {
    let full = format!("{:x}", Sha256::digest(story_id.as_bytes()));
    full[..16].to_string()
}

/// Append an event using the app's resolved data dir. Best-effort by contract —
/// the caller ignores the result so a trace failure never blocks preparation.
pub fn record_event(app: &AppHandle, event: Event) -> Result<(), AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| trace_unavailable("diagnostics_app_data_dir"))?;
    let log_path = log_path_for(&app_data_dir);
    record_event_at_path(&log_path, event)
}

/// Resolve the trace channel path under `app_data_dir`.
pub fn log_path_for(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("diagnostics").join("transfer.jsonl")
}

/// Append directly at a path (used by tests with a temp dir).
pub fn record_event_at_path(log_path: &Path, event: Event) -> Result<(), AppError> {
    jsonl::append_event(log_path, &event, MAX_TRANSFER_LOG_BYTES, "transfer")
        .map_err(map_jsonl_error)
}

fn map_jsonl_error(err: JsonlError) -> AppError {
    let (source, kind) = match &err {
        JsonlError::DirNotWritable { kind } => ("diagnostics_dir", kind.as_str()),
        JsonlError::PathInvalid => ("diagnostics_path_invalid", "n_a"),
        JsonlError::Open(kind) => ("diagnostics_open", kind.as_str()),
        JsonlError::Write(kind) => ("diagnostics_write", kind.as_str()),
        JsonlError::Serialize => ("diagnostics_serialize", "n_a"),
        JsonlError::SystemClock => ("diagnostics_clock", "n_a"),
        JsonlError::Rotate(kind) => ("diagnostics_rotate", kind.as_str()),
    };
    trace_unavailable_with(source, kind)
}

fn trace_unavailable(source: &'static str) -> AppError {
    trace_unavailable_with(source, "n_a")
}

fn trace_unavailable_with(source: &str, kind: &str) -> AppError {
    AppError::preparation_failed(
        "Trace de préparation indisponible.",
        "Vérifie l'espace disque et les permissions puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": source,
        "kind": kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_path_is_under_diagnostics() {
        let path = log_path_for(Path::new("/tmp/app"));
        assert!(path.ends_with("diagnostics/transfer.jsonl"));
    }

    #[test]
    fn story_ref_is_short_stable_hex_and_not_the_raw_id() {
        let id = "0197a5d0-0000-7000-8000-000000000000";
        let r = story_ref(id);
        assert_eq!(r.len(), 16);
        assert!(r.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(r, id);
        assert_eq!(r, story_ref(id), "must be deterministic");
    }

    #[test]
    fn events_serialize_with_category_tag() {
        let v = serde_json::to_value(Event::PreparationStarted {
            story_ref: "abcd".into(),
        })
        .expect("ser");
        assert_eq!(v["category"], "preparation_started");
        assert_eq!(v["story_ref"], "abcd");

        let v = serde_json::to_value(Event::PreparationFailed {
            story_ref: "abcd".into(),
            cause: "device_changed",
            elapsed_ms: 12,
        })
        .expect("ser");
        assert_eq!(v["category"], "preparation_failed");
        assert_eq!(v["cause"], "device_changed");
    }

    #[test]
    fn appends_a_line_per_event_to_a_temp_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("diagnostics").join("transfer.jsonl");
        record_event_at_path(
            &path,
            Event::PreparationStarted {
                story_ref: "ref1".into(),
            },
        )
        .expect("write start");
        record_event_at_path(
            &path,
            Event::PreparationCompleted {
                story_ref: "ref1".into(),
                elapsed_ms: 5,
            },
        )
        .expect("write completed");
        let contents = std::fs::read_to_string(&path).expect("read log");
        assert_eq!(contents.lines().count(), 2);
        assert!(contents.contains("preparation_started"));
        assert!(contents.contains("preparation_completed"));
    }
}
