//! Append-only JSONL log of external-source ingestion events.
//!
//! Sibling to `device_log.rs`: each call to [`record_event`] writes a
//! single newline-terminated JSON line to
//! `{app_data_dir}/diagnostics/import.jsonl`. The line carries a stable
//! `category` token from a closed set. PII discipline: the feed HOST at
//! most — never the full address (query strings can carry private
//! tokens), never the feed content, never a raw network message.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::jsonl::{self, JsonlError};
use crate::domain::shared::AppError;

/// Soft cap on the live `import.jsonl` file.
pub const MAX_IMPORT_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Diagnostic events emitted by the external-source (RSS) creation flow.
/// Closed by design so a log consumer can grep / route without surprise.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum Event {
    /// A preview fetch+parse settled. `state` is the verdict tag of the
    /// flow analysis (`needs_review` for an exploitable feed, `blocked`
    /// for a typed content verdict — nothing was persisted either way).
    RssPreviewSettled {
        host: String,
        state: &'static str,
        item_count: usize,
    },
    /// An accepted ingestion was committed. `import_state` is the durable
    /// tag actually persisted on the provenance row.
    RssCreationSettled {
        host: String,
        import_state: &'static str,
    },
    /// The accept re-fetch refused honestly: the source diverged from the
    /// previewed state (missing/ambiguous item, or a feed turned blocked).
    RssSourceChanged { host: String },
    /// The fetch transport failed (preview or accept). `stage` mirrors the
    /// upstream `AppError` `details.stage` closed set. STRICTLY the
    /// `RSS_SOURCE_UNREACHABLE` code — a local failure of the accept
    /// (SQLite commit, clock, worker join) is [`Event::RssCreationFailed`],
    /// never counted as a network problem.
    RssSourceUnreachable { host: String, stage: String },
    /// An accepted ingestion failed LOCALLY (DB commit, clock, worker
    /// join…): `code` is the wire error code (`IMPORT_FAILED`…), `source`
    /// mirrors the upstream `details.source` when present.
    RssCreationFailed {
        host: String,
        code: String,
        source: String,
    },
    /// A creation flow was refused by the content-source POLICY: the
    /// requested kind is not enabled by the distribution
    /// (`CONTENT_SOURCE_UNAVAILABLE`). The KIND wire tag only — the
    /// refusal precedes any network dispatch, so no host exists yet (and
    /// the PII discipline forbids the address anyway). NEVER counted as
    /// [`Event::RssSourceUnreachable`] (network) nor
    /// [`Event::RssCreationFailed`] (local).
    ContentSourceBlocked { kind: String },
}

/// Append a single event to the import log. Production entry point —
/// wraps an [`AppHandle`] to get the app-data dir from the Tauri runtime.
/// Tests use [`record_event_at_path`] directly.
pub fn record_event(app: &AppHandle, event: Event) -> Result<(), AppError> {
    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        AppError::import_failed(
            "Trace locale inaccessible.",
            "Vérifie les permissions de ton dossier utilisateur puis relance.",
        )
        .with_details(serde_json::json!({
            "source": "diagnostics_app_data_dir",
        }))
    })?;
    let log_path = log_path_for(&app_data_dir);
    record_event_at_path(&log_path, event)
}

/// Resolve the canonical log path inside an app-data dir.
pub fn log_path_for(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("diagnostics").join("import.jsonl")
}

/// Path-direct entry point. Exposed `pub` rather than `pub(crate)` so the
/// integration test crate can exercise it without a Tauri runtime.
pub fn record_event_at_path(log_path: &Path, event: Event) -> Result<(), AppError> {
    jsonl::append_event(log_path, &event, MAX_IMPORT_LOG_BYTES, "import").map_err(map_jsonl_error)
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
    AppError::import_failed(
        "Trace locale inaccessible.",
        "Vérifie l'espace disque et les permissions de ton dossier utilisateur.",
    )
    .with_details(serde_json::json!({
        "source": source,
        "kind": kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn events_serialize_with_stable_categories_and_host_only() {
        let dir = TempDir::new().expect("tempdir");
        let log = dir.path().join("import.jsonl");
        record_event_at_path(
            &log,
            Event::RssPreviewSettled {
                host: "exemple.fr".into(),
                state: "needs_review",
                item_count: 3,
            },
        )
        .expect("write");
        record_event_at_path(
            &log,
            Event::RssSourceUnreachable {
                host: "exemple.fr".into(),
                stage: "request".into(),
            },
        )
        .expect("write");
        let content = std::fs::read_to_string(&log).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"category\":\"rss_preview_settled\""));
        assert!(lines[0].contains("\"host\":\"exemple.fr\""));
        assert!(lines[1].contains("\"category\":\"rss_source_unreachable\""));
        // Host only — never a scheme/path fragment of the full address.
        assert!(!content.contains("http"));
    }

    #[test]
    fn content_source_blocked_serializes_the_kind_and_nothing_else() {
        let dir = TempDir::new().expect("tempdir");
        let log = dir.path().join("import.jsonl");
        record_event_at_path(&log, Event::ContentSourceBlocked { kind: "rss".into() })
            .expect("write");
        let content = std::fs::read_to_string(&log).expect("read");
        assert!(content.contains("\"category\":\"content_source_blocked\""));
        assert!(content.contains("\"kind\":\"rss\""));
        // A policy refusal precedes any network dispatch: no host, no
        // address fragment may ever appear on the line.
        assert!(!content.contains("host"));
        assert!(!content.contains("http"));
    }
}
