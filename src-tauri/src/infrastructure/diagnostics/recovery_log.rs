//! Append-only JSONL log of recovery-flow events.
//!
//! Each call to [`record_event`] writes a single newline-terminated JSON
//! line to `{app_data_dir}/diagnostics/recovery.jsonl`. The line carries
//! a stable `category` token from a closed set — that token is the NFR24
//! identifier support uses to triage. No raw OS messages, no localized
//! copy, no user content beyond the story id.
//!
//! Rotation is opportunistic: when the file would exceed
//! [`MAX_RECOVERY_LOG_BYTES`], it is renamed to a timestamped `.archived`
//! sibling before the new line is written. There is no compression and
//! no retention policy beyond that — diagnostics rooms cleans up archived
//! files manually if needed.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// Process-wide monotonic counter used as a tie-breaker when two
/// rotations land in the same millisecond. Without it, two `record_event`
/// calls that both crossed the cap inside one ms would race on the same
/// `recovery-{ts}.jsonl.archived` filename — the second `fs::rename`
/// would clobber the first archive. Pairing the timestamp with the
/// counter guarantees unique archive names even under tight bursts.
static ROTATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Process-wide cache of diagnostic-directory paths we already proved
/// writable in this run. Without it, every recovery-log event re-runs
/// `ensure_dir_writable` — which performs a sentinel-file create+write+
/// remove probe. That's wasted I/O on the hot path (one write per
/// keystroke at the 150 ms record cadence is plausible). Once a path
/// has been validated, repeated `record_event` calls on it skip the
/// probe.
fn ensure_dir_writable_cached(path: &Path) -> Result<(), AppError> {
    static VERIFIED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
    {
        let guard = VERIFIED.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(set) = guard.as_ref() {
            if set.contains(path) {
                return Ok(());
            }
        }
    }
    crate::infrastructure::filesystem::ensure_dir_writable(path).map_err(|err| {
        AppError::recovery_draft_unavailable(
            "Récupération indisponible: vérifie le disque local et réessaie.",
            "Vérifie l'espace disque et les permissions de ton dossier utilisateur.",
        )
        .with_details(serde_json::json!({
            "source": "diagnostics_dir",
            "kind": err.details.as_ref().and_then(|d| d.get("kind").cloned()),
        }))
    })?;
    let mut guard = VERIFIED.lock().unwrap_or_else(|p| p.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    set.insert(path.to_path_buf());
    Ok(())
}

use crate::application::story::now_iso_ms;
use crate::domain::shared::AppError;

/// Soft cap on the live `recovery.jsonl` file. Past this size, the file
/// is archived and a fresh one starts on the next append.
pub const MAX_RECOVERY_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Diagnostic events emitted by the recovery flow. The `category` tag is
/// the NFR24 stable identifier — the variant set is closed by design so a
/// log consumer can grep / route on it without surprise.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum Event {
    /// Boot probe: at least one row survived in `story_drafts`. Includes
    /// the exhaustive list of story ids so support can correlate.
    InterruptedSessionDetected { story_ids: Vec<String> },
    /// The UI mounted the recovery banner for a specific story.
    RecoveryDraftProposed { story_id: String },
    /// The user accepted the recovered draft. Emitted only on the success
    /// path of `apply_recovery`.
    RecoveryDraftApplied { story_id: String },
    /// The user dismissed the recovered draft. Emitted only on the
    /// success path of `discard_draft`.
    RecoveryDraftDiscarded { story_id: String },
    /// The recovery itself failed (transport, FS, lock). `source` is a
    /// closed set so the log line stays grep-able.
    RecoveryDraftUnavailable {
        story_id: String,
        source: &'static str,
    },
}

