//! Managed on-disk store of node source media (images and audio).
//!
//! Layout under the Tauri `app_data_dir`:
//!
//! ```text
//! {app_data_dir}/node-media/                ← promoted media, content-addressed
//! {app_data_dir}/node-media/.staging/       ← transient acquisition area
//! {app_data_dir}/node-media/<hash>.<ext>    ← promoted bytes (named by content)
//! ```
//!
//! The store mirrors `catalog_covers` (sniff magic bytes, hard byte ceiling,
//! safe read) and `import_store` (staging → promote so the promoting
//! `rename(2)` stays on one filesystem and is atomic). The frontend NEVER owns
//! the bytes: it only ever sees a preview produced by a Rust read. Source media
//! are stored AS-IS — no decoding, no transcoding, zero new dependency. A file
//! is recognized strictly by its magic bytes, never by its extension.

use std::path::{Path, PathBuf};

use crate::domain::story::content_checksum_bytes;

use super::app_paths::ensure_dir_writable;

/// Directory (under `app_data_dir`) holding the promoted node media.
pub const NODE_MEDIA_DIR_NAME: &str = "node-media";

/// Hidden staging sub-directory (under `node-media/`) for in-flight copies.
pub const NODE_MEDIA_STAGING_DIR_NAME: &str = ".staging";

/// Hard ceiling on a single stored media file. Generous enough for a short
/// node narration or an illustration, small enough to stop a hostile/oversized
/// file from filling the disk. Applies to both images and audio.
pub const MAX_MEDIA_BYTES: usize = 32 * 1024 * 1024;

/// The two media kinds a node can carry. Stable wire strings (`image`/`audio`)
/// matching the `assets.media_type` CHECK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Audio,
}

impl MediaKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
        }
    }
}

/// What a successful magic-byte sniff resolves to. `format` is the stable wire
/// string matching the `assets.media_format` CHECK; `ext` names the stored file
/// on disk; `mime` is what a preview hands back to the webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniffedMedia {
    pub kind: MediaKind,
    pub format: &'static str,
    pub ext: &'static str,
    pub mime: &'static str,
}

/// A promoted media file: its content hash (the on-disk identity), kind,
/// format, exact byte size and stored file name (`<hash>.<ext>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMedia {
    pub content_hash: String,
    pub kind: MediaKind,
    pub format: &'static str,
    pub byte_size: u64,
    pub file_name: String,
}

/// Typed store failure. A VALIDATION failure (the file is not a supported,
/// readable, in-bound media) is a real block surfaced at the slot; a TRANSPORT
/// failure is a media-store I/O degradation. The application layer maps the two
/// onto `MEDIA_INVALID` vs `MEDIA_PROCESSING_FAILED`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeMediaError {
    /// The bytes are not a recognized supported media format.
    UnsupportedFormat,
    /// The bytes exceed [`MAX_MEDIA_BYTES`].
    Oversize,
    /// A transport stage failed (`staging` / `promote` / `read` / `invalid_name`).
    Transport(&'static str),
}

impl NodeMediaError {
    /// Stable, PII-free stage tag for `details.stage`.
    pub const fn stage(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat => "unsupported_format",
            Self::Oversize => "oversize",
            Self::Transport(stage) => stage,
        }
    }

    /// `true` when the failure is a user-correctable validation block (refuse
    /// the file at the slot), `false` for a media-store transport degradation.
    pub const fn is_validation(&self) -> bool {
        matches!(self, Self::UnsupportedFormat | Self::Oversize)
    }
}

/// Resolve `{app_data_dir}/node-media`. Pure — no creation.
pub fn resolve_node_media_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(NODE_MEDIA_DIR_NAME)
}

/// Resolve `{app_data_dir}/node-media/.staging`. Pure — no creation.
pub fn resolve_node_media_staging_dir(app_data_dir: &Path) -> PathBuf {
    resolve_node_media_dir(app_data_dir).join(NODE_MEDIA_STAGING_DIR_NAME)
}

