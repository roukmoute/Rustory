//! Managed on-disk store of the ORIGINAL source `.zip` archives of imported
//! packs — the device-format material (`story.json` + BMP-RLE4/MP3 assets)
//! that the canonical import path discards but the V3 send engine needs.
//!
//! Layout under the Tauri `app_data_dir`:
//!
//! ```text
//! {app_data_dir}/source-archives/                 ← retained archives, one per story
//! {app_data_dir}/source-archives/.staging/        ← transient copy area
//! {app_data_dir}/source-archives/<story_id>.zip   ← the committed source archive
//! ```
//!
//! The staging directory lives INSIDE `source-archives/` so the promotion
//! `rename(2)` stays on one filesystem (atomic). A retained archive lets
//! "Envoyer vers la Lunii" send an imported story to a V3 WITHOUT re-picking
//! the file: the transfer feeds this `.zip` to the proven transcode → cipher
//! (keyed on the TARGET `.md`) → write engine. Path resolution is pure (no
//! I/O) so tests target a TempDir; creation is lazy via
//! [`ensure_source_archive_store`].

use std::path::{Path, PathBuf};

use crate::domain::shared::AppError;

use super::app_paths::ensure_dir_writable;

/// Directory (under `app_data_dir`) holding the retained source archives.
pub const SOURCE_ARCHIVES_DIR_NAME: &str = "source-archives";

/// Hidden staging sub-directory (under `source-archives/`) for in-flight copies.
pub const SOURCE_ARCHIVES_STAGING_DIR_NAME: &str = ".staging";

/// Resolve `{app_data_dir}/source-archives`. Pure — no creation.
pub fn resolve_source_archives_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SOURCE_ARCHIVES_DIR_NAME)
}

/// Resolve `{app_data_dir}/source-archives/.staging`. Pure — no creation.
pub fn resolve_source_archives_staging_dir(app_data_dir: &Path) -> PathBuf {
    resolve_source_archives_dir(app_data_dir).join(SOURCE_ARCHIVES_STAGING_DIR_NAME)
}

/// Resolve the retained archive path of one story. Pure. The `story_id` is
/// always a Rust-generated UUIDv7 — never user input, never a device-supplied
/// name — so no path traversal is possible here.
pub fn resolve_source_archive_path(app_data_dir: &Path, story_id: &str) -> PathBuf {
    resolve_source_archives_dir(app_data_dir).join(format!("{story_id}.zip"))
}

/// Lazily create `source-archives/` and its `.staging/`, probing writability.
/// Returns `(archives_dir, staging_dir)`.
pub fn ensure_source_archive_store(app_data_dir: &Path) -> Result<(PathBuf, PathBuf), AppError> {
    let archives = resolve_source_archives_dir(app_data_dir);
    ensure_dir_writable(&archives)?;
    let staging = resolve_source_archives_staging_dir(app_data_dir);
    ensure_dir_writable(&staging)?;
    Ok((archives, staging))
}

/// Copy `source` (a source `.zip` on disk) into the store for `story_id`,
/// atomically: stage a copy inside `.staging/` then `rename(2)` it onto the
/// canonical `<story_id>.zip`. Best-effort staging cleanup on failure. The
/// promotion overwrites any prior archive for the story (a re-import replaces).
pub fn retain_source_archive(
    app_data_dir: &Path,
    story_id: &str,
    source: &Path,
) -> Result<PathBuf, AppError> {
    let (_, staging) = ensure_source_archive_store(app_data_dir)?;
    let staged = staging.join(format!("{story_id}.zip.part"));
    // A previous crash could have left a stale staged file — remove it first.
    let _ = std::fs::remove_file(&staged);
    std::fs::copy(source, &staged).map_err(|_| retain_error("stage_copy"))?;
    let promoted = resolve_source_archive_path(app_data_dir, story_id);
    match std::fs::rename(&staged, &promoted) {
        Ok(()) => Ok(promoted),
        Err(_) => {
            let _ = std::fs::remove_file(&staged);
            Err(retain_error("promote"))
        }
    }
}

/// Remove the retained archive of a story, if any. Idempotent — an absent
/// archive is a no-op success (a story that carried no retained source, or an
/// already-swept one). Used by the story-delete compensation/cleanup.
pub fn remove_source_archive(app_data_dir: &Path, story_id: &str) {
    let _ = std::fs::remove_file(resolve_source_archive_path(app_data_dir, story_id));
}