/// Append a single event to the recovery log.
///
/// This is the production entry point — wrap an [`AppHandle`] to get the
/// app-data dir from the Tauri runtime. Tests use [`record_event_at_path`]
/// directly to avoid the Tauri runtime dependency.
pub fn record_event(app: &AppHandle, event: Event) -> Result<(), AppError> {
    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        AppError::recovery_draft_unavailable(
            "Récupération indisponible: vérifie le disque local et réessaie.",
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
    app_data_dir.join("diagnostics").join("recovery.jsonl")
}

/// Path-direct entry point. Exposed `pub` rather than `pub(crate)` so the
/// integration test crate can exercise it without a Tauri runtime.
pub fn record_event_at_path(log_path: &Path, event: Event) -> Result<(), AppError> {
    let parent = log_path.parent().ok_or_else(|| {
        AppError::recovery_draft_unavailable(
            "Récupération indisponible: chemin de trace invalide.",
            "Relance Rustory ; si le problème persiste, consulte les traces locales.",
        )
        .with_details(serde_json::json!({
            "source": "diagnostics_path_invalid",
        }))
    })?;

    // Re-tag any FS error as a recovery-log diagnostic so the UI / log
    // consumer can tell where it actually came from. The original code
    // is dropped on purpose: the recovery flow has its own taxonomy.
    // Memoize the success: at the 150 ms record cadence the parent
    // directory is checked dozens of times per minute and the sentinel
    // probe is wasted I/O after the first proof.
    ensure_dir_writable_cached(parent)?;

    // Rotate before appending if we would cross the cap. Rotation is
    // best-effort: on rotation failure we still try to append (better
    // log a too-big file than lose the line).
    if let Ok(metadata) = fs::metadata(log_path) {
        if metadata.len() >= MAX_RECOVERY_LOG_BYTES {
            let _ = rotate(log_path);
        }
    }

    let line = serialize_line(&event)?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|err| map_io_error(&err, "diagnostics_open"))?;
    file.write_all(line.as_bytes())
        .map_err(|err| map_io_error(&err, "diagnostics_write"))?;
    file.flush()
        .map_err(|err| map_io_error(&err, "diagnostics_flush"))?;
    Ok(())
}

fn rotate(log_path: &Path) -> Result<(), AppError> {
    let parent = log_path.parent().expect("checked in record_event_at_path");
    let now = now_iso_ms()?.replace(':', "-");
    // Pair the timestamp with a monotonic counter so two rotations in
    // the same millisecond produce different filenames. Wrapping is
    // theoretical for this counter (u64 ≈ 600 years of one rotation
    // per second), but `Ordering::Relaxed` keeps the counter cheap.
    let seq = ROTATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let archived = parent.join(format!("recovery-{now}-{seq}.jsonl.archived"));
    fs::rename(log_path, &archived).map_err(|err| map_io_error(&err, "diagnostics_rotate"))
}

fn serialize_line(event: &Event) -> Result<String, AppError> {
    // A clock failure must NOT swallow the event category — that is the
    // NFR24 stable identifier support relies on. If `now_iso_ms()` fails
    // (system clock outside the formattable range), fall back to the
    // sentinel string `"unknown-clock"` and keep the rest of the line.
    let now = now_iso_ms().unwrap_or_else(|_| String::from("unknown-clock"));
    let payload = serde_json::json!({
        "ts": now,
        "event": event,
    });
    let mut line = serde_json::to_string(&payload).map_err(|_| {
        AppError::recovery_draft_unavailable(
            "Récupération indisponible: trace illisible.",
            "Relance Rustory ; si le problème persiste, consulte les traces locales.",
        )
        .with_details(serde_json::json!({
            "source": "diagnostics_serialize",
        }))
    })?;
    line.push('\n');
    Ok(line)
}