/// Lazily create `node-media/` and `node-media/.staging/`, probing
/// writability. Returns `(media_dir, staging_dir)`.
pub fn ensure_node_media_store(app_data_dir: &Path) -> Result<(PathBuf, PathBuf), NodeMediaError> {
    let media = resolve_node_media_dir(app_data_dir);
    ensure_dir_writable(&media).map_err(|_| NodeMediaError::Transport("staging"))?;
    let staging = resolve_node_media_staging_dir(app_data_dir);
    ensure_dir_writable(&staging).map_err(|_| NodeMediaError::Transport("staging"))?;
    Ok((media, staging))
}

/// Recognize a media by its magic bytes → [`SniffedMedia`]. Returns `None` for
/// anything that is not a supported image (PNG / JPEG) or audio (MP3 / WAV /
/// OGG), so an unsupported file is refused rather than stored.
pub fn sniff_media(bytes: &[u8]) -> Option<SniffedMedia> {
    // Images.
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(SniffedMedia {
            kind: MediaKind::Image,
            format: "png",
            ext: "png",
            mime: "image/png",
        });
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(SniffedMedia {
            kind: MediaKind::Image,
            format: "jpeg",
            ext: "jpg",
            mime: "image/jpeg",
        });
    }
    // Audio. WAV is a RIFF container tagged `WAVE`; OGG starts with `OggS`.
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return Some(SniffedMedia {
            kind: MediaKind::Audio,
            format: "wav",
            ext: "wav",
            mime: "audio/wav",
        });
    }
    if bytes.starts_with(b"OggS") {
        return Some(SniffedMedia {
            kind: MediaKind::Audio,
            format: "ogg",
            ext: "ogg",
            mime: "audio/ogg",
        });
    }
    // MP3: either an ID3v2 tag (`ID3`) or a raw MPEG audio frame sync
    // (`0xFF` then `0xEx`/`0xFx` — MPEG-1/2/2.5 layer III frame headers).
    if bytes.starts_with(b"ID3") {
        return Some(SniffedMedia {
            kind: MediaKind::Audio,
            format: "mp3",
            ext: "mp3",
            mime: "audio/mpeg",
        });
    }
    // Raw MPEG audio frame: the 11 sync bits PLUS a self-consistent frame
    // header (version / layer / bitrate / sampling not in their reserved
    // values). Validating the whole header — not just the sync bits — keeps an
    // arbitrary `0xFF 0xEx ..` binary from being accepted as `mp3`, honouring
    // the "recognized by magic bytes" promise without decoding.
    if bytes.len() >= 3 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        let version = (bytes[1] >> 3) & 0x03; // 01 = reserved
        let layer = (bytes[1] >> 1) & 0x03; // 00 = reserved
        let bitrate = (bytes[2] >> 4) & 0x0F; // 1111 = invalid (free=0000 allowed)
        let sampling = (bytes[2] >> 2) & 0x03; // 11 = reserved
        if version != 0b01 && layer != 0b00 && bitrate != 0b1111 && sampling != 0b11 {
            return Some(SniffedMedia {
                kind: MediaKind::Audio,
                format: "mp3",
                ext: "mp3",
                mime: "audio/mpeg",
            });
        }
    }
    None
}

/// MIME for a stored extension (used by the preview read).
fn mime_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "wav" => Some("audio/wav"),
        "ogg" => Some("audio/ogg"),
        "mp3" => Some("audio/mpeg"),
        _ => None,
    }
}