fn retain_error(stage: &'static str) -> AppError {
    AppError::import_failed(
        "Import impossible: le pack source n'a pas pu être conservé.",
        "Vérifie l'espace disque de ton dossier utilisateur puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "source_archive_retain", "stage": stage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    const STORY_ID: &str = "0197a5d0-0000-7000-8000-000000000000";

    #[test]
    fn resolves_paths_under_app_data_dir() {
        let base = Path::new("/data");
        assert_eq!(
            resolve_source_archives_dir(base),
            Path::new("/data/source-archives")
        );
        assert_eq!(
            resolve_source_archives_staging_dir(base),
            Path::new("/data/source-archives/.staging")
        );
        assert_eq!(
            resolve_source_archive_path(base, STORY_ID),
            Path::new("/data/source-archives/0197a5d0-0000-7000-8000-000000000000.zip")
        );
    }

    #[test]
    fn ensure_creates_both_directories_idempotently() {
        let tmp = TempDir::new().expect("tempdir");
        let (archives, staging) = ensure_source_archive_store(tmp.path()).expect("ensure");
        assert!(archives.is_dir());
        assert!(staging.is_dir() && staging.starts_with(&archives));
        ensure_source_archive_store(tmp.path()).expect("idempotent");
    }

    fn write_zip(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).expect("create");
        f.write_all(bytes).expect("write");
        p
    }

    #[test]
    fn retain_promotes_the_archive_atomically_and_leaves_no_staging_residue() {
        let tmp = TempDir::new().expect("tempdir");
        let src_dir = TempDir::new().expect("src");
        let src = write_zip(src_dir.path(), "pack.zip", b"PK\x03\x04payload");

        let promoted = retain_source_archive(tmp.path(), STORY_ID, &src).expect("retain");
        assert_eq!(promoted, resolve_source_archive_path(tmp.path(), STORY_ID));
        assert_eq!(
            std::fs::read(&promoted).expect("read"),
            b"PK\x03\x04payload"
        );
        // No residue in staging.
        let staging = resolve_source_archives_staging_dir(tmp.path());
        let residue: Vec<_> = std::fs::read_dir(&staging)
            .expect("readdir")
            .filter_map(Result::ok)
            .collect();
        assert!(residue.is_empty(), "staging must be clean after promote");
    }

    #[test]
    fn retain_replaces_a_prior_archive_for_the_same_story() {
        let tmp = TempDir::new().expect("tempdir");
        let src_dir = TempDir::new().expect("src");
        let first = write_zip(src_dir.path(), "a.zip", b"first");
        retain_source_archive(tmp.path(), STORY_ID, &first).expect("first");
        let second = write_zip(src_dir.path(), "b.zip", b"second-longer");
        retain_source_archive(tmp.path(), STORY_ID, &second).expect("second");
        assert_eq!(
            std::fs::read(resolve_source_archive_path(tmp.path(), STORY_ID)).expect("read"),
            b"second-longer"
        );
    }

    #[test]
    fn remove_is_idempotent() {
        let tmp = TempDir::new().expect("tempdir");
        // Absent → no-op.
        remove_source_archive(tmp.path(), STORY_ID);
        let src_dir = TempDir::new().expect("src");
        let src = write_zip(src_dir.path(), "p.zip", b"bytes");
        retain_source_archive(tmp.path(), STORY_ID, &src).expect("retain");
        assert!(resolve_source_archive_path(tmp.path(), STORY_ID).is_file());
        remove_source_archive(tmp.path(), STORY_ID);
        assert!(!resolve_source_archive_path(tmp.path(), STORY_ID).exists());
        // Second remove → still no-op.
        remove_source_archive(tmp.path(), STORY_ID);
    }

    #[test]
    fn retain_fails_cleanly_when_the_source_is_missing() {
        let tmp = TempDir::new().expect("tempdir");
        let err = retain_source_archive(tmp.path(), STORY_ID, Path::new("/nonexistent.zip"))
            .expect_err("missing source");
        assert_eq!(
            serde_json::to_value(&err).unwrap()["details"]["stage"],
            "stage_copy"
        );
    }
}
