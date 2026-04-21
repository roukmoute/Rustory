use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use crate::domain::shared::AppError;

const WRITE_CHECK_PREFIX: &str = ".rustory-write-check-";

/// Monotonically increasing counter used as a last-resort entropy source for
/// sentinel filenames. Guarantees uniqueness within a single process even if
/// the system clock is broken or jumps backwards.
static PROBE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Build a sentinel filename unique per probe so two concurrent start-ups
/// never race on the same file, and a pre-existing file with the same name
/// is never clobbered. Uniqueness is guaranteed by the atomic counter; the
/// epoch timestamp is opportunistic extra entropy, only when available.
fn sentinel_filename() -> String {
    let counter = PROBE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_nanos())
        .map(|n| format!("-{n}"))
        .unwrap_or_default();
    format!("{WRITE_CHECK_PREFIX}{pid}-{counter}{nanos}")
}

/// Classify an `io::Error` into a short, PII-free diagnostic token. Absolute
/// paths and OS messages are NOT included in wire details — they may contain
/// the user's home directory or locale-specific strings.
fn io_error_kind_tag(err: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match err.kind() {
        NotFound => "not_found",
        PermissionDenied => "permission_denied",
        AlreadyExists => "already_exists",
        InvalidInput | InvalidData => "invalid_input",
        NotADirectory => "not_a_directory",
        IsADirectory => "is_a_directory",
        StorageFull => "storage_full",
        ReadOnlyFilesystem => "read_only_filesystem",
        _ => "other",
    }
}

/// Resolve the managed application data directory via the Tauri v2 path API,
/// ensure it exists and is writable.
///
/// Returns the resolved path on success, or [`AppError`] with code
/// `LocalStorageUnavailable` if resolution, creation or the write probe fails.
pub fn ensure_app_data_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_data_dir().map_err(|_| {
        AppError::local_storage_unavailable(
            "Le stockage local de Rustory n'a pas pu être localisé sur ce système.",
            "Vérifie les permissions de ton dossier utilisateur puis relance l'application.",
        )
        .with_details(serde_json::json!({ "source": "app_data_dir" }))
    })?;

    ensure_dir_writable(&dir)?;
    Ok(dir)
}

/// Ensure `path` exists as a writable directory.
///
/// Creates the directory tree if needed, then performs a sentinel-file write
/// probe to confirm the directory is actually writable by the current process.
///
/// Decoupled from [`AppHandle`] so it can be exercised by unit and integration
/// tests without spinning up a Tauri runtime.
///
/// Diagnostic `details` intentionally omit the absolute `path` and raw OS
/// error message to keep PII out of the UI-facing error payload. Only a
/// stable `source` marker and a coarse `kind` tag are reported.
pub fn ensure_dir_writable(path: &Path) -> Result<(), AppError> {
    if let Err(cause) = fs::create_dir_all(path) {
        return Err(AppError::local_storage_unavailable(
            "Impossible de créer ou d'accéder au dossier de stockage local.",
            "Vérifie que le dossier existe et qu'il est accessible en écriture, puis relance.",
        )
        .with_details(serde_json::json!({
            "source": "create_dir_all",
            "kind": io_error_kind_tag(&cause),
        })));
    }

    let probe = path.join(sentinel_filename());

    // `create_new(true)` atomically refuses to truncate a pre-existing file,
    // which is the only guarantee we rely on to avoid clobbering user data.
    // We do NOT short-circuit on `probe.exists()` — a pre-existing file
    // would say nothing about *our* ability to write, so we must still fail
    // closed rather than pretend the directory is writable.
    let open_result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe);

    match open_result {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(cause) => Err(AppError::local_storage_unavailable(
            "Le dossier de stockage local de Rustory n'accepte pas l'écriture.",
            "Vérifie les permissions en écriture du dossier puis relance l'application.",
        )
        .with_details(serde_json::json!({
            "source": "write_probe",
            "kind": io_error_kind_tag(&cause),
        }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;
    use tempfile::TempDir;

    #[test]
    fn creates_missing_directory_and_probes_write() {
        let tmp = TempDir::new().expect("tempdir");
        let target = tmp.path().join("nested").join("storage");

        ensure_dir_writable(&target).expect("should succeed");

        assert!(target.is_dir());

        let leftover_probes: Vec<_> = fs::read_dir(&target)
            .expect("read_dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(WRITE_CHECK_PREFIX)
            })
            .collect();
        assert!(
            leftover_probes.is_empty(),
            "write probes must be cleaned up: {leftover_probes:?}"
        );
    }

    #[test]
    fn preserves_existing_user_file_with_colliding_name() {
        // Regression guard: even if a user's file happens to shadow our
        // sentinel name, the probe must not overwrite it.
        let tmp = TempDir::new().expect("tempdir");
        let target = tmp.path().join("storage");
        fs::create_dir_all(&target).expect("prepare target");
        let intruder = target.join(format!("{WRITE_CHECK_PREFIX}intruder"));
        fs::write(&intruder, b"user-content").expect("seed user file");

        // `ensure_dir_writable` generates its own unique sentinel, so this
        // call should never touch `intruder` regardless.
        ensure_dir_writable(&target).expect("must still succeed");

        assert_eq!(
            fs::read(&intruder).expect("read intruder"),
            b"user-content",
            "pre-existing file must never be overwritten"
        );
    }

    #[test]
    fn sentinel_filenames_are_unique_without_clock() {
        // Two back-to-back calls must differ even if the OS clock happens
        // to return the same nanosecond value (or is broken).
        let a = sentinel_filename();
        let b = sentinel_filename();
        assert_ne!(a, b, "probe filenames must be unique per call: {a} vs {b}");
    }

    #[test]
    fn fails_when_parent_is_a_regular_file() {
        let tmp = TempDir::new().expect("tempdir");
        let file_path = tmp.path().join("not-a-dir");
        fs::write(&file_path, b"blocker").expect("write blocker");

        let target = file_path.join("storage");
        let err = ensure_dir_writable(&target).expect_err("must fail");

        assert_eq!(err.code, AppErrorCode::LocalStorageUnavailable);
        let details = err.details.as_ref().unwrap();
        assert_eq!(details["source"], "create_dir_all");
        // PII guardrail: the raw path MUST NOT leak into wire details.
        assert!(
            details.get("path").is_none(),
            "details must not expose the absolute path: {details}"
        );
        assert!(
            details.get("cause").is_none(),
            "details must not expose raw OS error messages: {details}"
        );
    }
}