/// Validate, content-address and PROMOTE `bytes` into the store. Returns the
/// promoted [`StoredMedia`]. The bytes are written to a staging temp file first,
/// then atomically `rename`d to `<hash>.<ext>` so a crash mid-write never leaves
/// a half-written promoted file. Re-storing identical bytes is idempotent (the
/// content hash names the same file).
pub fn store_media(
    media_dir: &Path,
    staging_dir: &Path,
    bytes: &[u8],
) -> Result<StoredMedia, NodeMediaError> {
    if bytes.len() > MAX_MEDIA_BYTES {
        return Err(NodeMediaError::Oversize);
    }
    let sniffed = sniff_media(bytes).ok_or(NodeMediaError::UnsupportedFormat)?;
    let content_hash = content_checksum_bytes(bytes);
    let file_name = format!("{content_hash}.{}", sniffed.ext);
    let promoted = media_dir.join(&file_name);

    // Idempotent fast path: identical bytes already promoted.
    if !promoted.exists() {
        // Stage under a unique temp name in the same filesystem, then promote
        // by rename so a reader never sees a partially-written file.
        let staged = staging_dir.join(format!("{content_hash}.tmp"));
        std::fs::write(&staged, bytes).map_err(|_| NodeMediaError::Transport("staging"))?;
        if let Err(err) = std::fs::rename(&staged, &promoted) {
            // Promotion failed: best-effort clean the staged temp and report.
            let _ = std::fs::remove_file(&staged);
            let _ = err;
            return Err(NodeMediaError::Transport("promote"));
        }
    }

    Ok(StoredMedia {
        content_hash,
        kind: sniffed.kind,
        format: sniffed.format,
        byte_size: bytes.len() as u64,
        file_name,
    })
}

/// Read a promoted media by its stored file name, returning `(bytes, mime)` for
/// a preview. The name MUST be a bare `<hash>.<ext>` (no path separators, no
/// `..`) so a crafted `assets.file_name` can never escape the store directory.
/// The on-disk bytes are re-sniffed (defense in depth) and the result MIME must
/// agree with the extension.
pub fn read_media(
    media_dir: &Path,
    file_name: &str,
) -> Result<(Vec<u8>, &'static str), NodeMediaError> {
    if !is_safe_media_name(file_name) {
        return Err(NodeMediaError::Transport("invalid_name"));
    }
    let ext = file_name.rsplit('.').next().unwrap_or("");
    let mime = mime_for_ext(ext).ok_or(NodeMediaError::Transport("invalid_name"))?;
    let bytes = read_file_bounded(&media_dir.join(file_name))?;
    match sniff_media(&bytes) {
        Some(sniffed) if sniffed.mime == mime => Ok((bytes, mime)),
        _ => Err(NodeMediaError::Transport("read")),
    }
}

fn read_file_bounded(path: &Path) -> Result<Vec<u8>, NodeMediaError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| NodeMediaError::Transport("read"))?;
    let mut buf = Vec::new();
    file.take(MAX_MEDIA_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|_| NodeMediaError::Transport("read"))?;
    if buf.len() > MAX_MEDIA_BYTES {
        return Err(NodeMediaError::Oversize);
    }
    Ok(buf)
}

/// A bare `<stem>.<ext>` with a recognized media extension and no path parts.
fn is_safe_media_name(name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    name.rsplit('.')
        .next()
        .and_then(mime_for_ext)
        .is_some_and(|_| name.len() > 4)
}

