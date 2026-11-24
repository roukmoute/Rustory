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
//! Every asset is ONE FILE on the device, under a name assigned here and
//! nowhere else ([`TranscodedAsset::device_name`]): its `ri` / `si` entry and
//! the file the caller writes carry that same name. It is the asset's
//! NATURAL name — the last 8 characters of its source file name, what a
//! device pack has always been written with — unless another asset of the
//! same kind already bears it: two different assets never share a file.
//!
//! All `ni`/`li` integers are little-endian; absent transitions write `-1`.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use sha2::{Digest, Sha256};

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

/// One asset of a transcoded pack: where its bytes come from, and the file
/// it becomes on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodedAsset {
    /// The reference the pack's stages carry — an entry of the pack archive,
    /// or a media-store file name. The caller resolves the bytes from it.
    pub source_ref: String,
    /// The name of the asset's file on the device (`rf/000/<name>` for a
    /// picture, `sf/000/<name>` for a sound) — the very name its `ri` / `si`
    /// entry carries. No other asset of the same kind bears it.
    pub device_name: String,
}

/// The transcoded device pack: the four binary index files plus the ORDERED
/// asset lists (`images[i]` / `audios[i]` is the asset at index `i`). The
/// caller writes each asset to `rf/000/<name>` / `sf/000/<name>`, `<name>`
/// being the asset's [`device_name`](TranscodedAsset::device_name) — the
/// value its `ri`/`si` entry carries — and emits an empty `nm` iff
/// `night_mode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodedPack {
    pub ni: Vec<u8>,
    pub li: Vec<u8>,
    pub ri: Vec<u8>,
    pub si: Vec<u8>,
    pub images: Vec<TranscodedAsset>,
    pub audios: Vec<TranscodedAsset>,
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
    let mut image_refs: Vec<String> = Vec::new();
    let mut image_index: HashMap<&str, i32> = HashMap::new();
    let mut audio_refs: Vec<String> = Vec::new();
    let mut audio_index: HashMap<&str, i32> = HashMap::new();
    for node in &pack.stage_nodes {
        if let Some(img) = node.image.as_deref() {
            image_index.entry(img).or_insert_with(|| {
                image_refs.push(img.to_string());
                image_refs.len() as i32 - 1
            });
        }
        if let Some(aud) = node.audio.as_deref() {
            audio_index.entry(aud).or_insert_with(|| {
                audio_refs.push(aud.to_string());
                audio_refs.len() as i32 - 1
            });
        }
    }
    // The file each asset becomes on the device, settled here once: the
    // `ri` / `si` tables below and the caller's writes both read it.
    let images = name_device_assets(AssetKind::Image, image_refs);
    let audios = name_device_assets(AssetKind::Audio, audio_refs);

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

    // ri / si: flat 12-byte ASCII `000\XXXXXXXX` entries, XXXXXXXX = the
    // asset's device name — for a pack without a clash, the last 8 hex chars
    // of the asset's (SHA-1) filename stem, UPPERCASE. The asset file is
    // stored on-device at `rf/000/XXXXXXXX` / `sf/000/XXXXXXXX` (no
    // extension) — the same name, so the ri/si entry and the file name are
    // one self-consistent contract.
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

/// READ back the ENTRY stage's image index from a cleartext `ni` (the exact
/// inverse of the record layout this module writes): stage record 0 starts at
/// the 512-byte header boundary and its first field is the i32 LE image
/// index into `ri`; `-1` means "no image". `None` on any bound/shape
/// violation — the caller treats a cover as pure decoration.
pub fn ni_entry_image_index(ni: &[u8]) -> Option<usize> {
    let record = ni.get(NI_HEADER_LEN..NI_HEADER_LEN + NI_STAGE_RECORD_LEN)?;
    let raw = i32::from_le_bytes([record[0], record[1], record[2], record[3]]);
    usize::try_from(raw).ok()
}

