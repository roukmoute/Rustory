//! Transcode a STUdio-format story pack (`story.json`, "FS/raw" model) into the
//! binary index files a Lunii device expects under `.content/<SHORTID>/`:
//! `ni` (node index), `li` (action-node option lists), `ri`/`si` (image/sound
//! indices). PURE: bytes in, bytes out — no I/O, no ciphering (the caller
//! ciphers `li`/`ri`/`si` + assets; `ni` stays cleartext). Faithful port of
//! STUdio `core/.../writer/fs/FsStoryPackWriter.java`.
//!
//! Everything on-device is referenced by INTEGER index, assigned here:
//!   - stage index = position in `stage_nodes`;
//!   - image / sound index = first-appearance order over the stage nodes,
//!     deduplicated by asset reference (the STUdio refs are content-hash
//!     filenames, so equal ref = equal bytes);
//!   - an action node's "index" stored in a transition = the CUMULATIVE option
//!     offset of its list within `li` (in 4-byte units), assigned in
//!     first-appearance order, OK transition before HOME.
//!
//! All `ni`/`li` integers are little-endian; absent transitions write `-1`.

use std::collections::HashMap;

use serde::Deserialize;

/// Size of the `ni` header and the offset at which stage records begin.
const NI_HEADER_LEN: usize = 512;
/// Size of one `ni` stage-node record.
const NI_STAGE_RECORD_LEN: usize = 44;

