//! Managed on-disk store of device-imported packs.
//!
//! Layout under the Tauri `app_data_dir`:
//!
//! ```text
//! {app_data_dir}/imports/                 ← promoted packs, one dir per story
//! {app_data_dir}/imports/.staging/        ← transient acquisition area
//! {app_data_dir}/imports/<story_id>/      ← committed pack bytes (manifest files)
//! ```
//!
//! The staging directory lives INSIDE `imports/` so the promotion
//! `rename(2)` is guaranteed to stay on one filesystem (atomic). Path
//! resolution is pure (no I/O) so tests can target a TempDir; creation is
//! lazy via [`ensure_import_store`].

use std::path::{Path, PathBuf};

use crate::domain::shared::AppError;

use super::app_paths::ensure_dir_writable;

/// Directory (under `app_data_dir`) holding the promoted imported packs.
pub const IMPORTS_DIR_NAME: &str = "imports";

/// Hidden staging sub-directory (under `imports/`) for in-flight copies.
pub const IMPORTS_STAGING_DIR_NAME: &str = ".staging";

/// Resolve `{app_data_dir}/imports`. Pure — no creation.
pub fn resolve_imports_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(IMPORTS_DIR_NAME)
}

/// Resolve `{app_data_dir}/imports/.staging`. Pure — no creation.
pub fn resolve_imports_staging_dir(app_data_dir: &Path) -> PathBuf {
    resolve_imports_dir(app_data_dir).join(IMPORTS_STAGING_DIR_NAME)
}

/// Resolve the promoted directory of one imported story. Pure. The
/// `story_id` is always a Rust-generated UUIDv7 — never user input, never
/// a device-supplied name — so no path traversal is possible here.
pub fn resolve_import_story_dir(app_data_dir: &Path, story_id: &str) -> PathBuf {
    resolve_imports_dir(app_data_dir).join(story_id)
}

/// Lazily create `imports/` and `imports/.staging/`, probing writability.
/// Returns `(imports_dir, staging_dir)`.
pub fn ensure_import_store(app_data_dir: &Path) -> Result<(PathBuf, PathBuf), AppError> {
    let imports = resolve_imports_dir(app_data_dir);
    ensure_dir_writable(&imports)?;
    let staging = resolve_imports_staging_dir(app_data_dir);
    ensure_dir_writable(&staging)?;
    Ok((imports, staging))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolves_paths_under_app_data_dir() {
        let base = Path::new("/data");
        assert_eq!(resolve_imports_dir(base), Path::new("/data/imports"));
        assert_eq!(
            resolve_imports_staging_dir(base),
            Path::new("/data/imports/.staging")
        );
        assert_eq!(
            resolve_import_story_dir(base, "0197-id"),
            Path::new("/data/imports/0197-id")
        );
    }

    #[test]
    fn ensure_import_store_creates_both_directories() {
        let tmp = TempDir::new().expect("tempdir");
        let (imports, staging) = ensure_import_store(tmp.path()).expect("ensure");
        assert!(imports.is_dir());
        assert!(staging.is_dir());
        assert!(
            staging.starts_with(&imports),
            "staging must live inside imports"
        );
    }

    #[test]
    fn ensure_import_store_is_idempotent() {
        let tmp = TempDir::new().expect("tempdir");
        ensure_import_store(tmp.path()).expect("first");
        ensure_import_store(tmp.path()).expect("second");
    }
}