/// Sweep stale staging temporaries left by a crash mid-acquisition. Best-effort
/// by contract — a non-existent directory is a no-op and an unremovable stray
/// entry is skipped. Mirrors the import-store boot sweep.
pub fn sweep_node_media_staging(staging_dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(staging_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0];
    const OGG: &[u8] = b"OggS\0\0\0\0\0\0\0\0";
    const MP3_ID3: &[u8] = b"ID3\x03\x00\x00\x00";
    const MP3_SYNC: &[u8] = &[0xFF, 0xFB, 0x90, 0x00];

    fn wav() -> Vec<u8> {
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&[0, 0, 0, 0]);
        v.extend_from_slice(b"WAVE");
        v.extend_from_slice(&[0; 8]);
        v
    }

    fn store(tmp: &TempDir) -> (PathBuf, PathBuf) {
        ensure_node_media_store(tmp.path()).expect("store")
    }

    #[test]
    fn sniffs_every_supported_format() {
        assert_eq!(sniff_media(PNG).unwrap().format, "png");
        assert_eq!(sniff_media(JPEG).unwrap().format, "jpeg");
        assert_eq!(sniff_media(&wav()).unwrap().format, "wav");
        assert_eq!(sniff_media(OGG).unwrap().format, "ogg");
        assert_eq!(sniff_media(MP3_ID3).unwrap().format, "mp3");
        assert_eq!(sniff_media(MP3_SYNC).unwrap().format, "mp3");
        assert_eq!(sniff_media(PNG).unwrap().kind, MediaKind::Image);
        assert_eq!(sniff_media(OGG).unwrap().kind, MediaKind::Audio);
    }

    #[test]
    fn refuses_an_invalid_mpeg_frame_header() {
        // 0xFF 0xFB passes sync + version + layer, but a 1111 bitrate is invalid.
        assert!(sniff_media(&[0xFF, 0xFB, 0xF0, 0x00]).is_none());
        // 0xFF 0xE0 has a reserved layer (00) — not a real MPEG frame.
        assert!(sniff_media(&[0xFF, 0xE0, 0x00, 0x00]).is_none());
        // An arbitrary 0xFF 0xEx.. binary is no longer accepted as mp3.
        assert!(sniff_media(&[0xFF, 0xE2, 0xFF, 0xFF]).is_none());
        // The real frame-sync sample still sniffs as mp3.
        assert_eq!(sniff_media(MP3_SYNC).unwrap().format, "mp3");
    }

    #[test]
    fn refuses_unsupported_bytes() {
        assert!(sniff_media(b"<html>not media</html>").is_none());
        assert!(sniff_media(b"GIF89a").is_none(), "GIF is not in the set");
        // An extension lie cannot help: the bytes are what is sniffed.
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        assert_eq!(
            store_media(&media, &staging, b"PK\x03\x04 zip not media"),
            Err(NodeMediaError::UnsupportedFormat)
        );
    }

    #[test]
    fn stores_and_reads_back_a_png_round_trip() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        let stored = store_media(&media, &staging, PNG).expect("store");
        assert_eq!(stored.kind, MediaKind::Image);
        assert_eq!(stored.format, "png");
        assert_eq!(stored.byte_size, PNG.len() as u64);
        assert_eq!(stored.file_name, format!("{}.png", stored.content_hash));
        let (bytes, mime) = read_media(&media, &stored.file_name).expect("read");
        assert_eq!(bytes, PNG);
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn content_addressing_is_deterministic_and_idempotent() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        let a = store_media(&media, &staging, &wav()).expect("a");
        let b = store_media(&media, &staging, &wav()).expect("b");
        assert_eq!(a.content_hash, b.content_hash);
        assert_eq!(a.file_name, b.file_name);
    }

    #[test]
    fn rejects_oversize() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        let mut huge = PNG.to_vec();
        huge.resize(MAX_MEDIA_BYTES + 1, 0);
        assert_eq!(
            store_media(&media, &staging, &huge),
            Err(NodeMediaError::Oversize)
        );
    }

    #[test]
    fn read_rejects_path_traversal_and_unknown_ext() {
        let tmp = TempDir::new().unwrap();
        let (media, _staging) = store(&tmp);
        for bad in [
            "../secret.png",
            "a/b.png",
            "..",
            "noext",
            "evil.txt",
            "x.gif",
        ] {
            assert!(read_media(&media, bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn sweep_removes_staging_temporaries() {
        let tmp = TempDir::new().unwrap();
        let (_media, staging) = store(&tmp);
        std::fs::write(staging.join("orphan.tmp"), b"stale").unwrap();
        sweep_node_media_staging(&staging);
        assert!(std::fs::read_dir(&staging).unwrap().next().is_none());
    }

    #[test]
    fn error_stage_and_classification() {
        assert_eq!(
            NodeMediaError::UnsupportedFormat.stage(),
            "unsupported_format"
        );
        assert_eq!(NodeMediaError::Oversize.stage(), "oversize");
        assert_eq!(NodeMediaError::Transport("promote").stage(), "promote");
        assert!(NodeMediaError::UnsupportedFormat.is_validation());
        assert!(NodeMediaError::Oversize.is_validation());
        assert!(!NodeMediaError::Transport("read").is_validation());
    }
}
