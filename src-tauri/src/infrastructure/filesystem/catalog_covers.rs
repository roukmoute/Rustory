//! Local cache of official-catalog cover images.
//!
//! Layout under the Tauri `app_data_dir`:
//!
//! ```text
//! {app_data_dir}/catalog-covers/<uuid>.<ext>
//! ```
//!
//! Populated ONLY during the explicit catalog refresh (the covers are
//! downloaded there, never on a device read — offline-first). Disposable:
//! the whole directory is cleared before a refresh re-fills it. Covers are
//! read back from disk on an explicit cover request — a LOCAL read, no
//! network. Downloaded bytes are UNTRUSTED: only recognized image magic
//! bytes are accepted, the file name is a fixed `<uuid>.<ext>` (the UUID is
//! validated canonical upstream, so no path traversal), and reads are
//! bounded.

use std::path::{Path, PathBuf};

use crate::domain::shared::AppError;

use super::app_paths::ensure_dir_writable;

/// Directory (under `app_data_dir`) holding the cached cover images.
pub const CATALOG_COVERS_DIR_NAME: &str = "catalog-covers";

/// Hard ceiling on a single cached cover. Real Lunii covers are well under
/// this; the bound stops a hostile/oversized download from filling the disk.
pub const MAX_COVER_BYTES: usize = 4 * 1024 * 1024;

/// Resolve `{app_data_dir}/catalog-covers`. Pure — no creation.
pub fn resolve_catalog_covers_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CATALOG_COVERS_DIR_NAME)
}

/// Lazily create the cover cache directory, probing writability.
pub fn ensure_catalog_covers_dir(app_data_dir: &Path) -> Result<PathBuf, AppError> {
    let dir = resolve_catalog_covers_dir(app_data_dir);
    ensure_dir_writable(&dir)?;
    Ok(dir)
}

/// Remove every cached cover (the disposable cache is wiped before a
/// refresh re-fills it). Best-effort: a non-existent directory is a no-op,
/// and an unremovable stray entry is skipped rather than fatal.
pub fn clear_catalog_covers(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Persist cover `bytes` for `uuid`, returning the stored file name
/// (`<uuid>.<ext>`). Rejects payloads that are not a recognized image or
/// exceed [`MAX_COVER_BYTES`]: an untrusted download is never written blind.
pub fn write_catalog_cover(dir: &Path, uuid: &str, bytes: &[u8]) -> Result<String, AppError> {
    if bytes.len() > MAX_COVER_BYTES {
        return Err(cover_error("oversize"));
    }
    let ext = sniff_image(bytes)
        .ok_or_else(|| cover_error("not_an_image"))?
        .0;
    let file_name = format!("{uuid}.{ext}");
    std::fs::write(dir.join(&file_name), bytes).map_err(|_| cover_error("write"))?;
    Ok(file_name)
}

/// Read a cached cover by its stored file name, returning `(bytes, mime)`.
/// The name MUST be a bare `<stem>.<ext>` (no path separators, no `..`) so a
/// crafted `pack_metadata.thumbnail` can never escape the cache directory.
pub fn read_catalog_cover(
    dir: &Path,
    file_name: &str,
) -> Result<(Vec<u8>, &'static str), AppError> {
    if !is_safe_cover_name(file_name) {
        return Err(cover_error("invalid_name"));
    }
    let ext = file_name.rsplit('.').next().unwrap_or("");
    let mime = mime_for_ext(ext).ok_or_else(|| cover_error("invalid_name"))?;
    let bytes = read_file_bounded(&dir.join(file_name))?;
    // Re-validate the on-disk bytes are still a real image (defense in depth).
    match sniff_image(&bytes) {
        Some((_, sniffed_mime)) if sniffed_mime == mime => Ok((bytes, mime)),
        _ => Err(cover_error("corrupt")),
    }
}

fn read_file_bounded(path: &Path) -> Result<Vec<u8>, AppError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| cover_error("read"))?;
    let mut buf = Vec::new();
    file.take(MAX_COVER_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|_| cover_error("read"))?;
    if buf.len() > MAX_COVER_BYTES {
        return Err(cover_error("oversize"));
    }
    Ok(buf)
}

/// A bare `<stem>.<ext>` with a recognized image extension and no path parts.
fn is_safe_cover_name(name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    name.rsplit('.')
        .next()
        .and_then(mime_for_ext)
        .is_some_and(|_| name.len() > 4)
}

/// Recognize an image by its magic bytes → `(extension, mime)`. Returns
/// `None` for anything that is not a supported image, so a non-image
/// download is refused rather than cached.
fn sniff_image(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(("png", "image/png"));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("jpg", "image/jpeg"));
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(("webp", "image/webp"));
    }
    if bytes.starts_with(b"GIF8") {
        return Some(("gif", "image/gif"));
    }
    None
}

fn mime_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn cover_error(stage: &'static str) -> AppError {
    AppError::official_catalog_unavailable(
        "Couverture indisponible.",
        "Récupère à nouveau le catalogue officiel puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "cover",
        "stage": stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const UUID: &str = "11111111-1111-1111-1111-1111111111aa";
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    #[test]
    fn writes_and_reads_a_png_cover_round_trip() {
        let tmp = TempDir::new().expect("tmp");
        let dir = ensure_catalog_covers_dir(tmp.path()).expect("dir");
        let name = write_catalog_cover(&dir, UUID, PNG).expect("write");
        assert_eq!(name, format!("{UUID}.png"));
        let (bytes, mime) = read_catalog_cover(&dir, &name).expect("read");
        assert_eq!(bytes, PNG);
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn refuses_a_non_image_download() {
        let tmp = TempDir::new().expect("tmp");
        let dir = ensure_catalog_covers_dir(tmp.path()).expect("dir");
        let err = write_catalog_cover(&dir, UUID, b"<html>nope</html>").expect_err("not image");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "not_an_image");
    }

    #[test]
    fn refuses_an_oversize_download() {
        let tmp = TempDir::new().expect("tmp");
        let dir = ensure_catalog_covers_dir(tmp.path()).expect("dir");
        let mut huge = PNG.to_vec();
        huge.resize(MAX_COVER_BYTES + 1, 0);
        let err = write_catalog_cover(&dir, UUID, &huge).expect_err("oversize");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "oversize");
    }

    #[test]
    fn read_rejects_path_traversal_names() {
        let tmp = TempDir::new().expect("tmp");
        let dir = ensure_catalog_covers_dir(tmp.path()).expect("dir");
        for bad in ["../secret.png", "a/b.png", "..", "noext", "evil.txt"] {
            assert!(
                read_catalog_cover(&dir, bad).is_err(),
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn clear_removes_cached_covers() {
        let tmp = TempDir::new().expect("tmp");
        let dir = ensure_catalog_covers_dir(tmp.path()).expect("dir");
        write_catalog_cover(&dir, UUID, PNG).expect("write");
        clear_catalog_covers(&dir);
        assert!(read_catalog_cover(&dir, &format!("{UUID}.png")).is_err());
    }
}