/// A STUdio `story.json` (`format: "v1"`). Unknown fields (editor metadata like
/// node names/positions) are ignored — only the transferable structure is read.
#[derive(Debug, Clone, Deserialize)]
pub struct StudioStoryPack {
    #[serde(default)]
    pub version: u16,
    #[serde(rename = "nightModeAvailable", default)]
    pub night_mode_available: bool,
    #[serde(rename = "stageNodes")]
    pub stage_nodes: Vec<StudioStageNode>,
    #[serde(rename = "actionNodes")]
    pub action_nodes: Vec<StudioActionNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StudioStageNode {
    pub uuid: String,
    /// The pack entry point (cover). Not used for indexing (position is), kept
    /// for completeness / potential validation.
    #[serde(rename = "squareOne", default)]
    pub square_one: bool,
    /// Asset reference (content-hash filename) or `None`.
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(rename = "okTransition", default)]
    pub ok_transition: Option<StudioTransition>,
    #[serde(rename = "homeTransition", default)]
    pub home_transition: Option<StudioTransition>,
    #[serde(rename = "controlSettings")]
    pub control_settings: StudioControlSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StudioTransition {
    #[serde(rename = "actionNode")]
    pub action_node: String,
    #[serde(rename = "optionIndex", default)]
    pub option_index: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StudioActionNode {
    pub id: String,
    /// Target stage-node UUIDs, one per option.
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct StudioControlSettings {
    #[serde(default)]
    pub wheel: bool,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub home: bool,
    #[serde(default)]
    pub pause: bool,
    #[serde(default)]
    pub autoplay: bool,
}

/// A failure to transcode a pack — a structural inconsistency (a transition or
/// option that references a node/action that does not exist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscodeError {
    /// A transition references an action node id absent from `actionNodes`.
    MissingActionNode(String),
    /// An action node option references a stage uuid absent from `stageNodes`.
    MissingStageNode(String),
}

/// The transcoded device pack: the four binary index files plus the ORDERED
/// asset reference lists (`images[i]` / `audios[i]` are the source asset
/// filenames at index `i`). The caller copies each asset verbatim to
/// `rf/000/<basename>` / `sf/000/<basename>` where `<basename>` =
/// [`device_asset_basename`] of the filename (same value the `ri`/`si` entry
/// carries), and emits an empty `nm` iff `night_mode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodedPack {
    pub ni: Vec<u8>,
    pub li: Vec<u8>,
    pub ri: Vec<u8>,
    pub si: Vec<u8>,
    pub images: Vec<String>,
    pub audios: Vec<String>,
    pub night_mode: bool,
}

/// Turn a parsed STUdio pack into the device binary files. See the module doc
/// for the index-assignment contract. Fails closed on a dangling reference.
pub fn transcode_pack(pack: &StudioStoryPack) -> Result<TranscodedPack, TranscodeError> {
    // Stage uuid → 0-based index (its position defines the on-device index).
    let stage_index: HashMap<&str, i32> = pack
        .stage_nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.uuid.as_str(), i as i32))
        .collect();

    // Image/sound indices: first-appearance order over the stage nodes,
    // deduplicated by asset reference.
    let mut images: Vec<String> = Vec::new();
    let mut image_index: HashMap<&str, i32> = HashMap::new();
    let mut audios: Vec<String> = Vec::new();
    let mut audio_index: HashMap<&str, i32> = HashMap::new();
    for node in &pack.stage_nodes {
        if let Some(img) = node.image.as_deref() {
            image_index.entry(img).or_insert_with(|| {
                images.push(img.to_string());
                images.len() as i32 - 1
            });
        }
        if let Some(aud) = node.audio.as_deref() {
            audio_index.entry(aud).or_insert_with(|| {
                audios.push(aud.to_string());
                audios.len() as i32 - 1
            });
        }
    }

    // `li` is a flat concatenation of ALL action nodes' option lists, in
    // `actionNodes[]` ARRAY order (validated byte-for-byte against a real
    // device — NOT stage-scan order). Each action node's stored "index" is the
    // cumulative option offset (in 4-byte units) where its list begins.
    let mut action_offset: HashMap<&str, (i32, i32)> = HashMap::new();
    let mut li_entries: Vec<i32> = Vec::new();
    for action in &pack.action_nodes {
        let offset = li_entries.len() as i32;
        for opt in &action.options {
            let target = *stage_index
                .get(opt.as_str())
                .ok_or_else(|| TranscodeError::MissingStageNode(opt.clone()))?;
            li_entries.push(target);
        }
        action_offset.insert(action.id.as_str(), (offset, action.options.len() as i32));
    }
    // A transition may still reference an action id absent from the array —
    // fail closed rather than write a phantom offset.
    for node in &pack.stage_nodes {
        for transition in [node.ok_transition.as_ref(), node.home_transition.as_ref()]
            .into_iter()
            .flatten()
        {
            if !action_offset.contains_key(transition.action_node.as_str()) {
                return Err(TranscodeError::MissingActionNode(
                    transition.action_node.clone(),
                ));
            }
        }
    }

    // ni: 512-byte little-endian header, then one 44-byte record per stage node.
    let mut ni = Vec::with_capacity(NI_HEADER_LEN + pack.stage_nodes.len() * NI_STAGE_RECORD_LEN);
    ni.extend_from_slice(&1u16.to_le_bytes()); // node-index format version
    ni.extend_from_slice(&pack.version.to_le_bytes()); // story pack version
    ni.extend_from_slice(&(NI_HEADER_LEN as u32).to_le_bytes()); // node list offset
    ni.extend_from_slice(&(NI_STAGE_RECORD_LEN as u32).to_le_bytes()); // record size
    ni.extend_from_slice(&(pack.stage_nodes.len() as u32).to_le_bytes());
    ni.extend_from_slice(&(images.len() as u32).to_le_bytes());
    ni.extend_from_slice(&(audios.len() as u32).to_le_bytes());
    ni.push(1); // "is factory pack" flag
    ni.resize(NI_HEADER_LEN, 0); // zero-pad to the 0x200 node-list start

    for node in &pack.stage_nodes {
        write_stage_record(&mut ni, node, &image_index, &audio_index, &action_offset);
    }

    // li: flat little-endian i32 stage indices, in action-assignment order.
    let mut li = Vec::with_capacity(li_entries.len() * 4);
    for entry in &li_entries {
        li.extend_from_slice(&entry.to_le_bytes());
    }

    // ri / si: flat 12-byte ASCII `000\XXXXXXXX` entries, XXXXXXXX = the device
    // asset basename = last 8 hex chars of the asset's (SHA-1) filename stem,
    // UPPERCASE. The asset file is stored on-device at `rf/000/XXXXXXXX` /
    // `sf/000/XXXXXXXX` (no extension) — same basename, so the ri/si entry and
    // the folder name are one self-consistent contract.
    let ri = asset_index_table(&images);
    let si = asset_index_table(&audios);

    Ok(TranscodedPack {
        ni,
        li,
        ri,
        si,
        images,
        audios,
        night_mode: pack.night_mode_available,
    })
}

