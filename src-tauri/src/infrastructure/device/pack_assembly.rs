//! Assemble the complete on-device file set of a Lunii **V3** pack
//! (`.content/<SHORTID>/…`) from a transcoded pack + the device `.md`.
//!
//! Combines the two proven layers — the pure [`transcode_pack`] (binary index
//! files) and the [`cipher`](super::cipher) (AES-128-CBC first-512) — plus the
//! asset copy and the forged `bt`, producing the exact bytes the device
//! expects. PURE: returns `(relative path, bytes)` pairs; the on-volume write
//! (staging + atomic promotion + `.pi` append) is a separate infrastructure
//! step that consumes this.
//!
//! Cleartext on device: `ni`, `nm`, `bt`. Ciphered (first 512 bytes):
//! `li`, `ri`, `si`, and every `rf/000/*` / `sf/000/*` asset. Validated
//! byte-for-byte against a real device by
//! `assembles_a_real_pack_matching_the_device` — every produced file equals the
//! device's actual file.

use crate::domain::device::pack_transcode::{device_asset_basename, TranscodedPack};

use super::cipher::{v3_cipher_in_place, v3_forge_bt, v3_story_key_iv};

/// One file of an assembled pack, path relative to `.content/<SHORTID>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledFile {
    pub rel_path: String,
    pub bytes: Vec<u8>,
}

/// Why a pack could not be assembled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssembleError {
    /// The device `.md` is not a readable v7 metadata (no key/IV/SNU).
    UnreadableDeviceMetadata,
    /// An asset referenced by the pack has no bytes from the resolver.
    MissingAsset(String),
}