/// READ back the device asset basename of `ri` entry `index` from a
/// DECIPHERED `ri` (12-byte ASCII entries `000\XXXXXXXX`, as written above).
/// Validates the fixed prefix and the 8 uppercase-hex characters so a
/// garbled index (wrong key, corrupt file) yields `None`, never a path.
pub fn ri_entry_basename(ri_plain: &[u8], index: usize) -> Option<String> {
    let start = index.checked_mul(12)?;
    let entry = ri_plain.get(start..start + 12)?;
    if &entry[..4] != b"000\\" {
        return None;
    }
    let basename = &entry[4..12];
    if !basename
        .iter()
        .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(b))
    {
        return None;
    }
    Some(String::from_utf8_lossy(basename).into_owned())
}

/// The NATURAL on-device name of an asset: the last 8 characters of its
/// filename stem (drop any extension), UPPERCASED. For a SHA-1-named STUdio
/// asset (`<40 hex>.bmp`) this is the last 8 hex chars, matching the
/// `.content` convention observed on real devices. It is the name the asset
/// gets on the device unless another asset of its kind bears it already
/// (see [`TranscodedAsset::device_name`]) — and the short tag a diagnostic
/// names a source file by.
///
/// The 8 are counted in BYTES, which for a hash-named file is the same
/// thing. A name that is not ASCII is never cut inside a character: when the
/// 8th byte from the end falls in the middle of one, the name starts at the
/// next whole character and is shorter.
pub fn device_asset_basename(filename: &str) -> String {
    let stem = filename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(filename);
    let mut start = stem.len().saturating_sub(8);
    // The end of the stem is a boundary: the walk always stops.
    while !stem.is_char_boundary(start) {
        start += 1;
    }
    stem[start..].to_ascii_uppercase()
}

/// The two kinds of asset a pack carries. Each has its own folder on the
/// device (`rf/000` for pictures, `sf/000` for sounds) and its own index
/// (`ri`, `si`): a device name only has to be unique within its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    Image,
    Audio,
}

impl AssetKind {
    /// Stable tag mixed into a derived device name.
    const fn tag(self) -> &'static [u8] {
        match self {
            Self::Image => b"image",
            Self::Audio => b"audio",
        }
    }
}

/// Give every asset of one kind (`references`, in index order) the name of
/// its file on the device, so that two different assets NEVER share one: the
/// later file would replace the earlier without a word, and a stage would
/// show or play another stage's asset.
///
/// An asset keeps its natural name ([`device_asset_basename`]) when it is
/// the first of its kind to bear it — the only case there is in a pack
/// without a clash, which is therefore written byte for byte as it always
/// was. An asset whose natural name is already borne gets a DERIVED one
/// ([`derived_device_name`]). The natural names are settled first, so a
/// derived name is never drawn onto the natural name of another asset,
/// wherever that asset stands in the list.
fn name_device_assets(kind: AssetKind, references: Vec<String>) -> Vec<TranscodedAsset> {
    let mut taken: HashSet<String> = HashSet::with_capacity(references.len());
    let natural_names: Vec<Option<String>> = references
        .iter()
        .map(|reference| {
            let name = device_asset_basename(reference);
            taken.insert(name.clone()).then_some(name)
        })
        .collect();
    references
        .into_iter()
        .zip(natural_names)
        .map(|(source_ref, natural_name)| {
            let device_name = natural_name.unwrap_or_else(|| {
                let mut salt = 0u32;
                loop {
                    let candidate = derived_device_name(kind, &source_ref, salt);
                    if taken.insert(candidate.clone()) {
                        break candidate;
                    }
                    salt += 1;
                }
            });
            TranscodedAsset {
                source_ref,
                device_name,
            }
        })
        .collect()
}

