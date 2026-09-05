//! Converts pack assets to the LUNII DEVICE format at send time.
//!
//! Community `.zip` packs come in two shapes: some (exported from a device or
//! a prepared STUdio transfer) already carry DEVICE-READY assets — images as
//! BMP 320×240 4-bit BI_RLE4, audio as bare MP3 (MPEG-1 Layer III, 44100 Hz,
//! mono) — while others (raw STUdio library exports) carry PNG/JPEG images and
//! ID3-tagged MP3s. The device decodes NEITHER PNG nor JPEG: sending them
//! verbatim yields a blank/stale screen and an "Error SD Card" at play time.
//!
//! So the send pipeline normalizes every asset through this module:
//!
//! - [`to_device_image`] — an already-device-ready BMP passes VERBATIM
//!   (byte-for-byte, preserving the hardware-proven ground truth for such
//!   packs); anything else is decoded (PNG/JPEG/BMP), letterboxed onto a
//!   320×240 black canvas, quantized to ≤16 colors (median cut) and encoded
//!   as BMP BI_RLE4 with the EXACT header layout observed on a real device
//!   pack (offset 118, bottom-up, 16-entry BGRA palette with 0xFF alpha,
//!   `colorsUsed = 0`, `colorsImportant = 16`).
//! - [`to_device_audio`] — a bare, conformant MP3 passes VERBATIM; ID3v2
//!   headers and ID3v1 trailers are STRIPPED (lossless); anything else
//!   (m4a/AAC, stereo or 48 kHz MP3, WAV, Ogg — what podcast pages and the
//!   media store hold) is TRANSCODED to the device MP3 by
//!   [`audio_transcode`](super::audio_transcode); what cannot be decoded is
//!   REFUSED (fail closed: an unprovable file must never reach the device to
//!   fail there as an opaque SD error). The produced bytes are re-checked
//!   against the same conformance rule before they are handed out.

use image::GenericImageView;

/// Device screen geometry — the only image shape a Lunii V3 renders.
const DEVICE_IMAGE_WIDTH: u32 = 320;
const DEVICE_IMAGE_HEIGHT: u32 = 240;

/// BMP layout constants mirrored from a real device pack (see module doc).
const BMP_HEADER_BYTES: usize = 14 + 40 + 16 * 4; // file + DIB + 16 BGRA entries = 118
const BI_RLE4: u32 = 2;

/// Why an asset could not be made device-ready. The caller attaches the asset
/// identity (device basename) — this type carries only the closed reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetConvertError {
    /// The image could not be decoded (corrupt, or an unsupported codec).
    ImageUndecodable,
    /// The audio is neither a device-playable MP3 nor decodable into one
    /// (unrecognized container/codec, or a stream without audio).
    AudioUnsupported,
}

impl AssetConvertError {
    /// Stable diagnostic tag for logs / error details. Closed set.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::ImageUndecodable => "image_undecodable",
            Self::AudioUnsupported => "audio_unsupported",
        }
    }
}

// ===== Images =====

/// True iff the bytes already ARE a device-format image: BMP, 4 bpp,
/// BI_RLE4, 320×240 (bottom-up or top-down). Such bytes pass verbatim.
fn is_device_bmp(bytes: &[u8]) -> bool {
    if bytes.len() < BMP_HEADER_BYTES || &bytes[0..2] != b"BM" {
        return false;
    }
    let width = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let height = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    let bpp = u16::from_le_bytes([bytes[28], bytes[29]]);
    let compression = u32::from_le_bytes([bytes[30], bytes[31], bytes[32], bytes[33]]);
    width == DEVICE_IMAGE_WIDTH as i32
        && height.unsigned_abs() == DEVICE_IMAGE_HEIGHT
        && bpp == 4
        && compression == BI_RLE4
}