/// Assemble every file of `.content/<SHORTID>/` for a V3 device whose `.md` is
/// `md`. `resolve_asset(filename)` returns the VERBATIM device-format bytes of
/// an asset (community `.zip` assets are already BMP-4bit-RLE4-320x240 /
/// MP3-mono-44100 — copied as-is, only the first 512 bytes get ciphered).
///
/// The caller writes each returned file under the pack folder, then appends the
/// pack UUID to the device `.pi` (files first, index second — the writer's
/// existing discipline).
pub fn assemble_v3_pack(
    transcoded: &TranscodedPack,
    md: &[u8],
    resolve_asset: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<AssembledFile>, AssembleError> {
    let (key, iv) = v3_story_key_iv(md).ok_or(AssembleError::UnreadableDeviceMetadata)?;
    let bt = v3_forge_bt(md).ok_or(AssembleError::UnreadableDeviceMetadata)?;

    let mut files = Vec::new();

    // Cleartext index + markers.
    files.push(AssembledFile {
        rel_path: "ni".to_string(),
        bytes: transcoded.ni.clone(),
    });
    files.push(AssembledFile {
        rel_path: "bt".to_string(),
        bytes: bt.to_vec(),
    });
    if transcoded.night_mode {
        files.push(AssembledFile {
            rel_path: "nm".to_string(),
            bytes: Vec::new(),
        });
    }

    // Ciphered index files (first 512 bytes).
    for (name, plain) in [
        ("li", &transcoded.li),
        ("ri", &transcoded.ri),
        ("si", &transcoded.si),
    ] {
        let mut bytes = plain.clone();
        v3_cipher_in_place(&mut bytes, &key, &iv);
        files.push(AssembledFile {
            rel_path: name.to_string(),
            bytes,
        });
    }

    // Ciphered assets, named by their device basename under rf/000 (images) and
    // sf/000 (audio), in index order.
    for (dir, assets) in [("rf", &transcoded.images), ("sf", &transcoded.audios)] {
        for filename in assets {
            let mut bytes = resolve_asset(filename)
                .ok_or_else(|| AssembleError::MissingAsset(filename.clone()))?;
            v3_cipher_in_place(&mut bytes, &key, &iv);
            files.push(AssembledFile {
                rel_path: format!("{dir}/000/{}", device_asset_basename(filename)),
                bytes,
            });
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::pack_transcode::transcode_pack;
    use std::collections::HashMap;

    fn tiny_md() -> Vec<u8> {
        // A minimal v7-shaped .md: SNU hex at 0x1A, key at 0x40, iv at 0x50.
        let mut md = vec![0u8; 0x60];
        md[0x1A..0x28].copy_from_slice(b"40025040005071");
        md[0x40..0x50].copy_from_slice(&[1u8; 16]);
        md[0x50..0x60].copy_from_slice(&[2u8; 16]);
        md
    }

    #[test]
    fn assembles_the_expected_file_set_with_cleartext_and_ciphered_layers() {
        let json = r#"{
            "version":1,"nightModeAvailable":true,
            "stageNodes":[{"uuid":"s0","squareOne":true,"image":"aaaaaaaaaaaaaaaa1234abcd.bmp",
                "audio":"bbbbbbbbbbbbbbbb5678ef01.mp3","okTransition":null,"homeTransition":null,
                "controlSettings":{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}],
            "actionNodes":[]
        }"#;
        let pack: crate::domain::device::StudioStoryPack = serde_json::from_str(json).unwrap();
        let transcoded = transcode_pack(&pack).unwrap();

        let mut assets: HashMap<String, Vec<u8>> = HashMap::new();
        // 600-byte assets so the first 512 get ciphered, tail left as-is.
        assets.insert("aaaaaaaaaaaaaaaa1234abcd.bmp".into(), vec![0xAB; 600]);
        assets.insert("bbbbbbbbbbbbbbbb5678ef01.mp3".into(), vec![0xCD; 600]);
        let resolve = |f: &str| assets.get(f).cloned();

        let files = assemble_v3_pack(&transcoded, &tiny_md(), &resolve).expect("assemble");
        let by_path: HashMap<&str, &Vec<u8>> = files
            .iter()
            .map(|f| (f.rel_path.as_str(), &f.bytes))
            .collect();

        // Cleartext files present and untouched.
        assert_eq!(
            by_path.get("ni").unwrap().as_slice(),
            transcoded.ni.as_slice()
        );
        assert_eq!(by_path.get("bt").unwrap().len(), 32);
        assert!(by_path.contains_key("nm")); // night mode → empty marker
        assert!(by_path.get("nm").unwrap().is_empty());
        // Asset paths use the device basename (last 8 hex, upper) under 000/.
        assert!(by_path.contains_key("rf/000/1234ABCD"));
        assert!(by_path.contains_key("sf/000/5678EF01"));
        // The asset's first 512 bytes are ciphered (changed), the tail is not.
        let img = by_path.get("rf/000/1234ABCD").unwrap();
        assert_eq!(img.len(), 600);
        assert_ne!(&img[..16], &[0xAB; 16]);
        assert_eq!(&img[512..], &[0xAB; 88]);
    }

    #[test]
    fn a_missing_asset_fails_closed() {
        let json = r#"{"version":1,"nightModeAvailable":false,
            "stageNodes":[{"uuid":"s0","squareOne":true,"image":"missing0000abcd1234.bmp","audio":null,
                "okTransition":null,"homeTransition":null,
                "controlSettings":{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}],
            "actionNodes":[]}"#;
        let pack: crate::domain::device::StudioStoryPack = serde_json::from_str(json).unwrap();
        let transcoded = transcode_pack(&pack).unwrap();
        let resolve = |_: &str| None;
        assert_eq!(
            assemble_v3_pack(&transcoded, &tiny_md(), &resolve),
            Err(AssembleError::MissingAsset(
                "missing0000abcd1234.bmp".into()
            ))
        );
    }

    /// Ground truth: assemble the full pack from a real STUdio `story.json` + its
    /// `.zip` assets + the device `.md`, and assert EVERY produced file equals the
    /// device's actual `.content/<SHORTID>/` file, byte-for-byte. Env:
    /// RUSTORY_TEST_STORYJSON, RUSTORY_TEST_ASSETS (dir), RUSTORY_TEST_MD,
    /// RUSTORY_TEST_CONTENT (the device `.content/<SHORTID>` dir).
    #[test]
    #[ignore = "manual: set RUSTORY_TEST_STORYJSON/_ASSETS/_MD/_CONTENT"]
    fn assembles_a_real_pack_matching_the_device() {
        use std::path::PathBuf;
        let json =
            std::fs::read_to_string(std::env::var("RUSTORY_TEST_STORYJSON").unwrap()).unwrap();
        let pack: crate::domain::device::StudioStoryPack = serde_json::from_str(&json).unwrap();
        let transcoded = transcode_pack(&pack).unwrap();
        let md = std::fs::read(std::env::var("RUSTORY_TEST_MD").unwrap()).unwrap();
        let assets_dir = PathBuf::from(std::env::var("RUSTORY_TEST_ASSETS").unwrap());
        let content_dir = PathBuf::from(std::env::var("RUSTORY_TEST_CONTENT").unwrap());
        let resolve = |f: &str| std::fs::read(assets_dir.join(f)).ok();

        let files = assemble_v3_pack(&transcoded, &md, &resolve).expect("assemble");
        let mut checked = 0usize;
        for f in &files {
            let device_path = content_dir.join(&f.rel_path);
            let device_bytes = std::fs::read(&device_path)
                .unwrap_or_else(|_| panic!("device file missing: {}", f.rel_path));
            assert_eq!(
                f.bytes, device_bytes,
                "[assembly-smoke] mismatch on {}",
                f.rel_path
            );
            checked += 1;
        }
        eprintln!("[assembly-smoke] {checked} files all match the device byte-for-byte ✓");
    }
}
