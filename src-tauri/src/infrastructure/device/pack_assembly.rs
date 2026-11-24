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
//! Cleartext on device: `ni`, `nm`, `bt`. Ciphered: `li`, `ri`, `si`, and
//! every `rf/000/*` / `sf/000/*` asset — the first 512 bytes for files of at
//! least 512 bytes; a SHORTER file is zero-padded to the next AES-block
//! multiple and ciphered over its whole length (see
//! [`pad_short_ciphered_file`]). Validated byte-for-byte against a real device
//! by `assembles_a_real_pack_matching_the_device` — every produced file equals
//! the device's actual file.

use crate::domain::device::pack_transcode::TranscodedPack;

use super::cipher::{v3_cipher_in_place, v3_forge_bt, v3_story_key_iv, V3_CIPHER_REGION};

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
/// `md`. `resolve_asset(source_ref)` returns DEVICE-FORMAT bytes of an asset
/// (BMP-4bit-RLE4-320x240 / bare MP3 — the send layer normalizes them via
/// `asset_convert` before handing the resolver over); they are copied as-is,
/// then ciphered per the module-doc regime. Every returned file has a path
/// of its own: an asset is written under the device name the transcoder
/// gave it, which no other asset of its kind bears.
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
        pad_short_ciphered_file(&mut bytes);
        v3_cipher_in_place(&mut bytes, &key, &iv);
        files.push(AssembledFile {
            rel_path: name.to_string(),
            bytes,
        });
    }

    // Ciphered assets, each under its device name in rf/000 (images) and
    // sf/000 (audio), in index order. The name is the one the transcoder
    // settled — the one the `ri` / `si` entry carries, and no other asset's.
    for (dir, assets) in [("rf", &transcoded.images), ("sf", &transcoded.audios)] {
        for asset in assets {
            let mut bytes = resolve_asset(&asset.source_ref)
                .ok_or_else(|| AssembleError::MissingAsset(asset.source_ref.clone()))?;
            pad_short_ciphered_file(&mut bytes);
            v3_cipher_in_place(&mut bytes, &key, &iv);
            files.push(AssembledFile {
                rel_path: format!("{dir}/000/{}", asset.device_name),
                bytes,
            });
        }
    }

    Ok(files)
}