/// A device name for the asset `reference` of kind `kind` that does not come
/// from its file name: 8 uppercase hexadecimal characters — the shape of
/// every name in a device pack — taken from a SHA-256 over the kind, the
/// reference and `salt`. Deterministic: the same pack always gets the same
/// names. `salt` only moves on when the name drawn is itself taken.
fn derived_device_name(kind: AssetKind, reference: &str, salt: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rustory:device-pack:asset-name\0");
    hasher.update(kind.tag());
    hasher.update([0]);
    hasher.update(reference.as_bytes());
    hasher.update([0]);
    hasher.update(salt.to_le_bytes());
    // 4 bytes, 8 hexadecimal characters.
    hasher
        .finalize()
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

/// Build a `ri`/`si` table: one fixed 12-byte ASCII entry `000\XXXXXXXX` per
/// asset (in index order), `\` = 0x5C, `XXXXXXXX` = the asset's device name.
fn asset_index_table(assets: &[TranscodedAsset]) -> Vec<u8> {
    let mut out = Vec::with_capacity(assets.len() * 12);
    for asset in assets {
        out.extend_from_slice(format!("000\\{}", asset.device_name).as_bytes());
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

    /// The source references of `assets`, in index order.
    fn source_refs(assets: &[TranscodedAsset]) -> Vec<&str> {
        assets
            .iter()
            .map(|asset| asset.source_ref.as_str())
            .collect()
    }

    /// The device names of `assets`, in index order.
    fn device_names(assets: &[TranscodedAsset]) -> Vec<&str> {
        assets
            .iter()
            .map(|asset| asset.device_name.as_str())
            .collect()
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
    fn ni_and_ri_parsers_round_trip_the_builder_output() {
        // Build with the module's own writer, read back with its parsers:
        // the entry stage's image index and its ri basename must agree.
        let pack = StudioStoryPack {
            version: 1,
            night_mode_available: false,
            stage_nodes: vec![StudioStageNode {
                uuid: "s0".into(),
                square_one: true,
                image: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaa1234abcd.png".into()),
                audio: None,
                ok_transition: None,
                home_transition: None,
                control_settings: cs(true, true),
            }],
            action_nodes: vec![],
        };
        let out = transcode_pack(&pack).expect("transcode");
        let idx = ni_entry_image_index(&out.ni).expect("entry image index");
        assert_eq!(idx, 0);
        assert_eq!(
            ri_entry_basename(&out.ri, idx).as_deref(),
            Some("1234ABCD"),
            "the parsed basename equals the written device basename"
        );
        // An entry WITHOUT an image round-trips to None (index -1).
        let bare = StudioStoryPack {
            version: 1,
            night_mode_available: false,
            stage_nodes: vec![StudioStageNode {
                uuid: "s0".into(),
                square_one: true,
                image: None,
                audio: None,
                ok_transition: None,
                home_transition: None,
                control_settings: cs(true, true),
            }],
            action_nodes: vec![],
        };
        let bare_out = transcode_pack(&bare).expect("transcode");
        assert_eq!(ni_entry_image_index(&bare_out.ni), None);
    }

    #[test]
    fn ri_entry_basename_refuses_garbage_and_out_of_bounds() {
        // Wrong prefix (a mis-deciphered ri) → None, never a path.
        assert_eq!(ri_entry_basename(b"XXX\\1234ABCD", 0), None);
        // Non-hex payload → None.
        assert_eq!(ri_entry_basename(b"000\\1234ZZZZ", 0), None);
        // Lowercase hex is NOT the device convention → None.
        assert_eq!(ri_entry_basename(b"000\\1234abcd", 0), None);
        // Out-of-bounds index → None.
        assert_eq!(ri_entry_basename(b"000\\1234ABCD", 1), None);
        // A short ni (no stage record) yields no entry index.
        assert_eq!(ni_entry_image_index(&[0u8; 512]), None);
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
        assert_eq!(source_refs(&out.images), vec!["i0"]);
        assert_eq!(source_refs(&out.audios), vec!["a0", "a1"]);
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
    fn device_asset_basename_never_cuts_a_character_in_two() {
        // `é` takes two bytes, and the 8th byte from the end of `xé1234567`
        // is its second half: the name starts at the next whole character.
        assert_eq!(device_asset_basename("xé1234567.png"), "1234567");
        // A cut that falls between two characters stays where it always was.
        assert_eq!(device_asset_basename("éé123456.png"), "é123456");
        // Characters of three and of four bytes.
        assert_eq!(device_asset_basename("日本語の物語.bmp"), "物語");
        assert_eq!(device_asset_basename("🦊🦊x"), "🦊X");
    }

    #[test]
    fn a_pack_whose_media_names_are_not_ascii_is_transcoded() {
        // Either name used to stop the transcoding with a panic, the cut of
        // its last 8 bytes landing inside a character.
        let pack = media_pack(vec![media_stage(
            "s0",
            Some("xé1234567.png"),
            Some("écoutons.mp3"),
        )]);
        let out = transcode_pack(&pack).expect("transcode");
        assert_eq!(source_refs(&out.images), vec!["xé1234567.png"]);
        assert_eq!(source_refs(&out.audios), vec!["écoutons.mp3"]);
        assert_eq!(stage_asset_indices(&out.ni, 0), (0, 0));
    }

    #[test]
    fn ri_si_entries_are_12_byte_hash_suffix_paths() {
        let table = asset_index_table(&name_device_assets(
            AssetKind::Image,
            vec![
                "5b699d67ef92f738012e3db36f3b2837532195b9.bmp".to_string(),
                "ca31822c1384c4847d22acfedfb0c2ff8b70753a.mp3".to_string(),
            ],
        ));
        assert_eq!(&table[0..12], b"000\\532195B9");
        assert_eq!(&table[12..24], b"000\\8B70753A");
    }

    // ===== One device file per asset =====

    /// A stage that shows `image` and plays `audio`, with no transition.
    fn media_stage(uuid: &str, image: Option<&str>, audio: Option<&str>) -> StudioStageNode {
        StudioStageNode {
            uuid: uuid.into(),
            square_one: uuid == "s0",
            image: image.map(Into::into),
            audio: audio.map(Into::into),
            ok_transition: None,
            home_transition: None,
            control_settings: cs(true, true),
        }
    }

    fn media_pack(stage_nodes: Vec<StudioStageNode>) -> StudioStoryPack {
        StudioStoryPack {
            version: 1,
            night_mode_available: false,
            stage_nodes,
            action_nodes: vec![],
        }
    }

    /// The image and sound indices of stage `index`, read from `ni`.
    fn stage_asset_indices(ni: &[u8], index: usize) -> (i32, i32) {
        let at = NI_HEADER_LEN + index * NI_STAGE_RECORD_LEN;
        (
            i32::from_le_bytes(ni[at..at + 4].try_into().unwrap()),
            i32::from_le_bytes(ni[at + 4..at + 8].try_into().unwrap()),
        )
    }

    fn is_device_name(name: &str) -> bool {
        name.len() == 8
            && name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
    }

    #[test]
    fn two_assets_whose_names_end_alike_get_two_device_names_each_in_its_own_index_entry() {
        // Two pictures and two sounds whose file names end with the same 8
        // characters: named by those characters alone, the second file of
        // each pair would land on the first.
        let picture_one = format!("{}1234abcd.png", "a".repeat(32));
        let picture_two = format!("{}1234abcd.png", "b".repeat(32));
        let sound_one = format!("{}5678ef01.mp3", "c".repeat(32));
        let sound_two = format!("{}5678ef01.mp3", "d".repeat(32));
        let pack = media_pack(vec![
            media_stage("s0", Some(&picture_one), Some(&sound_one)),
            media_stage("s1", Some(&picture_two), Some(&sound_two)),
            // The first picture again: one asset, one index, one file.
            media_stage("s2", Some(&picture_one), None),
        ]);
        let out = transcode_pack(&pack).expect("transcode");

        // The first of each pair keeps the name it always had; the second
        // gets a name of its own, of the same shape.
        assert_eq!(
            source_refs(&out.images),
            vec![picture_one.as_str(), picture_two.as_str()]
        );
        assert_eq!(
            source_refs(&out.audios),
            vec![sound_one.as_str(), sound_two.as_str()]
        );
        let pictures = device_names(&out.images);
        let sounds = device_names(&out.audios);
        assert_eq!(pictures[0], "1234ABCD");
        assert_eq!(sounds[0], "5678EF01");
        assert_ne!(pictures[1], pictures[0]);
        assert_ne!(sounds[1], sounds[0]);
        assert!(is_device_name(pictures[1]), "{}", pictures[1]);
        assert!(is_device_name(sounds[1]), "{}", sounds[1]);
        assert_eq!(
            pictures[1],
            derived_device_name(AssetKind::Image, &picture_two, 0)
        );
        assert_eq!(
            sounds[1],
            derived_device_name(AssetKind::Audio, &sound_two, 0)
        );

        // `ri` / `si` name those very files, entry for entry…
        assert_eq!(
            out.ri,
            format!("000\\{}000\\{}", pictures[0], pictures[1]).into_bytes()
        );
        assert_eq!(
            out.si,
            format!("000\\{}000\\{}", sounds[0], sounds[1]).into_bytes()
        );
        assert_eq!(ri_entry_basename(&out.ri, 0).as_deref(), Some(pictures[0]));
        assert_eq!(
            ri_entry_basename(&out.ri, 1).as_deref(),
            Some(pictures[1]),
            "a derived name reads back like any other"
        );
        // …and every stage points at the entry of ITS picture and sound.
        assert_eq!(stage_asset_indices(&out.ni, 0), (0, 0));
        assert_eq!(stage_asset_indices(&out.ni, 1), (1, 1));
        assert_eq!(stage_asset_indices(&out.ni, 2), (0, -1));

        // The same pack always gets the same names.
        assert_eq!(transcode_pack(&pack).expect("transcode"), out);
    }

    #[test]
    fn a_picture_and_a_sound_may_bear_the_same_device_name() {
        // Pictures and sounds live in two folders and two indices: the same
        // 8 characters on both sides are no clash, and both keep them.
        let picture = format!("{}1234abcd.bmp", "a".repeat(32));
        let sound = format!("{}1234abcd.mp3", "b".repeat(32));
        let pack = media_pack(vec![media_stage("s0", Some(&picture), Some(&sound))]);
        let out = transcode_pack(&pack).expect("transcode");
        assert_eq!(device_names(&out.images), vec!["1234ABCD"]);
        assert_eq!(device_names(&out.audios), vec!["1234ABCD"]);
        assert_eq!(out.ri, b"000\\1234ABCD");
        assert_eq!(out.si, b"000\\1234ABCD");
    }

    #[test]
    fn a_derived_name_never_lands_on_the_natural_name_of_another_asset() {
        // The second picture clashes with the first and would be given the
        // derived name `drawn` — but a THIRD picture, further down the pack,
        // bears exactly that name naturally. It keeps it, and the second
        // picture draws again.
        let first = format!("{}1234abcd.png", "a".repeat(32));
        let second = format!("{}1234abcd.png", "b".repeat(32));
        let drawn = derived_device_name(AssetKind::Image, &second, 0);
        let third = format!("{}{}.png", "c".repeat(32), drawn.to_ascii_lowercase());
        let pack = media_pack(vec![
            media_stage("s0", Some(&first), None),
            media_stage("s1", Some(&second), None),
            media_stage("s2", Some(&third), None),
        ]);
        let out = transcode_pack(&pack).expect("transcode");
        let redrawn = derived_device_name(AssetKind::Image, &second, 1);
        assert_ne!(redrawn, drawn);
        assert_eq!(
            device_names(&out.images),
            vec!["1234ABCD", redrawn.as_str(), drawn.as_str()]
        );
    }

    #[test]
    fn every_asset_gets_its_own_device_name_however_many_end_alike() {
        // 500 pictures whose names all end with the same 8 characters, in a
        // pack that also holds pictures with a name of their own.
        let mut stages = Vec::new();
        for index in 0..500 {
            let clashing = format!("{index:032x}1234abcd.png");
            stages.push(media_stage(&format!("c{index}"), Some(&clashing), None));
            if index % 50 == 0 {
                let unique = format!("{}{index:08x}.png", "f".repeat(32));
                stages.push(media_stage(&format!("u{index}"), Some(&unique), None));
            }
        }
        let out = transcode_pack(&media_pack(stages)).expect("transcode");
        assert_eq!(out.images.len(), 510);
        let names = device_names(&out.images);
        let distinct: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(distinct.len(), names.len(), "no two pictures share a file");
        assert!(names.iter().all(|name| is_device_name(name)));
        for (index, asset) in out.images.iter().enumerate() {
            // An asset alone to bear its natural name keeps it.
            if asset.source_ref.starts_with('f') {
                assert_eq!(asset.device_name, device_asset_basename(&asset.source_ref));
            }
            // The index entry of every asset names its file.
            assert_eq!(
                &out.ri[index * 12..(index + 1) * 12],
                format!("000\\{}", asset.device_name).as_bytes()
            );
        }
        assert_eq!(names[0], "1234ABCD", "the first one keeps the shared name");
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