fn map_io_error(err: &std::io::Error, source: &'static str) -> AppError {
    use std::io::ErrorKind::*;
    let kind = match err.kind() {
        NotFound => "not_found",
        PermissionDenied => "permission_denied",
        StorageFull => "storage_full",
        ReadOnlyFilesystem => "read_only_filesystem",
        _ => "other",
    };
    AppError::recovery_draft_unavailable(
        "Récupération indisponible: vérifie le disque local et réessaie.",
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

    fn read_lines(path: &Path) -> Vec<String> {
        let raw = fs::read_to_string(path).expect("read");
        raw.lines().map(str::to_string).collect()
    }

    #[test]
    fn event_serializes_with_category_discriminator() {
        let event = Event::RecoveryDraftProposed {
            story_id: "id-1".into(),
        };
        let v = serde_json::to_value(&event).expect("serialize");
        assert_eq!(v["category"], "recovery_draft_proposed");
        assert_eq!(v["story_id"], "id-1");
    }

    #[test]
    fn event_recovery_draft_proposed_carries_story_id() {
        let event = Event::RecoveryDraftProposed {
            story_id: "id-42".into(),
        };
        let v = serde_json::to_value(&event).expect("serialize");
        assert_eq!(v["story_id"], "id-42");
    }

    #[test]
    fn event_interrupted_session_detected_carries_story_ids_array() {
        let event = Event::InterruptedSessionDetected {
            story_ids: vec!["a".into(), "b".into()],
        };
        let v = serde_json::to_value(&event).expect("serialize");
        assert_eq!(v["category"], "interrupted_session_detected");
        assert_eq!(v["story_ids"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn event_recovery_draft_unavailable_carries_source_token() {
        let event = Event::RecoveryDraftUnavailable {
            story_id: "x".into(),
            source: "apply_recovery",
        };
        let v = serde_json::to_value(&event).expect("serialize");
        assert_eq!(v["category"], "recovery_draft_unavailable");
        assert_eq!(v["source"], "apply_recovery");
    }

    #[test]
    fn record_event_appends_one_jsonl_line() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("diagnostics").join("recovery.jsonl");

        record_event_at_path(
            &path,
            Event::RecoveryDraftApplied {
                story_id: "id-1".into(),
            },
        )
        .expect("record");

        let lines = read_lines(&path);
        assert_eq!(lines.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable JSON");
        assert_eq!(parsed["event"]["category"], "recovery_draft_applied");
        assert!(parsed["ts"].is_string());
    }

    #[test]
    fn record_event_appends_without_overwriting_existing_lines() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("diagnostics").join("recovery.jsonl");

        for i in 0..3 {
            record_event_at_path(
                &path,
                Event::RecoveryDraftProposed {
                    story_id: format!("id-{i}"),
                },
            )
            .expect("record");
        }

        let lines = read_lines(&path);
        assert_eq!(lines.len(), 3, "every record must append a new line");
        for (i, line) in lines.iter().enumerate() {
            let parsed: serde_json::Value = serde_json::from_str(line).expect("parsable");
            assert_eq!(parsed["event"]["story_id"], format!("id-{i}"));
        }
    }

    #[test]
    fn record_event_creates_diagnostics_dir_if_missing() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("diagnostics").join("recovery.jsonl");
        assert!(
            !path.parent().unwrap().exists(),
            "fixture: parent must not exist yet"
        );

        record_event_at_path(
            &path,
            Event::RecoveryDraftDiscarded {
                story_id: "id-1".into(),
            },
        )
        .expect("record");

        assert!(path.parent().unwrap().exists());
        assert!(path.exists());
    }

    #[test]
    fn record_event_rotates_when_file_exceeds_10mb() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("diagnostics").join("recovery.jsonl");
        // Pre-create the file beyond the rotation threshold.
        fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        fs::write(&path, vec![b'x'; (MAX_RECOVERY_LOG_BYTES + 1) as usize])
            .expect("seed huge file");

        record_event_at_path(
            &path,
            Event::RecoveryDraftApplied {
                story_id: "after-rotation".into(),
            },
        )
        .expect("record");

        // The new live file must contain only the freshly-recorded line.
        let lines = read_lines(&path);
        assert_eq!(
            lines.len(),
            1,
            "post-rotation file must contain only the new line"
        );
        let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable");
        assert_eq!(parsed["event"]["story_id"], "after-rotation");

        // The archived sibling exists.
        let archived: Vec<_> = fs::read_dir(path.parent().unwrap())
            .expect("readdir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl.archived"))
            .collect();
        assert_eq!(
            archived.len(),
            1,
            "rotation must produce exactly one archive"
        );
    }

    #[test]
    fn log_path_for_resolves_under_diagnostics_subdir() {
        let p = Path::new("/tmp/app");
        let resolved = log_path_for(p);
        assert_eq!(resolved, Path::new("/tmp/app/diagnostics/recovery.jsonl"));
    }
}