/// Write one 44-byte little-endian stage-node record (see the format spec).
fn write_stage_record(
    ni: &mut Vec<u8>,
    node: &StudioStageNode,
    image_index: &HashMap<&str, i32>,
    audio_index: &HashMap<&str, i32>,
    action_offset: &HashMap<&str, (i32, i32)>,
) {
    let img = node.image.as_deref().map(|i| image_index[i]).unwrap_or(-1);
    let snd = node.audio.as_deref().map(|a| audio_index[a]).unwrap_or(-1);
    ni.extend_from_slice(&img.to_le_bytes());
    ni.extend_from_slice(&snd.to_le_bytes());
    write_transition_fields(ni, node.ok_transition.as_ref(), action_offset);
    write_transition_fields(ni, node.home_transition.as_ref(), action_offset);
    let cs = &node.control_settings;
    for flag in [cs.wheel, cs.ok, cs.home, cs.pause, cs.autoplay] {
        ni.extend_from_slice(&(flag as i16).to_le_bytes());
    }
    ni.extend_from_slice(&0i16.to_le_bytes()); // reserved
}

/// Write a transition's three i32 fields (action li-offset, option count,
/// selected option), or `-1, -1, -1` when the transition is absent.
fn write_transition_fields(
    ni: &mut Vec<u8>,
    transition: Option<&StudioTransition>,
    action_offset: &HashMap<&str, (i32, i32)>,
) {
    let (offset, count, selected) = match transition {
        Some(t) => {
            let (offset, count) = action_offset[t.action_node.as_str()];
            (offset, count, t.option_index)
        }
        None => (-1, -1, -1),
    };
    ni.extend_from_slice(&offset.to_le_bytes());
    ni.extend_from_slice(&count.to_le_bytes());
    ni.extend_from_slice(&selected.to_le_bytes());
}

/// The on-device basename of an asset: the last 8 characters of its filename
/// stem (drop any extension), UPPERCASED. For a SHA-1-named STUdio asset
/// (`<40 hex>.bmp`) this is the last 8 hex chars, matching the `.content`
/// convention observed on real devices.
pub fn device_asset_basename(filename: &str) -> String {
    let stem = filename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(filename);
    let start = stem.len().saturating_sub(8);
    stem[start..].to_ascii_uppercase()
}