/// Convert arbitrary image bytes to the device BMP format (verbatim when the
/// input already is one — see the module doc for why that invariant matters).
pub fn to_device_image(bytes: &[u8]) -> Result<Vec<u8>, AssetConvertError> {
    if is_device_bmp(bytes) {
        return Ok(bytes.to_vec());
    }
    let decoded =
        image::load_from_memory(bytes).map_err(|_| AssetConvertError::ImageUndecodable)?;

    // Letterbox onto the exact device canvas: scale to fit (never stretch),
    // composite over black (the device background), center.
    let (w, h) = decoded.dimensions();
    if w == 0 || h == 0 {
        return Err(AssetConvertError::ImageUndecodable);
    }
    let resized = decoded.resize(
        DEVICE_IMAGE_WIDTH,
        DEVICE_IMAGE_HEIGHT,
        image::imageops::FilterType::Lanczos3,
    );
    let rgba = resized.to_rgba8();
    let (rw, rh) = rgba.dimensions();
    let off_x = (DEVICE_IMAGE_WIDTH - rw) / 2;
    let off_y = (DEVICE_IMAGE_HEIGHT - rh) / 2;
    let mut canvas = vec![[0u8; 3]; (DEVICE_IMAGE_WIDTH * DEVICE_IMAGE_HEIGHT) as usize];
    for (x, y, px) in rgba.enumerate_pixels() {
        let [r, g, b, a] = px.0;
        // Composite over black: out = channel × alpha.
        let alpha = a as u16;
        let dst = ((y + off_y) * DEVICE_IMAGE_WIDTH + (x + off_x)) as usize;
        canvas[dst] = [
            ((r as u16 * alpha) / 255) as u8,
            ((g as u16 * alpha) / 255) as u8,
            ((b as u16 * alpha) / 255) as u8,
        ];
    }

    let (palette, indices) = quantize_to_16(&canvas);
    let rle = encode_rle4(
        &indices,
        DEVICE_IMAGE_WIDTH as usize,
        DEVICE_IMAGE_HEIGHT as usize,
    );
    Ok(assemble_bmp(&palette, &rle))
}

/// Median-cut quantization of RGB pixels to ≤16 colors. Returns the palette
/// and one palette index per input pixel. Deterministic (stable sorts, fixed
/// tie-breaking) so tests and re-sends are reproducible.
fn quantize_to_16(pixels: &[[u8; 3]]) -> (Vec<[u8; 3]>, Vec<u8>) {
    // Histogram: exact colors → counts (a story illustration rarely exceeds
    // a few thousand distinct colors; the map stays small).
    let mut counts: std::collections::BTreeMap<[u8; 3], u32> = std::collections::BTreeMap::new();
    for px in pixels {
        *counts.entry(*px).or_insert(0) += 1;
    }
    let mut boxes: Vec<Vec<([u8; 3], u32)>> = vec![counts.into_iter().collect()];

    while boxes.len() < 16 {
        // Split the box with the largest single-channel range; stop when no
        // box has more than one distinct color.
        let mut widest: Option<(usize, usize, u8)> = None; // (box, channel, range)
        for (bi, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for ch in 0..3 {
                let min = b.iter().map(|(c, _)| c[ch]).min().unwrap_or(0);
                let max = b.iter().map(|(c, _)| c[ch]).max().unwrap_or(0);
                let range = max - min;
                if widest.map(|(_, _, r)| range > r).unwrap_or(range > 0) {
                    widest = Some((bi, ch, range));
                }
            }
        }
        let Some((bi, ch, _)) = widest else { break };
        let mut b = boxes.swap_remove(bi);
        b.sort_by_key(|(c, _)| (c[ch], *c));
        // Split at the pixel-count median so both halves weigh similarly.
        let total: u64 = b.iter().map(|(_, n)| *n as u64).sum();
        let mut acc = 0u64;
        let mut split = 1;
        for (i, (_, n)) in b.iter().enumerate() {
            acc += *n as u64;
            if acc * 2 >= total && i + 1 < b.len() {
                split = i + 1;
                break;
            }
        }
        let right = b.split_off(split);
        boxes.push(b);
        boxes.push(right);
    }

    // Palette entry = pixel-count-weighted average of each box.
    let mut palette: Vec<[u8; 3]> = boxes
        .iter()
        .map(|b| {
            let total: u64 = b.iter().map(|(_, n)| *n as u64).sum::<u64>().max(1);
            let mut sum = [0u64; 3];
            for (c, n) in b {
                for ch in 0..3 {
                    sum[ch] += c[ch] as u64 * *n as u64;
                }
            }
            [
                (sum[0] / total) as u8,
                (sum[1] / total) as u8,
                (sum[2] / total) as u8,
            ]
        })
        .collect();
    palette.sort_unstable();
    palette.dedup();

    // Map every pixel to its nearest palette entry (squared distance).
    let nearest = |px: [u8; 3]| -> u8 {
        let mut best = 0usize;
        let mut best_d = u32::MAX;
        for (i, p) in palette.iter().enumerate() {
            let d = p
                .iter()
                .zip(px.iter())
                .map(|(a, b)| {
                    let diff = *a as i32 - *b as i32;
                    (diff * diff) as u32
                })
                .sum();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best as u8
    };
    let indices: Vec<u8> = pixels.iter().map(|px| nearest(*px)).collect();
    (palette, indices)
}

/// RLE4-encode the indexed pixels (row-major, top-down in memory) into the
/// BOTTOM-UP run stream BMP expects. Encoded-mode runs only (`[count][ab]`
/// draws `count` pixels alternating the two nibbles) — always valid RLE4,
/// no absolute mode needed. Each row ends with `00 00`, the bitmap with
/// `00 01`.
fn encode_rle4(indices: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(indices.len() / 4);
    for y in (0..height).rev() {
        let row = &indices[y * width..(y + 1) * width];
        let mut i = 0;
        while i < row.len() {
            let a = row[i];
            let b = if i + 1 < row.len() { row[i + 1] } else { a };
            // Length of the alternating a,b,a,b… pattern starting at i.
            let mut len = 1usize;
            while i + len < row.len()
                && len < 255
                && row[i + len] == if len.is_multiple_of(2) { a } else { b }
            {
                len += 1;
            }
            let packed = if len == 1 { a << 4 } else { (a << 4) | b };
            out.push(len as u8);
            out.push(packed);
            i += len;
        }
        out.push(0);
        out.push(0); // end of line
    }
    // The final 00 00 becomes 00 01 (end of bitmap replaces the last EOL).
    let n = out.len();
    out[n - 1] = 1;
    out
}

/// Assemble the full BMP: headers mirrored byte-for-byte on the layout of a
/// real device pack image, palette padded to 16 BGRA entries (alpha 0xFF).
fn assemble_bmp(palette: &[[u8; 3]], rle: &[u8]) -> Vec<u8> {
    let file_size = BMP_HEADER_BYTES + rle.len();
    let mut out = Vec::with_capacity(file_size);
    // BITMAPFILEHEADER
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&(BMP_HEADER_BYTES as u32).to_le_bytes());
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(DEVICE_IMAGE_WIDTH as i32).to_le_bytes());
    out.extend_from_slice(&(DEVICE_IMAGE_HEIGHT as i32).to_le_bytes()); // bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&4u16.to_le_bytes()); // bpp
    out.extend_from_slice(&BI_RLE4.to_le_bytes());
    out.extend_from_slice(&(rle.len() as u32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes()); // x px/m
    out.extend_from_slice(&0i32.to_le_bytes()); // y px/m
    out.extend_from_slice(&0u32.to_le_bytes()); // colorsUsed (0 ⇒ 2^4)
    out.extend_from_slice(&16u32.to_le_bytes()); // colorsImportant
                                                 // 16 palette entries, BGRA with 0xFF alpha (the observed device layout).
    for i in 0..16 {
        let [r, g, b] = palette.get(i).copied().unwrap_or([0, 0, 0]);
        out.extend_from_slice(&[b, g, r, 0xFF]);
    }
    out.extend_from_slice(rle);
    out
}

