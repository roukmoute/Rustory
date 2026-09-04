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
//! the bytes: it only ever sees a preview produced by a Rust read. A file is
//! recognized strictly by its magic bytes, never by its extension.
//!
//! IMAGES are TRANSCODED on store to the Lunii display format — decoded, fit
//! within 320×240 preserving aspect ratio, re-encoded as PNG — so an added
//! photo is never kept raw (a 10 MB source becomes a small PNG) and a
//! community-pack BMP image or a Radio France WebP enclosure becomes a usable
//! PNG. AUDIO is stored as-is.

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

/// Hard ceiling for MEDIA DOWNLOADED BY A WEB IMPORT. Web podcast pages can
/// link episodes much longer than a local node narration, so the web
/// acquisition path uses this wider ceiling while the local-attach flow keeps
/// [`MAX_MEDIA_BYTES`]. The read-back path admits it too (a stored 100 MB m4a
/// must stay previewable).
pub const WEB_MAX_MEDIA_BYTES: usize = 128 * 1024 * 1024;

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
    /// The bytes exceed the ceiling in force for the flow that stored them
    /// ([`MAX_MEDIA_BYTES`] local-attach / [`WEB_MAX_MEDIA_BYTES`] web import).
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
/// anything that is not a supported image (PNG / JPEG / BMP / WebP) or audio
/// (MP3 / WAV / OGG), so an unsupported file is refused rather than stored.
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
    // BMP (`BM`): the image format community Lunii/STUdio packs store their
    // illustrations in. Recognized as an image so a pack's images are RETAINED
    // (not discarded); `store_media` transcodes every image to a PNG, so `bmp`
    // is a transient input format that never reaches the `assets` table.
    if bytes.starts_with(b"BM") {
        return Some(SniffedMedia {
            kind: MediaKind::Image,
            format: "bmp",
            ext: "bmp",
            mime: "image/bmp",
        });
    }
    // WebP is a RIFF container tagged `WEBP`. In particular, Radio France can
    // advertise an enclosure as `image/jpeg` while serving WebP bytes, so the
    // magic bytes — not the feed MIME or URL suffix — remain authoritative.
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(SniffedMedia {
            kind: MediaKind::Image,
            format: "webp",
            ext: "webp",
            mime: "image/webp",
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
    // M4A (MP4 audio) is an ISOBMFF container: first box `ftyp` with an
    // `M4A ` brand. Radio France podcast pages link their episodes as m4a, so
    // the web import path must recognize the format the pages actually serve.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && &bytes[8..12] == b"M4A " {
        return Some(SniffedMedia {
            kind: MediaKind::Audio,
            format: "m4a",
            ext: "m4a",
            mime: "audio/mp4",
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
        "m4a" => Some("audio/mp4"),
        _ => None,
    }
}

/// Validate, content-address and PROMOTE `bytes` into the store, refusing any
/// payload strictly above `cap`. Returns the promoted [`StoredMedia`]. The
/// bytes are written to a staging temp file first, then atomically `rename`d
/// to `<hash>.<ext>` so a crash mid-write never leaves a half-written promoted
/// file. Re-storing identical bytes is idempotent (the content hash names the
/// same file).
pub fn store_media_capped(
    media_dir: &Path,
    staging_dir: &Path,
    bytes: &[u8],
    cap: usize,
) -> Result<StoredMedia, NodeMediaError> {
    if bytes.len() > cap {
        return Err(NodeMediaError::Oversize);
    }
    let sniffed = sniff_media(bytes).ok_or(NodeMediaError::UnsupportedFormat)?;

    // Images are TRANSCODED to the Lunii display PNG (≤320×240, aspect
    // preserved): a raw photo shrinks to a small PNG and a pack's BMP becomes
    // a usable PNG. Audio is stored verbatim. The stored bytes (never the
    // source) are what the content hash and `assets` row describe.
    let (stored_bytes, format, ext): (std::borrow::Cow<'_, [u8]>, &'static str, &'static str) =
        match sniffed.kind {
            MediaKind::Image => (
                std::borrow::Cow::Owned(transcode_image_to_display_png(bytes)?),
                "png",
                "png",
            ),
            MediaKind::Audio => (
                std::borrow::Cow::Borrowed(bytes),
                sniffed.format,
                sniffed.ext,
            ),
        };

    let content_hash = content_checksum_bytes(&stored_bytes);
    let file_name = format!("{content_hash}.{ext}");
    let promoted = media_dir.join(&file_name);

    // Idempotent fast path: identical bytes already promoted.
    if !promoted.exists() {
        // Stage under a unique temp name in the same filesystem, then promote
        // by rename so a reader never sees a partially-written file.
        let staged = staging_dir.join(format!("{content_hash}.tmp"));
        std::fs::write(&staged, &stored_bytes).map_err(|_| NodeMediaError::Transport("staging"))?;
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
        format,
        byte_size: stored_bytes.len() as u64,
        file_name,
    })
}

/// [`store_media_capped`] with the local-attach ceiling [`MAX_MEDIA_BYTES`]:
/// the classic attach/replace flow never moves to the wider web ceiling.
pub fn store_media(
    media_dir: &Path,
    staging_dir: &Path,
    bytes: &[u8],
) -> Result<StoredMedia, NodeMediaError> {
    store_media_capped(media_dir, staging_dir, bytes, MAX_MEDIA_BYTES)
}

/// Target of the image transcode: the Lunii display is 320×240. Images are
/// fit WITHIN this box preserving aspect ratio (never stretched), then
/// re-encoded as PNG.
pub const DISPLAY_IMAGE_WIDTH: u32 = 320;
pub const DISPLAY_IMAGE_HEIGHT: u32 = 240;

/// Decode any recognized image (PNG / JPEG / BMP), resize it to fit within
/// the Lunii display (320×240, aspect preserved), and re-encode it as PNG.
/// A source that does not decode is [`NodeMediaError::UnsupportedFormat`] —
/// the same verdict the sniffer gives a foreign format.
pub fn transcode_image_to_display_png(bytes: &[u8]) -> Result<Vec<u8>, NodeMediaError> {
    use image::ImageFormat;
    use std::io::Cursor;

    let decoded = image::load_from_memory(bytes).map_err(|_| NodeMediaError::UnsupportedFormat)?;
    // `resize` scales to fit WITHIN (w, h) preserving the aspect ratio — no
    // distortion, no cropping. A smaller source is scaled up to the frame.
    let resized = decoded.resize(
        DISPLAY_IMAGE_WIDTH,
        DISPLAY_IMAGE_HEIGHT,
        image::imageops::FilterType::Lanczos3,
    );
    let mut out = Vec::new();
    resized
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|_| NodeMediaError::Transport("encode"))?;
    Ok(out)
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
    // The store admits the WIDER web ceiling (a 128 MB episode is legal to
    // store), so the read-back bound must not reject what the store accepted.
    file.take(WEB_MAX_MEDIA_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|_| NodeMediaError::Transport("read"))?;
    if buf.len() > WEB_MAX_MEDIA_BYTES {
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

    /// A REAL, decodable image (the store transcodes images, so a fake magic
    /// header no longer suffices). `write_to` a genuine encoder.
    fn real_image(w: u32, h: u32, fmt: image::ImageFormat) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([12, 34, 56, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), fmt)
            .expect("encode image");
        out
    }

    fn png_image(w: u32, h: u32) -> Vec<u8> {
        real_image(w, h, image::ImageFormat::Png)
    }

    fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
        image::load_from_memory(bytes)
            .map(|i| (i.width(), i.height()))
            .expect("decode stored image")
    }

    fn store(tmp: &TempDir) -> (PathBuf, PathBuf) {
        ensure_node_media_store(tmp.path()).expect("store")
    }

    #[test]
    fn sniffs_every_supported_format() {
        assert_eq!(sniff_media(PNG).unwrap().format, "png");
        assert_eq!(sniff_media(JPEG).unwrap().format, "jpeg");
        assert_eq!(
            sniff_media(&real_image(1, 1, image::ImageFormat::WebP))
                .unwrap()
                .format,
            "webp"
        );
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
    fn mpeg_frame_header_reserved_fields_fire_one_by_one() {
        // version = 01 (reserved) alone, with a valid layer / bitrate /
        // sampling: a truncated 3-byte header is a complete frame sync.
        assert!(sniff_media(&[0xFF, 0xEE, 0x90]).is_none());
        // layer = 00 (reserved) alone, with a valid version / bitrate /
        // sampling: distinct from the existing free-bitrate (0x00) case.
        assert!(sniff_media(&[0xFF, 0xE0, 0x90]).is_none());
        // sampling = 11 (reserved) alone, with a valid version / layer /
        // bitrate.
        assert!(sniff_media(&[0xFF, 0xFB, 0x9C]).is_none());
        // A 3-byte header is already a complete frame sync: shortening the
        // length guard to `>= 4` would drop every real 3-byte frame.
        assert_eq!(sniff_media(&[0xFF, 0xFB, 0x90]).unwrap().format, "mp3");
        // Clear protection bit with a valid layer (0xFA: version 3, layer 1,
        // protection clear) — the only vector that separates the LAYER shift
        // `(bytes[1] >> 1)` from a `<< 1` flip: with `<<` the layer reads 00
        // (reserved) and this complete header would be rejected.
        assert_eq!(sniff_media(&[0xFF, 0xFA, 0x90]).unwrap().format, "mp3");
        // Short buffers: no panic, no mp3 (a relaxed length guard would
        // index bytes[2] on a 2-byte buffer or accept one).
        assert!(sniff_media(&[0xFF, 0xE3]).is_none());
        assert!(sniff_media(&[0xFF]).is_none());
        // Missing the first sync byte: not a frame at all.
        assert!(sniff_media(&[0xFE, 0xFB, 0x90]).is_none());
        // Incomplete sync nibble (0xC0 & 0xE0 = 0xC0): not a frame.
        assert!(sniff_media(&[0xFF, 0xC0, 0x90]).is_none());
    }

    #[test]
    fn mime_for_ext_maps_every_stored_extension() {
        assert_eq!(mime_for_ext("png"), Some("image/png"));
        assert_eq!(mime_for_ext("jpg"), Some("image/jpeg"));
        assert_eq!(mime_for_ext("jpeg"), Some("image/jpeg"));
        assert_eq!(mime_for_ext("wav"), Some("audio/wav"));
        assert_eq!(mime_for_ext("ogg"), Some("audio/ogg"));
        assert_eq!(mime_for_ext("mp3"), Some("audio/mpeg"));
        assert_eq!(mime_for_ext("m4a"), Some("audio/mp4"));
        assert_eq!(mime_for_ext("gif"), None, "gif is not a stored format");
        assert_eq!(mime_for_ext(""), None);
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
    fn stores_and_reads_back_an_image_round_trip() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        // A 400×300 source is LARGER than the 320×240 display frame.
        let stored = store_media(&media, &staging, &png_image(400, 300)).expect("store");
        assert_eq!(stored.kind, MediaKind::Image);
        assert_eq!(stored.format, "png");
        assert_eq!(stored.file_name, format!("{}.png", stored.content_hash));
        let (bytes, mime) = read_media(&media, &stored.file_name).expect("read");
        assert_eq!(mime, "image/png");
        // Stored bytes are the TRANSCODED display PNG: decodable + ≤320×240.
        let (w, h) = png_dimensions(&bytes);
        assert!(
            w <= 320 && h <= 240,
            "resized within the display frame: {w}x{h}"
        );
        // 4:3 source preserved (no stretch): fills the 320×240 box.
        assert_eq!((w, h), (320, 240));
    }

    #[test]
    fn transcodes_a_bmp_source_into_a_display_png() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        // A community-pack BMP: recognized, transcoded to PNG, resized.
        let stored = store_media(
            &media,
            &staging,
            &real_image(640, 480, image::ImageFormat::Bmp),
        )
        .expect("store bmp");
        assert_eq!(stored.format, "png", "bmp is transcoded to png");
        let (bytes, mime) = read_media(&media, &stored.file_name).expect("read");
        assert_eq!(mime, "image/png");
        let (w, h) = png_dimensions(&bytes);
        assert!(w <= 320 && h <= 240, "{w}x{h}");
    }

    #[test]
    fn transcodes_a_webp_source_into_a_display_png() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        let stored = store_media(
            &media,
            &staging,
            &real_image(640, 480, image::ImageFormat::WebP),
        )
        .expect("store webp");
        assert_eq!(stored.format, "png", "webp is transcoded to png");
        let (bytes, mime) = read_media(&media, &stored.file_name).expect("read");
        assert_eq!(mime, "image/png");
        assert_eq!(png_dimensions(&bytes), (320, 240));
    }

    #[test]
    fn refuses_an_undecodable_image() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        // PNG magic but no valid image body — the transcode refuses it.
        assert_eq!(
            store_media(&media, &staging, PNG),
            Err(NodeMediaError::UnsupportedFormat)
        );
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
    fn read_media_refuses_bytes_whose_sniff_disagrees_with_the_extension() {
        // Defense in depth: a name that passes `is_safe_media_name` is still
        // re-checked against its content — the sniffed MIME must agree with
        // the extension, otherwise a mismatched file would be served as a
        // preview under the wrong MIME.
        let tmp = TempDir::new().unwrap();
        let (media, _staging) = store(&tmp);
        let name = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef.png";
        std::fs::write(media.join(name), JPEG).expect("store jpeg bytes under a png name");
        assert!(
            read_media(&media, name).is_err(),
            "jpeg bytes under a .png name must fail the sniff-vs-extension recheck"
        );
    }

    #[test]
    fn is_safe_media_name_rejects_each_unsafe_shape_one_by_one() {
        // Each guard must fire on its own: a traversal name is rejected by
        // the NAME check, not accidentally by a missing file downstream.
        assert!(!is_safe_media_name(""), "empty name");
        assert!(!is_safe_media_name("a/b.png"), "slash is a path separator");
        assert!(
            !is_safe_media_name("a\\b.png"),
            "backslash is a path separator"
        );
        assert!(
            !is_safe_media_name("a..b.png"),
            "dot-dot can climb out of the store"
        );
        // Length floor: an extension-only name (len 4) has no stem.
        assert!(
            !is_safe_media_name(".png"),
            "extension-only name has no stem"
        );
        // And the shape the store actually uses stays valid.
        assert!(
            is_safe_media_name(
                "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef.png"
            ),
            "64-hex stem + valid ext is the stored shape"
        );
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

    /// A fake but correctly-shaped M4A box: 4-byte size + `ftyp` + `M4A `
    /// brand. Audio is stored verbatim, so a decodable payload is NOT required.
    fn m4a() -> Vec<u8> {
        let mut v = vec![0u8, 0, 0, 0];
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(b"M4A ");
        v.extend_from_slice(&[0; 8]);
        v
    }

    #[test]
    fn sniffs_m4a_audio() {
        assert_eq!(
            sniff_media(&m4a()),
            Some(SniffedMedia {
                kind: MediaKind::Audio,
                format: "m4a",
                ext: "m4a",
                mime: "audio/mp4",
            })
        );
    }

    #[test]
    fn stores_and_reads_back_m4a_round_trip() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        let source = m4a();
        let stored = store_media(&media, &staging, &source).expect("store m4a");
        assert_eq!(stored.kind, MediaKind::Audio);
        assert_eq!(stored.format, "m4a");
        assert_eq!(stored.file_name, format!("{}.m4a", stored.content_hash));
        let (bytes, mime) = read_media(&media, &stored.file_name).expect("read m4a");
        assert_eq!(mime, "audio/mp4");
        assert_eq!(bytes, source);
    }

    #[test]
    fn store_media_capped_refuses_bytes_above_the_cap() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        let source = m4a(); // 20 bytes
        assert_eq!(
            store_media_capped(&media, &staging, &source, 16),
            Err(NodeMediaError::Oversize),
            "strictly above the cap must be refused"
        );
        let stored =
            store_media_capped(&media, &staging, &source, source.len()).expect("at cap is ok");
        assert_eq!(stored.format, "m4a");
    }

    #[test]
    fn store_media_capped_is_idempotent_for_identical_bytes() {
        let tmp = TempDir::new().unwrap();
        let (media, staging) = store(&tmp);
        let source = m4a();
        let a = store_media_capped(&media, &staging, &source, 1024).expect("a");
        let b = store_media_capped(&media, &staging, &source, 1024).expect("b");
        assert_eq!(a.content_hash, b.content_hash);
        assert_eq!(a.file_name, b.file_name);
    }

    #[test]
    fn web_media_cap_is_wider_than_the_classic_cap() {
        const _: () = assert!(
            WEB_MAX_MEDIA_BYTES > MAX_MEDIA_BYTES,
            "web acquisition must admit files the local-attach cap refuses"
        );
    }

    /// Cap boundary: a stored file of EXACTLY the web ceiling is read back
    /// in full — the read-back bound must not reject what the (wider) web
    /// store admitted.
    #[test]
    fn read_file_bounded_accepts_a_file_of_exactly_the_web_cap() {
        let tmp = TempDir::new().unwrap();
        let (media, _staging) = store(&tmp);
        let path = media.join("cap-boundary.wav");
        std::fs::write(&path, vec![b'x'; WEB_MAX_MEDIA_BYTES]).expect("write boundary file");
        let bytes =
            read_file_bounded(&path).expect("a file of exactly the web cap must be accepted");
        assert_eq!(bytes.len(), WEB_MAX_MEDIA_BYTES);
    }

    /// Cap boundary: a stored file ONE byte above the web ceiling is refused
    /// (Oversize) — the bounded read is the ground truth for the read-back.
    #[test]
    fn read_file_bounded_refuses_a_file_one_byte_above_the_web_cap() {
        let tmp = TempDir::new().unwrap();
        let (media, _staging) = store(&tmp);
        let path = media.join("cap-oversize.wav");
        std::fs::write(&path, vec![b'x'; WEB_MAX_MEDIA_BYTES + 1]).expect("write oversize file");
        let err = read_file_bounded(&path).expect_err("one byte above the web cap must be refused");
        assert!(matches!(err, NodeMediaError::Oversize));
    }
}