/// Build a `ri`/`si` table: one fixed 12-byte ASCII entry `000\XXXXXXXX` per
/// asset (in index order), `\` = 0x5C, `XXXXXXXX` = [`device_asset_basename`].
fn asset_index_table(assets: &[String]) -> Vec<u8> {
    let mut out = Vec::with_capacity(assets.len() * 12);
    for filename in assets {
        out.extend_from_slice(format!("000\\{}", device_asset_basename(filename)).as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(wheel: bool, ok: bool) -> StudioControlSettings {
        StudioControlSettings {
            wheel,
            ok,
            home: false,
            pause: false,
            autoplay: false,
        }
    }

    #[test]
    fn ni_header_matches_the_spec() {
        let pack = StudioStoryPack {
            version: 1,
            night_mode_available: false,
            stage_nodes: vec![StudioStageNode {
                uuid: "s0".into(),
                square_one: true,
                image: Some("a.bmp".into()),
                audio: Some("a.mp3".into()),
                ok_transition: None,
                home_transition: None,
                control_settings: cs(true, true),
            }],
            action_nodes: vec![],
        };
        let out = transcode_pack(&pack).expect("transcode");
        assert_eq!(out.ni.len(), 512 + 44);
        assert_eq!(&out.ni[0..2], &1u16.to_le_bytes()); // format version
        assert_eq!(&out.ni[2..4], &1u16.to_le_bytes()); // pack version
        assert_eq!(&out.ni[4..8], &512u32.to_le_bytes()); // node list offset
        assert_eq!(&out.ni[8..12], &44u32.to_le_bytes()); // record size
        assert_eq!(&out.ni[12..16], &1u32.to_le_bytes()); // stage count
        assert_eq!(&out.ni[16..20], &1u32.to_le_bytes()); // image count
        assert_eq!(&out.ni[20..24], &1u32.to_le_bytes()); // sound count
        assert_eq!(out.ni[24], 1); // factory flag
        assert!(out.ni[25..512].iter().all(|&b| b == 0)); // zero pad
    }

    #[test]
    fn a_leaf_stage_writes_minus_one_transitions_and_asset_indices() {
        let pack = StudioStoryPack {
            version: 1,
            night_mode_available: false,
            stage_nodes: vec![StudioStageNode {
                uuid: "s0".into(),
                square_one: true,
                image: None,
                audio: None,
                ok_transition: None,
                home_transition: None,
                control_settings: cs(false, true),
            }],
            action_nodes: vec![],
        };
        let out = transcode_pack(&pack).expect("transcode");
        let rec = &out.ni[512..512 + 44];
        // image + sound indices = -1
        assert_eq!(i32::from_le_bytes(rec[0..4].try_into().unwrap()), -1);
        assert_eq!(i32::from_le_bytes(rec[4..8].try_into().unwrap()), -1);
        // both transitions = -1,-1,-1
        for off in [8, 12, 16, 20, 24, 28] {
            assert_eq!(
                i32::from_le_bytes(rec[off..off + 4].try_into().unwrap()),
                -1
            );
        }
    }

    #[test]
    fn transitions_resolve_action_offsets_and_options_go_to_li() {
        // s0 --ok--> A0[opt: s1] ; s1 --ok--> A1[opt: s0, s1]
        let pack = StudioStoryPack {
            version: 1,
            night_mode_available: false,
            stage_nodes: vec![
                StudioStageNode {
                    uuid: "s0".into(),
                    square_one: true,
                    image: Some("i0".into()),
                    audio: Some("a0".into()),
                    ok_transition: Some(StudioTransition {
                        action_node: "A0".into(),
                        option_index: 0,
                    }),
                    home_transition: None,
                    control_settings: cs(true, true),
                },
                StudioStageNode {
                    uuid: "s1".into(),
                    square_one: false,
                    image: Some("i0".into()), // same image → dedup to index 0
                    audio: Some("a1".into()),
                    ok_transition: Some(StudioTransition {
                        action_node: "A1".into(),
                        option_index: 1,
                    }),
                    home_transition: None,
                    control_settings: cs(true, true),
                },
            ],
            action_nodes: vec![
                StudioActionNode {
                    id: "A0".into(),
                    options: vec!["s1".into()],
                },
                StudioActionNode {
                    id: "A1".into(),
                    options: vec!["s0".into(), "s1".into()],
                },
            ],
        };
        let out = transcode_pack(&pack).expect("transcode");
        // Image dedup: only 1 distinct image, 2 distinct audios.
        assert_eq!(out.images, vec!["i0"]);
        assert_eq!(out.audios, vec!["a0", "a1"]);
        // li: A0 at offset 0 (1 opt → s1=1), A1 at offset 1 (opts → s0=0, s1=1).
        let li: Vec<i32> = out
            .li
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(li, vec![1, 0, 1]);
        // s0 record: image 0, audio 0, ok offset 0 count 1 selected 0.
        let r0 = &out.ni[512..512 + 44];
        assert_eq!(i32::from_le_bytes(r0[0..4].try_into().unwrap()), 0);
        assert_eq!(i32::from_le_bytes(r0[4..8].try_into().unwrap()), 0);
        assert_eq!(i32::from_le_bytes(r0[8..12].try_into().unwrap()), 0); // ok offset
        assert_eq!(i32::from_le_bytes(r0[12..16].try_into().unwrap()), 1); // ok count
        assert_eq!(i32::from_le_bytes(r0[16..20].try_into().unwrap()), 0); // ok selected
                                                                           // s1 record: audio index 1, ok offset 1 count 2 selected 1.
        let r1 = &out.ni[512 + 44..512 + 88];
        assert_eq!(i32::from_le_bytes(r1[4..8].try_into().unwrap()), 1);
        assert_eq!(i32::from_le_bytes(r1[8..12].try_into().unwrap()), 1); // ok offset
        assert_eq!(i32::from_le_bytes(r1[12..16].try_into().unwrap()), 2); // ok count
        assert_eq!(i32::from_le_bytes(r1[16..20].try_into().unwrap()), 1); // ok selected
    }

    #[test]
    fn device_asset_basename_is_the_uppercased_last_8_of_the_stem() {
        assert_eq!(
            device_asset_basename("5b699d67ef92f738012e3db36f3b2837532195b9.bmp"),
            "532195B9"
        );
        assert_eq!(
            device_asset_basename("ca31822c1384c4847d22acfedfb0c2ff8b70753a.mp3"),
            "8B70753A"
        );
        // No extension, short name → whole stem uppercased.
        assert_eq!(device_asset_basename("abc"), "ABC");
    }

    #[test]
    fn ri_si_entries_are_12_byte_hash_suffix_paths() {
        let table = asset_index_table(&[
            "5b699d67ef92f738012e3db36f3b2837532195b9.bmp".to_string(),
            "ca31822c1384c4847d22acfedfb0c2ff8b70753a.mp3".to_string(),
        ]);
        assert_eq!(&table[0..12], b"000\\532195B9");
        assert_eq!(&table[12..24], b"000\\8B70753A");
    }

    /// Ground truth: transcode a real STUdio `story.json` and compare the
    /// emitted `ni` BYTE-FOR-BYTE with the same pack's on-device `ni` (which is
    /// cleartext — never ciphered). Point `RUSTORY_TEST_STORYJSON` at the
    /// pack's `story.json` and `RUSTORY_TEST_NI` at the device's
    /// `.content/<SHORTID>/ni`. Ignored (needs a real device + its source zip).
    #[test]
    #[ignore = "manual: set RUSTORY_TEST_STORYJSON + RUSTORY_TEST_NI"]
    fn transcoded_ni_matches_a_real_device_pack_byte_for_byte() {
        let json = std::fs::read_to_string(std::env::var("RUSTORY_TEST_STORYJSON").unwrap())
            .expect("read story.json");
        let pack: StudioStoryPack = serde_json::from_str(&json).expect("parse story.json");
        let out = transcode_pack(&pack).expect("transcode");
        let device_ni = std::fs::read(std::env::var("RUSTORY_TEST_NI").unwrap()).expect("read ni");
        eprintln!(
            "[transcode-smoke] mine={} device={} stages={} images={} audios={}",
            out.ni.len(),
            device_ni.len(),
            pack.stage_nodes.len(),
            out.images.len(),
            out.audios.len()
        );
        if out.ni != device_ni {
            for (i, (a, b)) in out.ni.iter().zip(&device_ni).enumerate() {
                assert_eq!(
                    a, b,
                    "[transcode-smoke] first ni byte mismatch at offset {i} (0x{i:x}): mine={a:#04x} device={b:#04x}"
                );
            }
        }
        assert_eq!(out.ni.len(), device_ni.len(), "ni length differs");
        eprintln!("[transcode-smoke] ni MATCHES byte-for-byte ✓");
    }

    #[test]
    fn a_dangling_action_reference_fails_closed() {
        let pack = StudioStoryPack {
            version: 1,
            night_mode_available: false,
            stage_nodes: vec![StudioStageNode {
                uuid: "s0".into(),
                square_one: true,
                image: None,
                audio: None,
                ok_transition: Some(StudioTransition {
                    action_node: "ghost".into(),
                    option_index: 0,
                }),
                home_transition: None,
                control_settings: cs(true, true),
            }],
            action_nodes: vec![],
        };
        assert_eq!(
            transcode_pack(&pack),
            Err(TranscodeError::MissingActionNode("ghost".into()))
        );
    }
}