// ===== Audio =====

/// True iff the bytes are ALREADY a device-playable MP3 (bare or under a
/// losslessly strippable ID3 wrapper) — the cheap header check the send
/// pipeline uses to plan its progress before any transcoding runs.
pub fn audio_is_device_ready(bytes: &[u8]) -> bool {
    conformant_mp3_body(bytes).is_some()
}

/// Convert arbitrary audio bytes to a device-playable bare MP3: verbatim when
/// already bare and conformant, ID3 wrappers stripped losslessly, anything
/// else transcoded (see the module doc); undecodable input is refused.
pub fn to_device_audio(bytes: &[u8]) -> Result<Vec<u8>, AssetConvertError> {
    if let Some(body) = conformant_mp3_body(bytes) {
        return Ok(body.to_vec());
    }
    let transcoded = super::audio_transcode::transcode_to_device_mp3(bytes)
        .map_err(|_| AssetConvertError::AudioUnsupported)?;
    // Defense in depth: the encoder's output must satisfy the very rule the
    // device needs, or it is not handed out.
    match conformant_mp3_body(&transcoded) {
        Some(body) if body.len() == transcoded.len() => Ok(transcoded),
        _ => Err(AssetConvertError::AudioUnsupported),
    }
}

/// The bare MP3 body of `bytes` when — after skipping an ID3v2 header and an
/// ID3v1 trailer — its first frame header is MPEG-1 Layer III, 44100 Hz,
/// mono (the device format observed on real packs and the format both STUdio
/// and Lunii.QT produce). `None` otherwise.
fn conformant_mp3_body(bytes: &[u8]) -> Option<&[u8]> {
    let mut start = 0usize;
    let mut end = bytes.len();

    // ID3v2 header: "ID3" + version(2) + flags(1) + syncsafe size(4); the
    // optional footer (flag bit 4) adds 10 trailing-of-header bytes.
    if bytes.len() >= 10 && &bytes[0..3] == b"ID3" {
        let size = ((bytes[6] as usize & 0x7F) << 21)
            | ((bytes[7] as usize & 0x7F) << 14)
            | ((bytes[8] as usize & 0x7F) << 7)
            | (bytes[9] as usize & 0x7F);
        let footer = if bytes[5] & 0x10 != 0 { 10 } else { 0 };
        start = (10 + size + footer).min(bytes.len());
    }
    // ID3v1 trailer: fixed 128 bytes starting with "TAG".
    if end >= start + 128 && &bytes[end - 128..end - 125] == b"TAG" {
        end -= 128;
    }
    let body = &bytes[start..end];

    let h = body.get(0..4)?;
    if h[0] != 0xFF || (h[1] & 0xE0) != 0xE0 {
        return None;
    }
    let version_bits = (h[1] >> 3) & 0b11; // 3 = MPEG-1
    let layer_bits = (h[1] >> 1) & 0b11; // 1 = Layer III
    let samplerate_bits = (h[2] >> 2) & 0b11; // 0 = 44100 for MPEG-1
    let channel_bits = (h[3] >> 6) & 0b11; // 3 = mono
    if version_bits != 3 || layer_bits != 1 || samplerate_bits != 0 || channel_bits != 3 {
        return None;
    }
    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic 44100 Hz mono MPEG-1 Layer III frame header followed by
    /// filler — enough for the header validation the converter performs.
    fn bare_mp3() -> Vec<u8> {
        let mut b = vec![0xFF, 0xFB, 0x90, 0xC0]; // MPEG1 LIII 128kbps 44100 mono
        b.extend_from_slice(&[0x11; 64]);
        b
    }

    fn id3v2_wrapped(inner: &[u8]) -> Vec<u8> {
        let mut b = b"ID3\x04\x00\x00\x00\x00\x00\x0A".to_vec(); // 10-byte tag body
        b.extend_from_slice(&[0u8; 10]);
        b.extend_from_slice(inner);
        b
    }

    // ===== audio =====

    #[test]
    fn bare_conformant_mp3_passes_verbatim() {
        let src = bare_mp3();
        assert_eq!(to_device_audio(&src).expect("ok"), src);
    }

    #[test]
    fn id3v2_header_and_id3v1_trailer_are_stripped_losslessly() {
        let inner = bare_mp3();
        let mut src = id3v2_wrapped(&inner);
        src.extend_from_slice(b"TAG");
        src.extend_from_slice(&[0u8; 125]);
        assert_eq!(to_device_audio(&src).expect("ok"), inner);
    }

    #[test]
    fn audio_that_is_neither_conformant_nor_decodable_is_refused() {
        // A stereo / 48 kHz frame HEADER on filler bytes is not a decodable
        // stream: no transcode can prove it, so it never reaches the device.
        let mut stereo = bare_mp3();
        stereo[3] = 0x00;
        assert_eq!(
            to_device_audio(&stereo).unwrap_err(),
            AssetConvertError::AudioUnsupported
        );
        let mut sr48 = bare_mp3();
        sr48[2] = 0x94;
        assert_eq!(
            to_device_audio(&sr48).unwrap_err(),
            AssetConvertError::AudioUnsupported
        );
        // Not an audio stream at all.
        assert_eq!(
            to_device_audio(b"OggS whatever").unwrap_err(),
            AssetConvertError::AudioUnsupported
        );
        assert!(!audio_is_device_ready(&stereo));
        assert!(!audio_is_device_ready(b"OggS whatever"));
    }

    #[test]
    fn a_decodable_non_conformant_audio_is_transcoded_to_the_device_format() {
        // A 48 kHz STEREO WAV — the shape of a podcast episode once decoded —
        // must come out as a bare mono 44100 Hz MPEG-1 Layer III stream.
        let wav = super::super::audio_transcode::test_support::wav_sine(48_000, 2, 660.0, 1.0, 0.5);
        assert!(!audio_is_device_ready(&wav));
        let mp3 = to_device_audio(&wav).expect("transcoded");
        assert!(audio_is_device_ready(&mp3));
        assert_eq!(&mp3[0..1], &[0xFF]);
        assert_eq!((mp3[3] >> 6) & 0b11, 3, "mono");
        assert_eq!((mp3[2] >> 2) & 0b11, 0, "44100 Hz");
        // Roughly 128 kbps · 1 s = 16 KB (± the Info frame and padding).
        assert!((12_000..22_000).contains(&mp3.len()), "len {}", mp3.len());
    }

    #[test]
    fn device_ready_audio_is_reported_as_such_even_under_id3() {
        assert!(audio_is_device_ready(&bare_mp3()));
        assert!(audio_is_device_ready(&id3v2_wrapped(&bare_mp3())));
    }

    // ===== images =====

    /// Encode an in-memory PNG for conversion-input tests.
    fn png_bytes(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("encode png");
        out.into_inner()
    }

    #[test]
    fn a_png_becomes_a_valid_320x240_rle4_bmp_the_image_crate_can_decode() {
        // A 4-color 320×240 PNG: quantization must preserve the exact colors.
        let png = png_bytes(320, 240, |x, y| match ((x / 80) + (y / 60)) % 4 {
            0 => [255, 0, 0, 255],
            1 => [0, 255, 0, 255],
            2 => [0, 0, 255, 255],
            _ => [255, 255, 255, 255],
        });
        let bmp = to_device_image(&png).expect("convert");

        // Header facts mirrored on the real device layout.
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(
            u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]),
            BMP_HEADER_BYTES as u32,
            "pixel data at offset 118"
        );
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 4, "4 bpp");
        assert_eq!(
            u32::from_le_bytes([bmp[30], bmp[31], bmp[32], bmp[33]]),
            BI_RLE4,
            "BI_RLE4 compression"
        );
        assert_eq!(
            u32::from_le_bytes([bmp[50], bmp[51], bmp[52], bmp[53]]),
            16,
            "colorsImportant = 16"
        );
        for i in 0..16 {
            assert_eq!(bmp[14 + 40 + i * 4 + 3], 0xFF, "palette alpha 0xFF");
        }

        // Round-trip: the image crate decodes RLE4 — pixels must survive.
        let decoded = image::load_from_memory(&bmp).expect("decode our bmp");
        assert_eq!(decoded.dimensions(), (320, 240));
        let rgba = decoded.to_rgba8();
        assert_eq!(rgba.get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert_eq!(rgba.get_pixel(100, 10).0, [0, 255, 0, 255]);
        assert_eq!(rgba.get_pixel(170, 10).0, [0, 0, 255, 255]);
    }

    #[test]
    fn a_non_device_geometry_image_is_letterboxed_onto_the_320x240_canvas() {
        // A 100×100 all-white PNG: letterboxed centered, black bars on the
        // sides (100×100 scaled to 240×240 within 320×240).
        let png = png_bytes(100, 100, |_, _| [255, 255, 255, 255]);
        let bmp = to_device_image(&png).expect("convert");
        let decoded = image::load_from_memory(&bmp).expect("decode");
        assert_eq!(decoded.dimensions(), (320, 240));
        let rgba = decoded.to_rgba8();
        assert_eq!(
            rgba.get_pixel(160, 120).0,
            [255, 255, 255, 255],
            "center white"
        );
        assert_eq!(rgba.get_pixel(5, 120).0, [0, 0, 0, 255], "left bar black");
        assert_eq!(
            rgba.get_pixel(314, 120).0,
            [0, 0, 0, 255],
            "right bar black"
        );
    }

    #[test]
    fn an_already_device_ready_bmp_passes_verbatim() {
        // Produce a device BMP with our own encoder, then feed it back: the
        // bytes must pass through UNTOUCHED (the ground-truth invariant for
        // packs that already carry device-ready assets).
        let png = png_bytes(320, 240, |_, _| [10, 20, 30, 255]);
        let bmp = to_device_image(&png).expect("first conversion");
        assert_eq!(to_device_image(&bmp).expect("verbatim"), bmp);
    }

    #[test]
    fn undecodable_image_bytes_are_refused() {
        assert_eq!(
            to_device_image(b"not an image at all").unwrap_err(),
            AssetConvertError::ImageUndecodable
        );
    }

    #[test]
    fn a_busy_image_still_quantizes_to_at_most_16_palette_indices() {
        // A gradient forces quantization (way more than 16 source colors).
        let png = png_bytes(320, 240, |x, y| {
            [(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]
        });
        let bmp = to_device_image(&png).expect("convert");
        let decoded = image::load_from_memory(&bmp).expect("decode");
        // ≤16 distinct colors in the decoded output.
        let mut distinct = std::collections::BTreeSet::new();
        for px in decoded.to_rgba8().pixels() {
            distinct.insert(px.0);
        }
        assert!(distinct.len() <= 16, "got {} colors", distinct.len());
    }
}