/// Zero-pad a TO-CIPHER file shorter than the 512-byte cipher region up to the
/// next AES-block multiple, so the WHOLE file gets ciphered.
///
/// The device deciphers whole blocks: a sub-512 file whose length is not a
/// 16-multiple would otherwise keep a CLEARTEXT tail that the device
/// "deciphers" into garbage — its last index entries turn to nonsense and the
/// story dies with an opaque "Error SD Card" the moment one is used. This is
/// exactly what Lunii.QT produces (proven byte-for-byte on a real pack: `li`
/// 68→80, `ri` 108→112, `si` 216→224, zero-padded, fully ciphered — our
/// padded-and-ciphered bytes reproduce its files EXACTLY). Trailing zeros are
/// invisible to the reader: `ni` carries the authoritative entry counts. Files
/// of 512 bytes or more keep the proven first-512-prefix regime untouched.
fn pad_short_ciphered_file(bytes: &mut Vec<u8>) {
    if bytes.len() < V3_CIPHER_REGION && !bytes.len().is_multiple_of(16) {
        let padded = bytes.len() + (16 - bytes.len() % 16);
        bytes.resize(padded, 0);
    }
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
    fn short_index_files_are_zero_padded_to_the_aes_block_and_fully_ciphered() {
        // A single-stage pack yields SHORT index files (ri = one 12-byte
        // entry, si = one, li = empty). Every ciphered file below the
        // 512-byte region must land ZERO-PADDED to a 16-multiple and be
        // ciphered over its WHOLE length — the Lunii.QT-proven layout; an
        // unaligned cleartext tail would "decipher" to garbage on the device
        // ("Error SD Card" when its entry is used).
        let json = r#"{
            "version":1,"nightModeAvailable":false,
            "stageNodes":[{"uuid":"s0","squareOne":true,"image":"aaaaaaaaaaaaaaaa1234abcd.bmp",
                "audio":"bbbbbbbbbbbbbbbb5678ef01.mp3","okTransition":null,"homeTransition":null,
                "controlSettings":{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}],
            "actionNodes":[]
        }"#;
        let pack: crate::domain::device::StudioStoryPack = serde_json::from_str(json).unwrap();
        let transcoded = transcode_pack(&pack).unwrap();
        assert_eq!(transcoded.ri.len(), 12, "one 12-byte ri entry");

        let mut assets: HashMap<String, Vec<u8>> = HashMap::new();
        // A SHORT asset too (a tiny file exercises the same padding rule).
        assets.insert("aaaaaaaaaaaaaaaa1234abcd.bmp".into(), vec![0xAB; 100]);
        assets.insert("bbbbbbbbbbbbbbbb5678ef01.mp3".into(), vec![0xCD; 600]);
        let resolve = |f: &str| assets.get(f).cloned();

        let files = assemble_v3_pack(&transcoded, &tiny_md(), &resolve).expect("assemble");
        let by_path: HashMap<&str, &Vec<u8>> = files
            .iter()
            .map(|f| (f.rel_path.as_str(), &f.bytes))
            .collect();

        // ri: 12 → padded to 16, and FULLY ciphered (a cleartext ri starts
        // with ASCII "000\" — the ciphered one must not).
        let ri = by_path.get("ri").unwrap();
        assert_eq!(ri.len(), 16);
        assert_ne!(&ri[..4], b"000\\");
        // Deciphering the whole file restores the entry + zero padding.
        let (key, iv) = v3_story_key_iv(&tiny_md()).unwrap();
        let mut plain = ri.to_vec();
        super::super::cipher::v3_decipher_in_place(&mut plain, &key, &iv);
        assert_eq!(&plain[..4], b"000\\");
        assert_eq!(&plain[12..], &[0u8; 4], "zero padding after the entry");

        // The short asset is padded to a 16-multiple and fully ciphered.
        let img = by_path.get("rf/000/1234ABCD").unwrap();
        assert_eq!(img.len(), 112, "100 → next 16-multiple");
        assert_ne!(&img[..16], &[0xAB; 16], "head ciphered");
        assert_ne!(
            &img[96..112],
            &[0xAB; 16],
            "tail ciphered too (no cleartext tail)"
        );

        // A ≥512 file keeps the PROVEN prefix regime: 600 stays 600,
        // tail past 512 cleartext.
        let snd = by_path.get("sf/000/5678EF01").unwrap();
        assert_eq!(snd.len(), 600);
        assert_eq!(&snd[512..], &[0xCD; 88]);
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

    /// Deterministic bytes standing for one asset: `len` bytes that depend on
    /// `seed` and on their position, so two assets never look alike and a
    /// misplaced cipher boundary shows.
    fn asset_bytes(seed: u8, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    /// The device format is proven against real hardware by the env-gated
    /// ground-truth tests; this one freezes the same bytes for every run. A
    /// pack whose assets each have a device name of their own — hash-named
    /// files, the way the tools that write the format name them — is
    /// assembled to exactly these files, in this order. A picture and a sound
    /// may share a name (they live in two folders), a repeated asset is
    /// written once, and every length class of the cipher is covered: a short
    /// unaligned file, a short aligned one, one byte under the ciphered
    /// region, exactly the region, and past it.
    #[test]
    fn the_files_of_a_pack_without_a_name_clash_are_frozen() {
        use sha2::{Digest, Sha256};

        const PICTURE_A: &str = "5b699d67ef92f738012e3db36f3b2837532195b9.bmp";
        const PICTURE_B: &str =
            "0f3c5a7e9b1d2f4a6c8e0b2d4f6a8c0e1b3d5f7a9c1e3b5d7f9a1c3e9a8b7c6d.png";
        const SOUND_A: &str = "ca31822c1384c4847d22acfedfb0c2ff8b70753a.mp3";
        const SOUND_B: &str = "77e1c4a90d3b5f2e8a6c1d0b9f7e5a3c9a8b7c6d.mp3";
        const SOUND_C: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f809e4d5c6b7.mp3";
        const SOUND_D: &str = "f0e1d2c3b4a5968778695a4b3c2d1e0f00ff00ff.mp3";

        let json = format!(
            r#"{{
            "version":2,"nightModeAvailable":true,
            "stageNodes":[
              {{"uuid":"s0","squareOne":true,"image":"{PICTURE_A}","audio":"{SOUND_A}",
                "okTransition":{{"actionNode":"a0","optionIndex":1}},"homeTransition":null,
                "controlSettings":{{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}}},
              {{"uuid":"s1","image":"{PICTURE_B}","audio":"{SOUND_B}",
                "okTransition":{{"actionNode":"a1","optionIndex":0}},
                "homeTransition":{{"actionNode":"a0","optionIndex":0}},
                "controlSettings":{{"wheel":true,"ok":true,"home":true,"pause":false,"autoplay":false}}}},
              {{"uuid":"s2","image":"{PICTURE_A}","audio":"{SOUND_C}",
                "okTransition":{{"actionNode":"a1","optionIndex":0}},
                "homeTransition":{{"actionNode":"a0","optionIndex":1}},
                "controlSettings":{{"wheel":true,"ok":true,"home":true,"pause":false,"autoplay":false}}}},
              {{"uuid":"s3","image":null,"audio":"{SOUND_D}",
                "okTransition":null,
                "homeTransition":{{"actionNode":"a0","optionIndex":1}},
                "controlSettings":{{"wheel":false,"ok":false,"home":true,"pause":true,"autoplay":true}}}}
            ],
            "actionNodes":[
              {{"id":"a0","options":["s1","s2"]}},
              {{"id":"a1","options":["s3"]}}
            ]
        }}"#
        );
        let pack: crate::domain::device::StudioStoryPack = serde_json::from_str(&json).unwrap();
        let transcoded = transcode_pack(&pack).unwrap();

        let assets: HashMap<&str, Vec<u8>> = HashMap::from([
            (PICTURE_A, asset_bytes(1, 100)),
            (PICTURE_B, asset_bytes(2, 512)),
            (SOUND_A, asset_bytes(3, 600)),
            (SOUND_B, asset_bytes(4, 5000)),
            (SOUND_C, asset_bytes(5, 16)),
            (SOUND_D, asset_bytes(6, 511)),
        ]);
        let resolve = |f: &str| assets.get(f).cloned();
        let files = assemble_v3_pack(&transcoded, &tiny_md(), &resolve).expect("assemble");

        // The files and their order: the index files and markers, then the
        // pictures and the sounds in index order, each under the last 8
        // characters of its source name.
        let paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "ni",
                "bt",
                "nm",
                "li",
                "ri",
                "si",
                "rf/000/532195B9",
                "rf/000/9A8B7C6D",
                "sf/000/8B70753A",
                "sf/000/9A8B7C6D",
                "sf/000/E4D5C6B7",
                "sf/000/00FF00FF",
            ]
        );

        // Their bytes: one digest over every file, in order — its path, its
        // length, its content.
        let mut digest = Sha256::new();
        for file in &files {
            digest.update(file.rel_path.as_bytes());
            digest.update([0]);
            digest.update((file.bytes.len() as u64).to_le_bytes());
            digest.update(&file.bytes);
        }
        let digest: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            digest, "2f4b5f0a77b06cf42ab20e00d0f562d4da97494f84dc755976bb4d6c3c0344ff",
            "the bytes written for a pack without a name clash changed"
        );
    }

    #[test]
    fn two_assets_whose_names_end_alike_are_assembled_as_two_files_holding_their_own_bytes() {
        // Two pictures and two sounds whose names end with the same 8
        // characters: each must become a file of its own, found through its
        // own index entry — never one file written twice.
        let picture_one = format!("{}1234abcd.bmp", "a".repeat(32));
        let picture_two = format!("{}1234abcd.bmp", "b".repeat(32));
        let sound_one = format!("{}5678ef01.mp3", "c".repeat(32));
        let sound_two = format!("{}5678ef01.mp3", "d".repeat(32));
        let stage = |uuid: &str, image: &str, audio: &str| {
            format!(
                r#"{{"uuid":"{uuid}","squareOne":{},"image":"{image}","audio":"{audio}",
                    "okTransition":null,"homeTransition":null,
                    "controlSettings":{{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}}}"#,
                uuid == "s0"
            )
        };
        let json = format!(
            r#"{{"version":1,"nightModeAvailable":false,"stageNodes":[{},{}],"actionNodes":[]}}"#,
            stage("s0", &picture_one, &sound_one),
            stage("s1", &picture_two, &sound_two),
        );
        let pack: crate::domain::device::StudioStoryPack = serde_json::from_str(&json).unwrap();
        let transcoded = transcode_pack(&pack).unwrap();

        let assets: HashMap<&str, Vec<u8>> = HashMap::from([
            (picture_one.as_str(), asset_bytes(1, 600)),
            (picture_two.as_str(), asset_bytes(2, 600)),
            (sound_one.as_str(), asset_bytes(3, 700)),
            (sound_two.as_str(), asset_bytes(4, 700)),
        ]);
        let resolve = |f: &str| assets.get(f).cloned();
        let files = assemble_v3_pack(&transcoded, &tiny_md(), &resolve).expect("assemble");

        // No two files of the pack share a path.
        let mut paths = std::collections::HashSet::new();
        for file in &files {
            assert!(paths.insert(file.rel_path.as_str()), "{}", file.rel_path);
        }

        // Each index entry names a file that holds ITS asset: read the way
        // the device does, through the deciphered `ri` / `si`.
        let (key, iv) = v3_story_key_iv(&tiny_md()).unwrap();
        let deciphered = |rel_path: &str| -> Vec<u8> {
            let file = files
                .iter()
                .find(|f| f.rel_path == rel_path)
                .unwrap_or_else(|| panic!("{rel_path} is assembled"));
            let mut bytes = file.bytes.clone();
            super::super::cipher::v3_decipher_in_place(&mut bytes, &key, &iv);
            bytes
        };
        for (index_file, dir, sources) in [
            ("ri", "rf", [&picture_one, &picture_two]),
            ("si", "sf", [&sound_one, &sound_two]),
        ] {
            let table = deciphered(index_file);
            for (index, source) in sources.into_iter().enumerate() {
                let entry = &table[index * 12..(index + 1) * 12];
                assert_eq!(&entry[..4], b"000\\");
                let name = std::str::from_utf8(&entry[4..]).expect("an ascii name");
                assert_eq!(
                    deciphered(&format!("{dir}/000/{name}")),
                    assets[source.as_str()],
                    "{index_file} entry {index} leads to its own asset"
                );
            }
        }
        // The first of each pair keeps the name it always had.
        assert!(paths.contains("rf/000/1234ABCD"));
        assert!(paths.contains("sf/000/5678EF01"));
    }

    #[test]
    fn media_whose_names_are_no_device_names_are_assembled_as_files_their_index_entries_lead_to() {
        // Too short, inside a folder, not ASCII: written under the last 8
        // characters of these names, the pack had index entries of every
        // length and a picture in a folder of its own.
        let pictures = ["a.bmp", "assets/cover.png", "xé1234567.png"];
        let sounds = ["story.mp3", "sons/fin.mp3", "écoutons.mp3"];
        let stages: Vec<String> = (0..3)
            .map(|index| {
                format!(
                    r#"{{"uuid":"s{index}","squareOne":{},"image":"{}","audio":"{}",
                    "okTransition":null,"homeTransition":null,
                    "controlSettings":{{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}}}"#,
                    index == 0,
                    pictures[index],
                    sounds[index]
                )
            })
            .collect();
        let json = format!(
            r#"{{"version":1,"nightModeAvailable":false,"stageNodes":[{}],"actionNodes":[]}}"#,
            stages.join(",")
        );
        let pack: crate::domain::device::StudioStoryPack = serde_json::from_str(&json).unwrap();
        let transcoded = transcode_pack(&pack).unwrap();

        let assets: HashMap<&str, Vec<u8>> = pictures
            .iter()
            .chain(&sounds)
            .enumerate()
            .map(|(seed, name)| (*name, asset_bytes(seed as u8 + 1, 600)))
            .collect();
        let resolve = |f: &str| assets.get(f).cloned();
        let files = assemble_v3_pack(&transcoded, &tiny_md(), &resolve).expect("assemble");

        // Six assets, six files, each directly in its folder under 8 digits
        // and uppercase letters.
        let mut asset_paths = std::collections::HashSet::new();
        for file in &files {
            let Some(name) = file
                .rel_path
                .strip_prefix("rf/000/")
                .or_else(|| file.rel_path.strip_prefix("sf/000/"))
            else {
                assert!(
                    ["ni", "bt", "li", "ri", "si"].contains(&file.rel_path.as_str()),
                    "{} is neither an index file nor in an asset folder",
                    file.rel_path
                );
                continue;
            };
            assert!(
                name.len() == 8
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase()),
                "{}",
                file.rel_path
            );
            assert!(asset_paths.insert(file.rel_path.as_str()), "{}", name);
        }
        assert_eq!(asset_paths.len(), 6);

        // Each index holds one 12-byte entry per asset, and each entry leads
        // to the file of ITS asset: read the way the device does.
        let (key, iv) = v3_story_key_iv(&tiny_md()).unwrap();
        let deciphered = |rel_path: &str| -> Vec<u8> {
            let file = files
                .iter()
                .find(|f| f.rel_path == rel_path)
                .unwrap_or_else(|| panic!("{rel_path} is assembled"));
            let mut bytes = file.bytes.clone();
            super::super::cipher::v3_decipher_in_place(&mut bytes, &key, &iv);
            bytes
        };
        for (index_file, dir, sources) in [("ri", "rf", pictures), ("si", "sf", sounds)] {
            let table = deciphered(index_file);
            assert_eq!(table.len(), 48, "{index_file}: 36 bytes, a whole block");
            assert_eq!(&table[36..], &[0u8; 12], "{index_file}: zero padding");
            for (index, source) in sources.into_iter().enumerate() {
                let entry = &table[index * 12..(index + 1) * 12];
                assert_eq!(&entry[..4], b"000\\", "{index_file} entry {index}");
                let name = std::str::from_utf8(&entry[4..]).expect("an ascii name");
                assert_eq!(
                    deciphered(&format!("{dir}/000/{name}")),
                    assets[source],
                    "{index_file} entry {index} leads to its own asset"
                );
            }
        }
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
