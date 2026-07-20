//! Append-only JSONL log of update events — the availability check AND
//! the apply gesture (`Update Availability Contract` + `Update Apply
//! Contract`), one file, one discipline.
//!
//! Sibling to `device_log.rs` / `import_log.rs`: each call to
//! [`record_event_at_path`] writes a single newline-terminated JSON
//! line to `{app_data_dir}/diagnostics/update.jsonl` — one line per
//! settled check, ONE line per named gesture transition (start,
//! terminal, restart request — NEVER a line per chunk). The line
//! carries a stable `category` token from a closed set. PII
//! discipline: closed-set tokens and a bare `MAJOR.MINOR.PATCH` at
//! most — NEVER a URL, never an absolute path, never a raw network
//! message.
//!
//! Best-effort BY CONTRACT at every call site (`let _ = …`): losing a
//! trace line must never degrade the check or the gesture, let alone
//! the core flow. Unlike its siblings, this module has NO `AppHandle`
//! entry point: the producers are `application::update` (running on
//! `spawn_blocking` workers, carrying a pre-resolved path via
//! `log_path_for`) and the restart command (ONE best-effort line on the
//! IPC thread, resolving its own path right before the process dies) —
//! every caller resolves its path itself, so an `AppHandle`-wrapping
//! sibling would have nothing left to serve. And unlike its siblings,
//! the error surface stays the raw
//! [`JsonlError`]: no wire error family exists for the check or the
//! gesture (their failures are DTO states) and none is invented for
//! their trace.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::jsonl::{self, JsonlError};

/// Soft cap on the live `update.jsonl` file.
pub const MAX_UPDATE_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Diagnostic events emitted by the update-availability check. Closed by
/// design so a log consumer can grep / route without surprise.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum Event {
    /// The per-launch decision skipped the check. `reason` is the closed
    /// motive set (`development_build`, `unofficial_install`) — the
    /// motive lives HERE only, the wire carries the single `checkNotRun`
    /// state.
    UpdateCheckSkipped { reason: &'static str },
    /// A check ran and settled on a verdict. `result` is the closed wire
    /// tag set (`updateAvailable`, `upToDate`, `checkUnavailable`);
    /// `latest` is the bare parsed version (`MAJOR.MINOR.PATCH`), present
    /// IFF a newer version was found — omitted, never `null`.
    UpdateCheckCompleted {
        result: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        latest: Option<String>,
    },
    /// The consultation transport failed. `stage` mirrors the closed
    /// [`crate::infrastructure::updates::UpdateFetchStage`] token set —
    /// never a URL, never a raw network message.
    UpdateCheckUnreachable { stage: &'static str },
    /// An apply gesture actually started (plan integrated, no gesture
    /// already in flight). ONE line per accepted start.
    UpdateApplyStarted,
    /// An apply gesture reached its successful terminal (the update is
    /// applied, restart pending). ONE line per gesture.
    UpdateApplyCompleted,
    /// An apply gesture reached its failure terminal. `stage` mirrors
    /// the closed
    /// [`crate::domain::update::UpdateApplyFailureStage`] token set —
    /// never a URL, never a raw network message.
    UpdateApplyFailed { stage: &'static str },
    /// The user asked to restart on a ready-to-restart gesture — traced
    /// BEFORE the restart (the process does not survive it).
    UpdateApplyRestartRequested,
}

/// Resolve the canonical log path inside an app-data dir — the command
/// frontier resolves it once and hands it to the `spawn_blocking`
/// worker (which has no `AppHandle`).
pub fn log_path_for(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("diagnostics").join("update.jsonl")
}

/// THE entry point, path-direct: production (`application::update`)
/// and the integration test crate both call it without a Tauri runtime.
pub fn record_event_at_path(log_path: &Path, event: Event) -> Result<(), JsonlError> {
    jsonl::append_event(log_path, &event, MAX_UPDATE_LOG_BYTES, "update")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn events_serialize_with_stable_categories_and_closed_tokens() {
        let dir = TempDir::new().expect("tempdir");
        let log = dir.path().join("update.jsonl");
        record_event_at_path(
            &log,
            Event::UpdateCheckSkipped {
                reason: "development_build",
            },
        )
        .expect("write");
        record_event_at_path(
            &log,
            Event::UpdateCheckCompleted {
                result: "updateAvailable",
                latest: Some("9.9.9".to_string()),
            },
        )
        .expect("write");
        record_event_at_path(&log, Event::UpdateCheckUnreachable { stage: "request" })
            .expect("write");
        let content = std::fs::read_to_string(&log).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("\"category\":\"update_check_skipped\""));
        assert!(lines[0].contains("\"reason\":\"development_build\""));
        assert!(lines[1].contains("\"category\":\"update_check_completed\""));
        assert!(lines[1].contains("\"latest\":\"9.9.9\""));
        assert!(lines[2].contains("\"category\":\"update_check_unreachable\""));
        assert!(lines[2].contains("\"stage\":\"request\""));
        // PII discipline: closed tokens and a bare version at most —
        // never a URL, never a path fragment.
        assert!(!content.contains("http"));
        assert!(!content.contains("github"));
        assert!(!content.contains("/"));
    }

    #[test]
    fn apply_gesture_events_serialize_with_stable_categories_and_closed_tokens() {
        let dir = TempDir::new().expect("tempdir");
        let log = dir.path().join("update.jsonl");
        record_event_at_path(&log, Event::UpdateApplyStarted).expect("write");
        record_event_at_path(&log, Event::UpdateApplyCompleted).expect("write");
        record_event_at_path(&log, Event::UpdateApplyFailed { stage: "download" }).expect("write");
        record_event_at_path(&log, Event::UpdateApplyRestartRequested).expect("write");
        let content = std::fs::read_to_string(&log).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("\"category\":\"update_apply_started\""));
        assert!(lines[1].contains("\"category\":\"update_apply_completed\""));
        assert!(lines[2].contains("\"category\":\"update_apply_failed\""));
        assert!(lines[2].contains("\"stage\":\"download\""));
        assert!(lines[3].contains("\"category\":\"update_apply_restart_requested\""));
        // PII discipline holds for the gesture too: closed tokens only —
        // never a URL, never a path fragment.
        assert!(!content.contains("http"));
        assert!(!content.contains("github"));
        assert!(!content.contains("/"));
    }

    #[test]
    fn a_settled_check_without_a_newer_version_omits_the_latest_key() {
        let dir = TempDir::new().expect("tempdir");
        let log = dir.path().join("update.jsonl");
        record_event_at_path(
            &log,
            Event::UpdateCheckCompleted {
                result: "upToDate",
                latest: None,
            },
        )
        .expect("write");
        let content = std::fs::read_to_string(&log).expect("read");
        assert!(content.contains("\"result\":\"upToDate\""));
        // Omission discipline: the key is absent, never `null`.
        assert!(!content.contains("latest"));
        assert!(!content.contains("null"));
    }
}
