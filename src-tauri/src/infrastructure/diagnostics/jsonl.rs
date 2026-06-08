//! Shared JSONL append-only helper used by every diagnostics writer.
//!
//! Concerns kept here:
//! - directory writability probe with a process-wide success cache
//! - rotation when the live file would cross a configurable byte cap
//! - typed error variants so each log family maps the failure to its
//!   own `AppError` taxonomy without parsing strings.
//!
//! Concerns left to the caller:
//! - serialization of the actual event payload (each log family owns
//!   its own closed `Event` enum with its own field shape)
//! - mapping `JsonlError` to a domain `AppError` (recovery vs device
//!   diagnostics surface different user-facing copy)

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::application::story::now_iso_ms;

/// Process-wide monotonic counter used as a tie-breaker when two
/// rotations land in the same millisecond. Without it, two append
/// calls that both crossed the cap inside one ms would race on the
/// same archived filename — the second `fs::rename` would clobber
/// the first archive. Pairing the timestamp with the counter
/// guarantees unique archive names even under tight bursts.
static ROTATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Process-wide cache of diagnostic-directory paths we already proved
/// writable in this run. Without it, every event re-runs
/// `ensure_dir_writable` — which performs a sentinel-file probe.
/// That's wasted I/O on the hot path. Once a path has been validated,
/// repeated appends on it skip the probe.
fn ensure_dir_writable_cached(path: &Path) -> Result<(), JsonlError> {
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
        JsonlError::DirNotWritable {
            kind: err
                .details
                .as_ref()
                .and_then(|d| d.get("kind"))
                .and_then(|k| k.as_str())
                .unwrap_or("other")
                .to_string(),
        }
    })?;
    let mut guard = VERIFIED.lock().unwrap_or_else(|p| p.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    set.insert(path.to_path_buf());
    Ok(())
}

/// Typed failure modes from the JSONL helper. Callers map each variant
/// into their own `AppError` family with appropriate user-facing copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonlError {
    DirNotWritable { kind: String },
    PathInvalid,
    Open(String),
    Write(String),
    Serialize,
    SystemClock,
    Rotate(String),
}

/// Append one serializable event as a single JSONL line. Rotation is
/// best-effort: a rotation failure does not prevent the write — better
/// log a slightly oversized file than lose the line.
pub fn append_event<E: Serialize>(
    log_path: &Path,
    event: &E,
    max_bytes: u64,
    archive_prefix: &str,
) -> Result<(), JsonlError> {
    let parent = log_path.parent().ok_or(JsonlError::PathInvalid)?;
    ensure_dir_writable_cached(parent)?;

    if let Ok(metadata) = fs::metadata(log_path) {
        if metadata.len() >= max_bytes {
            let _ = rotate(log_path, archive_prefix);
        }
    }

    let line = serialize_line(event)?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|err| JsonlError::Open(io_kind_label(err.kind()).into()))?;
    file.write_all(line.as_bytes())
        .map_err(|err| JsonlError::Write(io_kind_label(err.kind()).into()))?;
    file.flush()
        .map_err(|err| JsonlError::Write(io_kind_label(err.kind()).into()))?;
    Ok(())
}

fn rotate(log_path: &Path, archive_prefix: &str) -> Result<(), JsonlError> {
    let parent = log_path
        .parent()
        .expect("checked by caller in append_event");
    let now = now_iso_ms()
        .map_err(|_| JsonlError::SystemClock)?
        .replace(':', "-");
    let seq = ROTATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let archived = parent.join(format!("{archive_prefix}-{now}-{seq}.jsonl.archived"));
    fs::rename(log_path, &archived)
        .map_err(|err| JsonlError::Rotate(io_kind_label(err.kind()).into()))
}

fn serialize_line<E: Serialize>(event: &E) -> Result<String, JsonlError> {
    // A clock failure must NOT swallow the event category — that is the
    // NFR24 stable identifier support relies on. If `now_iso_ms()` fails
    // (system clock outside the formattable range), fall back to the
    // sentinel string `"unknown-clock"` and keep the rest of the line.
    let now = now_iso_ms().unwrap_or_else(|_| String::from("unknown-clock"));
    let payload = serde_json::json!({
        "ts": now,
        "event": event,
    });
    let mut line = serde_json::to_string(&payload).map_err(|_| JsonlError::Serialize)?;
    line.push('\n');
    Ok(line)
}

pub fn io_kind_label(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind::*;
    match kind {
        NotFound => "not_found",
        PermissionDenied => "permission_denied",
        StorageFull => "storage_full",
        ReadOnlyFilesystem => "read_only_filesystem",
        TimedOut => "timeout",
        Interrupted => "interrupted",
        _ => "other",
    }
}
