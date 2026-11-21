//! The shareable STORY ARCHIVE — the structured archive (`story.json` +
//! `assets/` + an optional `thumbnail.png`, zipped) a library story is
//! exported to so it can be handed to another Rustory library or to another
//! tool that reads the community pack format.
//!
//! PURE: descriptors in, descriptor out — no I/O, no zip, no transcoding.
//! The application layer feeds it the story's pack (its retained source
//! descriptor, or the pack synthesized from its structure — the one the
//! device send writes, or the graph of a story with choices) and gets back
//! the `story.json` bytes plus the ORDERED list of assets to convert and
//! store under `assets/`.
//!
//! What makes the archive ready to use elsewhere, beyond the graph itself:
//!
//! - every stage `uuid` and every action `id` is a well-formed UUID. The
//!   synthesis names its inner nodes `<pack>-episode-<n>` and the like;
//!   readers that parse every stage id as a UUID reject those, and the tools
//!   that write the format only ever emit UUIDs, so each one is replaced by a
//!   UUID derived from the pack identity and the original id (deterministic:
//!   exporting twice gives the same archive). The ENTRY stage keeps its uuid
//!   — it is the pack's identity on a device;
//! - every asset reference is a bare `<40 hex>.<ext>` name under `assets/`,
//!   with the extension of the DEVICE format the application converts it to
//!   (`.bmp` / `.mp3`). Readers pick the media type from that extension, and
//!   a device (or a tool writing to one) keeps only the LAST 8 characters of
//!   the stem as the on-device file name — so those 8 characters are unique
//!   across the archive, never left to chance;
//! - the pack metadata another library needs is always present: `format`,
//!   `version`, `title`, `description`, `nightModeAvailable`.
//!
//! The graph is validated with the device transcoder itself
//! ([`transcode_pack`]): a descriptor the send engine would refuse is never
//! exported.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::domain::device::{
    format_pack_uuid, is_canonical_pack_uuid, transcode_pack, StudioStoryPack,
    LUNII_PACK_UUID_BYTES,
};

/// Extension of an exported story archive (no leading dot — the form the
/// save dialog filter expects).
pub const STORY_ARCHIVE_EXTENSION: &str = "zip";

/// Entry name of the pack descriptor, at the archive root.
pub const STORY_ARCHIVE_DESCRIPTOR_NAME: &str = "story.json";

/// Prefix of every asset entry. The descriptor references assets by the
/// bare name that follows it.
pub const STORY_ARCHIVE_ASSETS_PREFIX: &str = "assets/";

/// Entry name of the story's cover picture, at the archive root — what a
/// library shows for the pack before opening it.
pub const STORY_ARCHIVE_THUMBNAIL_NAME: &str = "thumbnail.png";

/// The descriptor format tag every reader of the community pack expects.
pub const STORY_ARCHIVE_DESCRIPTOR_FORMAT: &str = "v1";

/// Pack version written when the source declares none.
const DEFAULT_PACK_VERSION: u16 = 1;

/// Hex characters of an exported asset stem.
const ASSET_STEM_HEX_CHARS: usize = 40;

/// Trailing stem characters a device keeps as the on-device file name.
const DEVICE_BASENAME_CHARS: usize = 8;

/// Layout of the stage / action nodes on the editor canvas some tools draw:
/// a readable grid, bounded so every coordinate fits a signed 16-bit value.
const CANVAS_COLUMNS: usize = 8;
const CANVAS_COLUMN_STEP: usize = 300;
const CANVAS_ROW_STEP: usize = 420;
const CANVAS_MAX_COORDINATE: usize = 30_000;

/// The two kinds of asset an archive carries, each exported in the one
/// format a device plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoryArchiveAssetKind {
    /// A stage picture — exported as a device bitmap.
    Image,
    /// A stage sound — exported as a device MP3.
    Audio,
}

impl StoryArchiveAssetKind {
    /// Extension of the exported asset (the device format).
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Image => "bmp",
            Self::Audio => "mp3",
        }
    }

    /// Stable tag mixed into the derived asset name.
    const fn tag(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
        }
    }
}

/// One asset of the exported archive: where its bytes come from and the
/// name they are stored under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryArchiveAsset {
    pub kind: StoryArchiveAssetKind,
    /// The reference the SOURCE descriptor carried — an entry of the
    /// retained archive, or a media-store file name.
    pub source_ref: String,
    /// The bare entry name under [`STORY_ARCHIVE_ASSETS_PREFIX`], which the
    /// exported descriptor references.
    pub archive_name: String,
}

/// The exported descriptor and everything the writer needs beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryArchiveDescriptor {
    /// The `story.json` bytes: UTF-8, pretty-printed, trailing newline.
    pub story_json: Vec<u8>,
    /// The pack identity (the entry stage's uuid), canonical lowercase.
    pub pack_uuid: String,
    /// Every DISTINCT asset, images first then sounds, each group in the
    /// order the stages first reference them.
    pub assets: Vec<StoryArchiveAsset>,
}

/// Why a source descriptor cannot become an exported archive. Closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryArchiveDescriptorError {
    /// Not a pack descriptor: not an object, no stage / action node lists,
    /// or a stage without its identity or control settings.
    Unreadable,
    /// The pack has no stage.
    Empty,
    /// The entry stage's uuid is not a well-formed UUID — the pack has no
    /// identity a device or another library could file it under.
    PackIdentity,
    /// A transition or an option points at a node that does not exist.
    Inconsistent,
}

impl StoryArchiveDescriptorError {
    /// Stable diagnostic tag. Closed set.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::Unreadable => "descriptor_unreadable",
            Self::Empty => "descriptor_empty",
            Self::PackIdentity => "pack_identity",
            Self::Inconsistent => "descriptor_inconsistent",
        }
    }
}

/// Write a synthesized pack as a community descriptor value: the graph
/// exactly as the device send lays it out, plus the stage names another
/// library lists the story by. Asset references and stage ids are carried
/// VERBATIM — [`prepare_story_archive_descriptor`] makes them portable, for
/// this descriptor and for a retained one alike.
pub fn studio_pack_descriptor(
    pack: &StudioStoryPack,
    stage_names: &HashMap<String, String>,
) -> Value {
    let transition = |t: &Option<crate::domain::device::pack_transcode::StudioTransition>| match t {
        Some(t) => json!({ "actionNode": t.action_node, "optionIndex": t.option_index }),
        None => Value::Null,
    };
    let stage_nodes: Vec<Value> = pack
        .stage_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let mut stage = Map::new();
            stage.insert("uuid".into(), json!(node.uuid));
            stage.insert(
                "name".into(),
                json!(stage_names
                    .get(&node.uuid)
                    .cloned()
                    .unwrap_or_else(|| format!("Étape {}", index + 1))),
            );
            stage.insert("position".into(), canvas_position(index, 0));
            if node.square_one {
                stage.insert("squareOne".into(), json!(true));
            }
            stage.insert("image".into(), json!(node.image));
            stage.insert("audio".into(), json!(node.audio));
            stage.insert("okTransition".into(), transition(&node.ok_transition));
            stage.insert("homeTransition".into(), transition(&node.home_transition));
            let cs = node.control_settings;
            stage.insert(
                "controlSettings".into(),
                json!({
                    "wheel": cs.wheel,
                    "ok": cs.ok,
                    "home": cs.home,
                    "pause": cs.pause,
                    "autoplay": cs.autoplay,
                }),
            );
            Value::Object(stage)
        })
        .collect();
    let action_nodes: Vec<Value> = pack
        .action_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            json!({
                "id": node.id,
                "name": format!("Action {}", index + 1),
                "position": canvas_position(index, CANVAS_ROW_STEP / 2),
                "options": node.options,
            })
        })
        .collect();
    json!({
        "format": STORY_ARCHIVE_DESCRIPTOR_FORMAT,
        "version": pack.version,
        "nightModeAvailable": pack.night_mode_available,
        "stageNodes": stage_nodes,
        "actionNodes": action_nodes,
    })
}

/// Turn a SOURCE descriptor (a retained `story.json`, or the value
/// [`studio_pack_descriptor`] wrote) into the exported one, titled `title`.
///
/// Everything the source carries beyond what is rewritten — node names,
/// positions, types, a description — is preserved as is.
pub fn prepare_story_archive_descriptor(
    mut source: Value,
    title: &str,
) -> Result<StoryArchiveDescriptor, StoryArchiveDescriptorError> {
    if !source.is_object() {
        return Err(StoryArchiveDescriptorError::Unreadable);
    }
    // The typed view the device send reads: its parse and its transcode are
    // the validation, and the transcode's asset lists are the export's.
    let pack: StudioStoryPack = serde_json::from_value(source.clone())
        .map_err(|_| StoryArchiveDescriptorError::Unreadable)?;
    let entry = pack
        .stage_nodes
        .iter()
        .find(|node| node.square_one)
        .or_else(|| pack.stage_nodes.first())
        .ok_or(StoryArchiveDescriptorError::Empty)?;
    let pack_uuid = entry.uuid.to_ascii_lowercase();
    if !is_canonical_pack_uuid(&pack_uuid) {
        return Err(StoryArchiveDescriptorError::PackIdentity);
    }
    let transcoded =
        transcode_pack(&pack).map_err(|_| StoryArchiveDescriptorError::Inconsistent)?;

    // Node ids that are not UUIDs get a derived one; the others are kept.
    // Stages and actions draw from ONE set of taken ids, so a derived id can
    // never land on an id the pack already uses, on either side.
    let mut taken: HashSet<String> = pack
        .stage_nodes
        .iter()
        .map(|node| node.uuid.to_ascii_lowercase())
        .chain(
            pack.action_nodes
                .iter()
                .map(|node| node.id.to_ascii_lowercase()),
        )
        .collect();
    let portable_uuids = portable_ids(
        NodeRole::Stage,
        &pack_uuid,
        pack.stage_nodes.iter().map(|node| node.uuid.as_str()),
        &mut taken,
    );
    let portable_action_ids = portable_ids(
        NodeRole::Action,
        &pack_uuid,
        pack.action_nodes.iter().map(|node| node.id.as_str()),
        &mut taken,
    );

    // Asset names: images then sounds, each in first-reference order.
    let mut allocator = AssetNameAllocator::default();
    let mut assets = Vec::with_capacity(transcoded.images.len() + transcoded.audios.len());
    let mut archive_names: HashMap<(StoryArchiveAssetKind, String), String> = HashMap::new();
    for (kind, sources) in [
        (StoryArchiveAssetKind::Image, &transcoded.images),
        (StoryArchiveAssetKind::Audio, &transcoded.audios),
    ] {
        for reference in sources.iter().map(|asset| &asset.source_ref) {
            let archive_name = allocator.allocate(kind, reference);
            archive_names.insert((kind, reference.clone()), archive_name.clone());
            assets.push(StoryArchiveAsset {
                kind,
                source_ref: reference.clone(),
                archive_name,
            });
        }
    }

    let root = source
        .as_object_mut()
        .ok_or(StoryArchiveDescriptorError::Unreadable)?;
    if let Some(Value::Array(stages)) = root.get_mut("stageNodes") {
        for stage in stages.iter_mut().filter_map(Value::as_object_mut) {
            if let Some(Value::String(uuid)) = stage.get_mut("uuid") {
                if let Some(portable) = portable_uuids.get(uuid.as_str()) {
                    *uuid = portable.clone();
                }
            }
            for (field, kind) in [
                ("image", StoryArchiveAssetKind::Image),
                ("audio", StoryArchiveAssetKind::Audio),
            ] {
                if let Some(Value::String(reference)) = stage.get_mut(field) {
                    if let Some(name) = archive_names.get(&(kind, reference.clone())) {
                        *reference = name.clone();
                    }
                }
            }
            for field in ["okTransition", "homeTransition"] {
                if let Some(Value::String(action)) = stage
                    .get_mut(field)
                    .and_then(Value::as_object_mut)
                    .and_then(|transition| transition.get_mut("actionNode"))
                {
                    if let Some(portable) = portable_action_ids.get(action.as_str()) {
                        *action = portable.clone();
                    }
                }
            }
        }
    }
    if let Some(Value::Array(actions)) = root.get_mut("actionNodes") {
        for action in actions.iter_mut().filter_map(Value::as_object_mut) {
            if let Some(Value::String(id)) = action.get_mut("id") {
                if let Some(portable) = portable_action_ids.get(id.as_str()) {
                    *id = portable.clone();
                }
            }
            if let Some(Value::Array(options)) = action.get_mut("options") {
                for option in options.iter_mut() {
                    if let Value::String(target) = option {
                        if let Some(portable) = portable_uuids.get(target.as_str()) {
                            *target = portable.clone();
                        }
                    }
                }
            }
        }
    }

    // The pack metadata: the library title always wins (a story renamed
    // after its import is shared under its current name); the rest is
    // filled only where the source is silent.
    root.insert("title".into(), json!(title));
    if !root.get("format").is_some_and(Value::is_string) {
        root.insert("format".into(), json!(STORY_ARCHIVE_DESCRIPTOR_FORMAT));
    }
    if !root.get("version").is_some_and(Value::is_u64) {
        root.insert("version".into(), json!(DEFAULT_PACK_VERSION));
    }
    if !root.get("description").is_some_and(Value::is_string) {
        root.insert("description".into(), json!(""));
    }
    if !root
        .get("nightModeAvailable")
        .is_some_and(Value::is_boolean)
    {
        root.insert("nightModeAvailable".into(), json!(false));
    }

    let mut story_json =
        serde_json::to_vec_pretty(&source).map_err(|_| StoryArchiveDescriptorError::Unreadable)?;
    story_json.push(b'\n');
    Ok(StoryArchiveDescriptor {
        story_json,
        pack_uuid,
        assets,
    })
}

/// Position of node `index` on the editor canvas, `row_offset` below its
/// row's top — a grid of [`CANVAS_COLUMNS`] columns, clamped so a very large
/// pack still yields coordinates every reader accepts.
fn canvas_position(index: usize, row_offset: usize) -> Value {
    let x = 80 + (index % CANVAS_COLUMNS) * CANVAS_COLUMN_STEP;
    let y = 80 + (index / CANVAS_COLUMNS) * CANVAS_ROW_STEP + row_offset;
    json!({
        "x": x.min(CANVAS_MAX_COORDINATE),
        "y": y.min(CANVAS_MAX_COORDINATE),
    })
}

/// The two kinds of node a pack graph is made of. Each derives its portable
/// ids in its own namespace: a stage and an action that happen to share an
/// original id still get distinct UUIDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeRole {
    Stage,
    Action,
}

impl NodeRole {
    const fn namespace(self) -> &'static [u8] {
        match self {
            Self::Stage => b"rustory:story-archive:stage\0",
            Self::Action => b"rustory:story-archive:action\0",
        }
    }
}

/// Map every id of `ids` that is NOT a well-formed UUID to a derived one
/// (original id → portable id), reserving each in `taken`. Well-formed ids
/// are absent from the map: they are carried verbatim, whatever their case.
fn portable_ids<'a>(
    role: NodeRole,
    pack_uuid: &str,
    ids: impl Iterator<Item = &'a str>,
    taken: &mut HashSet<String>,
) -> HashMap<String, String> {
    let mut portable = HashMap::new();
    for id in ids {
        if is_canonical_pack_uuid(&id.to_ascii_lowercase()) || portable.contains_key(id) {
            continue;
        }
        let mut salt = 0u32;
        let derived = loop {
            let candidate = portable_node_uuid(role, pack_uuid, id, salt);
            if taken.insert(candidate.clone()) {
                break candidate;
            }
            salt += 1;
        };
        portable.insert(id.to_string(), derived);
    }
    portable
}

/// A well-formed UUID standing for node `node_id` of pack `pack_uuid`: the
/// first 16 bytes of a SHA-256 over the role, the pack and the id, stamped
/// as a version-8 (custom) variant-1 UUID. Deterministic; `salt` only moves
/// on in the (never observed) event of a clash with an id the pack already
/// uses.
fn portable_node_uuid(role: NodeRole, pack_uuid: &str, node_id: &str, salt: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(role.namespace());
    hasher.update(pack_uuid.as_bytes());
    hasher.update([0]);
    hasher.update(node_id.as_bytes());
    hasher.update([0]);
    hasher.update(salt.to_le_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; LUNII_PACK_UUID_BYTES];
    bytes.copy_from_slice(&digest[..LUNII_PACK_UUID_BYTES]);
    bytes[6] = (bytes[6] & 0x0F) | 0x80;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    format_pack_uuid(&bytes)
}

/// Hands out the archive names of the assets, keeping the device basename
/// (the last [`DEVICE_BASENAME_CHARS`] characters of the stem) unique: two
/// distinct assets must never land on the same on-device file.
#[derive(Debug, Default)]
struct AssetNameAllocator {
    taken_basenames: HashSet<String>,
}

impl AssetNameAllocator {
    /// The archive name of the asset `reference` of kind `kind`:
    /// `<40 hex>.<ext>`, derived from the kind and the source reference.
    fn allocate(&mut self, kind: StoryArchiveAssetKind, reference: &str) -> String {
        let mut salt = 0u32;
        loop {
            let stem = asset_stem(kind, reference, salt);
            let basename = stem[stem.len() - DEVICE_BASENAME_CHARS..].to_string();
            if self.taken_basenames.insert(basename) {
                return format!("{stem}.{}", kind.extension());
            }
            salt += 1;
        }
    }
}

/// The hex stem of an exported asset name.
fn asset_stem(kind: StoryArchiveAssetKind, reference: &str, salt: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rustory:story-archive:asset\0");
    hasher.update(kind.tag().as_bytes());
    hasher.update([0]);
    hasher.update(reference.as_bytes());
    hasher.update([0]);
    hasher.update(salt.to_le_bytes());
    let digest = hasher.finalize();
    let mut stem = String::with_capacity(ASSET_STEM_HEX_CHARS);
    for byte in digest.iter().take(ASSET_STEM_HEX_CHARS / 2) {
        stem.push_str(&format!("{byte:02x}"));
    }
    stem
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::{
        stage_names, synthesize_menu_pack, synthesize_sequential_pack, EpisodeAssets, MenuEpisode,
        MenuPackAssets, StoryLayout,
    };

    const PACK: &str = "01a06ed9-2040-77c1-9e03-b8f429f4e954";

    fn sequential_source() -> Value {
        let pack = synthesize_sequential_pack(
            PACK,
            &[
                EpisodeAssets {
                    audio_ref: format!("{}.m4a", "a".repeat(64)),
                    image_ref: Some(format!("{}.png", "b".repeat(64))),
                },
                EpisodeAssets {
                    audio_ref: format!("{}.mp3", "c".repeat(64)),
                    image_ref: None,
                },
            ],
        );
        let names = stage_names(
            StoryLayout::Sequential,
            PACK,
            "Les Bestioles",
            &["Le loup", ""],
        );
        studio_pack_descriptor(&pack, &names)
    }

    fn menu_source() -> Value {
        let pack = synthesize_menu_pack(
            PACK,
            &MenuPackAssets {
                title_ref: Some(format!("{}.wav", "1".repeat(64))),
                question_ref: format!("{}.wav", "2".repeat(64)),
                episodes: vec![
                    MenuEpisode {
                        audio_ref: format!("{}.m4a", "a".repeat(64)),
                        image_ref: Some(format!("{}.png", "b".repeat(64))),
                        prompt_ref: format!("{}.wav", "3".repeat(64)),
                    },
                    MenuEpisode {
                        audio_ref: format!("{}.mp3", "c".repeat(64)),
                        image_ref: None,
                        prompt_ref: format!("{}.wav", "4".repeat(64)),
                    },
                ],
                auto_continue: false,
            },
        );
        let names = stage_names(StoryLayout::Menu, PACK, "Série", &["Un", "Deux"]);
        studio_pack_descriptor(&pack, &names)
    }

    fn parsed(descriptor: &StoryArchiveDescriptor) -> Value {
        serde_json::from_slice(&descriptor.story_json).expect("story.json parses")
    }

    fn stage_uuids(root: &Value) -> Vec<String> {
        root["stageNodes"]
            .as_array()
            .expect("stages")
            .iter()
            .map(|s| s["uuid"].as_str().expect("uuid").to_string())
            .collect()
    }

    #[test]
    fn a_sequential_pack_exports_with_well_formed_stage_uuids_and_its_identity_kept() {
        let descriptor =
            prepare_story_archive_descriptor(sequential_source(), "Les Bestioles").expect("ok");
        assert_eq!(descriptor.pack_uuid, PACK);
        let root = parsed(&descriptor);
        let uuids = stage_uuids(&root);
        assert_eq!(uuids.len(), 3, "cover + two episodes");
        assert_eq!(uuids[0], PACK, "the entry stage keeps the pack identity");
        for uuid in &uuids {
            assert!(is_canonical_pack_uuid(uuid), "{uuid} must be a UUID");
        }
        let distinct: HashSet<&String> = uuids.iter().collect();
        assert_eq!(distinct.len(), uuids.len(), "no two stages share an id");
        // Derived ids are version-8, variant-1 UUIDs.
        assert_eq!(&uuids[1][14..15], "8");
        assert!(matches!(&uuids[1][19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn the_options_follow_the_renamed_stages_and_the_graph_still_transcodes() {
        let descriptor =
            prepare_story_archive_descriptor(sequential_source(), "Les Bestioles").expect("ok");
        let root = parsed(&descriptor);
        let uuids = stage_uuids(&root);
        let actions = root["actionNodes"].as_array().expect("actions");
        assert_eq!(actions[0]["options"], json!([uuids[1]]));
        assert_eq!(actions[1]["options"], json!([uuids[2]]));
        // The action ids are UUIDs too, and the transitions follow them.
        let action_ids: Vec<&str> = actions
            .iter()
            .map(|a| a["id"].as_str().expect("action id"))
            .collect();
        assert!(action_ids.iter().all(|id| is_canonical_pack_uuid(id)));
        assert_ne!(action_ids[0], action_ids[1]);
        assert!(action_ids.iter().all(|id| !uuids.iter().any(|u| u == id)));
        let stages = root["stageNodes"].as_array().expect("stages");
        assert_eq!(
            stages[0]["okTransition"]["actionNode"],
            json!(action_ids[0])
        );
        assert_eq!(
            stages[1]["okTransition"]["actionNode"],
            json!(action_ids[1])
        );
        assert_eq!(stages[2]["okTransition"], Value::Null);
        // The exported descriptor is one the send engine itself accepts.
        let pack: StudioStoryPack = serde_json::from_value(root).expect("typed");
        let transcoded = transcode_pack(&pack).expect("transcodes");
        assert_eq!(transcoded.images.len(), 1);
        assert_eq!(transcoded.audios.len(), 2);
    }

    #[test]
    fn a_menu_pack_exports_every_stage_and_announcement() {
        let descriptor = prepare_story_archive_descriptor(menu_source(), "Série").expect("ok");
        let root = parsed(&descriptor);
        let uuids = stage_uuids(&root);
        // cover + question + 2 wheel entries + 2 episodes.
        assert_eq!(uuids.len(), 6);
        assert!(uuids.iter().all(|u| is_canonical_pack_uuid(u)));
        let names: Vec<&str> = root["stageNodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "Série",
                "Question",
                "Choix : Un",
                "Choix : Deux",
                "Un",
                "Deux"
            ]
        );
        // title + question + 2 spoken titles + 2 episodes, one picture.
        let (images, audios): (Vec<_>, Vec<_>) = descriptor
            .assets
            .iter()
            .partition(|a| a.kind == StoryArchiveAssetKind::Image);
        assert_eq!(images.len(), 1);
        assert_eq!(audios.len(), 6);
        let pack: StudioStoryPack = serde_json::from_value(root).expect("typed");
        transcode_pack(&pack).expect("the menu graph transcodes");
    }

    #[test]
    fn assets_are_renamed_to_device_formats_and_listed_images_first() {
        let descriptor =
            prepare_story_archive_descriptor(sequential_source(), "Les Bestioles").expect("ok");
        let kinds: Vec<StoryArchiveAssetKind> = descriptor.assets.iter().map(|a| a.kind).collect();
        assert_eq!(
            kinds,
            vec![
                StoryArchiveAssetKind::Image,
                StoryArchiveAssetKind::Audio,
                StoryArchiveAssetKind::Audio
            ]
        );
        assert_eq!(
            descriptor.assets[0].source_ref,
            format!("{}.png", "b".repeat(64))
        );
        assert_eq!(
            descriptor.assets[1].source_ref,
            format!("{}.m4a", "a".repeat(64))
        );
        for asset in &descriptor.assets {
            let (stem, ext) = asset.archive_name.split_once('.').expect("stem.ext");
            assert_eq!(stem.len(), 40);
            assert!(stem
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
            assert_eq!(ext, asset.kind.extension());
        }
        // The descriptor references the archive names, bare.
        let root = parsed(&descriptor);
        let cover = &root["stageNodes"][0];
        assert_eq!(cover["image"], json!(descriptor.assets[0].archive_name));
        assert_eq!(cover["audio"], json!(descriptor.assets[1].archive_name));
        assert_eq!(root["stageNodes"][2]["image"], Value::Null);
        assert_eq!(
            root["stageNodes"][2]["audio"],
            json!(descriptor.assets[2].archive_name)
        );
    }

    #[test]
    fn the_pack_metadata_is_complete_and_carries_the_library_title() {
        let descriptor =
            prepare_story_archive_descriptor(sequential_source(), "Les Bestioles").expect("ok");
        let root = parsed(&descriptor);
        assert_eq!(root["format"], json!("v1"));
        assert_eq!(root["version"], json!(1));
        assert_eq!(root["title"], json!("Les Bestioles"));
        assert_eq!(root["description"], json!(""));
        assert_eq!(root["nightModeAvailable"], json!(false));
        assert_eq!(root["stageNodes"][0]["squareOne"], json!(true));
        assert!(root["stageNodes"][1].get("squareOne").is_none());
        assert_eq!(root["stageNodes"][0]["name"], json!("Les Bestioles"));
        assert_eq!(root["stageNodes"][2]["name"], json!("Épisode 2"));
        let cs = &root["stageNodes"][0]["controlSettings"];
        for key in ["wheel", "ok", "home", "pause", "autoplay"] {
            assert!(cs[key].is_boolean(), "{key} must be a boolean");
        }
        assert_eq!(
            descriptor.story_json.last(),
            Some(&b'\n'),
            "trailing newline"
        );
    }

    #[test]
    fn exporting_twice_yields_the_same_bytes() {
        let first = prepare_story_archive_descriptor(sequential_source(), "T").expect("ok");
        let second = prepare_story_archive_descriptor(sequential_source(), "T").expect("ok");
        assert_eq!(first, second);
    }

    /// A descriptor the way a community tool writes it: UUID stage ids,
    /// editor metadata, `assets/`-prefixed references, no `format`.
    fn retained_source() -> Value {
        json!({
            "title": "Titre d'origine",
            "description": "Une description",
            "version": 3,
            "nightModeAvailable": true,
            "stageNodes": [
                {
                    "uuid": "ABABABAB-ABAB-ABAB-ABAB-ABABFAC5562D",
                    "type": "cover",
                    "name": "Couverture",
                    "position": { "x": 10, "y": 20 },
                    "squareOne": true,
                    "image": "assets/cover.png",
                    "audio": "title.ogg",
                    "okTransition": { "actionNode": "AAAAAAAA-0000-4000-8000-000000000001", "optionIndex": 0 },
                    "homeTransition": null,
                    "controlSettings": { "wheel": true, "ok": true, "home": false, "pause": false, "autoplay": false }
                },
                {
                    "uuid": "11111111-2222-3333-4444-555555555555",
                    "groupId": "g1",
                    "name": "Histoire",
                    "image": null,
                    "audio": "story.mp3",
                    "okTransition": null,
                    "homeTransition": null,
                    "controlSettings": { "wheel": false, "ok": false, "home": true, "pause": true, "autoplay": true }
                }
            ],
            "actionNodes": [
                { "id": "AAAAAAAA-0000-4000-8000-000000000001", "name": "Vers l'histoire", "options": ["11111111-2222-3333-4444-555555555555"] }
            ]
        })
    }

    #[test]
    fn a_retained_descriptor_keeps_its_graph_and_editor_metadata() {
        let descriptor =
            prepare_story_archive_descriptor(retained_source(), "Mon titre").expect("ok");
        assert_eq!(
            descriptor.pack_uuid, "abababab-abab-abab-abab-ababfac5562d",
            "the identity is reported canonical"
        );
        let root = parsed(&descriptor);
        // Well-formed ids are carried verbatim, whatever their case.
        assert_eq!(
            stage_uuids(&root),
            vec![
                "ABABABAB-ABAB-ABAB-ABAB-ABABFAC5562D",
                "11111111-2222-3333-4444-555555555555"
            ]
        );
        assert_eq!(root["title"], json!("Mon titre"), "the library title wins");
        assert_eq!(root["description"], json!("Une description"));
        assert_eq!(root["version"], json!(3));
        assert_eq!(root["nightModeAvailable"], json!(true));
        assert_eq!(
            root["format"],
            json!("v1"),
            "filled where the source is silent"
        );
        let cover = &root["stageNodes"][0];
        assert_eq!(cover["type"], json!("cover"));
        assert_eq!(cover["name"], json!("Couverture"));
        assert_eq!(cover["position"], json!({ "x": 10, "y": 20 }));
        assert_eq!(root["stageNodes"][1]["groupId"], json!("g1"));
        assert_eq!(
            root["actionNodes"][0]["id"],
            json!("AAAAAAAA-0000-4000-8000-000000000001")
        );
        assert_eq!(
            root["stageNodes"][0]["okTransition"]["actionNode"],
            json!("AAAAAAAA-0000-4000-8000-000000000001")
        );
        assert_eq!(root["actionNodes"][0]["name"], json!("Vers l'histoire"));
        assert_eq!(
            root["actionNodes"][0]["options"],
            json!(["11111111-2222-3333-4444-555555555555"])
        );
    }

    #[test]
    fn a_retained_descriptor_has_its_asset_references_made_bare_and_device_named() {
        let descriptor =
            prepare_story_archive_descriptor(retained_source(), "Mon titre").expect("ok");
        let sources: Vec<(&str, StoryArchiveAssetKind)> = descriptor
            .assets
            .iter()
            .map(|a| (a.source_ref.as_str(), a.kind))
            .collect();
        assert_eq!(
            sources,
            vec![
                ("assets/cover.png", StoryArchiveAssetKind::Image),
                ("title.ogg", StoryArchiveAssetKind::Audio),
                ("story.mp3", StoryArchiveAssetKind::Audio),
            ]
        );
        let root = parsed(&descriptor);
        let image = root["stageNodes"][0]["image"].as_str().expect("image");
        assert!(image.ends_with(".bmp") && !image.contains('/'));
        let audio = root["stageNodes"][0]["audio"].as_str().expect("audio");
        assert!(audio.ends_with(".mp3"), "an ogg source is exported as mp3");
    }

    #[test]
    fn a_non_uuid_stage_id_of_a_retained_descriptor_is_made_portable_too() {
        let mut source = retained_source();
        source["stageNodes"][1]["uuid"] = json!("histoire-1");
        source["actionNodes"][0]["options"] = json!(["histoire-1"]);
        let descriptor = prepare_story_archive_descriptor(source, "T").expect("ok");
        let root = parsed(&descriptor);
        let uuids = stage_uuids(&root);
        assert!(is_canonical_pack_uuid(&uuids[1]));
        assert_eq!(root["actionNodes"][0]["options"], json!([uuids[1]]));
    }

    #[test]
    fn a_non_uuid_action_id_of_a_retained_descriptor_is_made_portable_with_its_transitions() {
        let mut source = retained_source();
        source["actionNodes"][0]["id"] = json!("vers-histoire");
        source["stageNodes"][0]["okTransition"]["actionNode"] = json!("vers-histoire");
        // A stage that homes back through the same action follows it too.
        source["stageNodes"][1]["homeTransition"] =
            json!({ "actionNode": "vers-histoire", "optionIndex": 0 });
        let descriptor = prepare_story_archive_descriptor(source, "T").expect("ok");
        let root = parsed(&descriptor);
        let action_id = root["actionNodes"][0]["id"]
            .as_str()
            .expect("id")
            .to_string();
        assert!(is_canonical_pack_uuid(&action_id));
        assert_eq!(
            root["stageNodes"][0]["okTransition"]["actionNode"],
            json!(action_id)
        );
        assert_eq!(
            root["stageNodes"][1]["homeTransition"]["actionNode"],
            json!(action_id)
        );
        // The graph is still one the send engine accepts.
        let pack: StudioStoryPack = serde_json::from_value(root).expect("typed");
        transcode_pack(&pack).expect("transcodes");
    }

    #[test]
    fn a_stage_and_an_action_sharing_an_original_id_get_distinct_uuids() {
        let mut source = retained_source();
        source["stageNodes"][1]["uuid"] = json!("meme-nom");
        source["actionNodes"][0]["id"] = json!("meme-nom");
        source["actionNodes"][0]["options"] = json!(["meme-nom"]);
        source["stageNodes"][0]["okTransition"]["actionNode"] = json!("meme-nom");
        let descriptor = prepare_story_archive_descriptor(source, "T").expect("ok");
        let root = parsed(&descriptor);
        let stage = root["stageNodes"][1]["uuid"].as_str().expect("stage");
        let action = root["actionNodes"][0]["id"].as_str().expect("action");
        assert!(is_canonical_pack_uuid(stage) && is_canonical_pack_uuid(action));
        assert_ne!(stage, action, "one namespace per node kind");
        assert_eq!(root["actionNodes"][0]["options"], json!([stage]));
        assert_eq!(
            root["stageNodes"][0]["okTransition"]["actionNode"],
            json!(action)
        );
    }

    #[test]
    fn a_descriptor_that_is_not_a_pack_is_unreadable() {
        for source in [
            json!([]),
            json!("pack"),
            json!({}),
            json!({ "stageNodes": [] }),
            json!({ "stageNodes": [{ "uuid": PACK }], "actionNodes": [] }),
            json!({ "stageNodes": "nope", "actionNodes": [] }),
        ] {
            assert_eq!(
                prepare_story_archive_descriptor(source.clone(), "T"),
                Err(StoryArchiveDescriptorError::Unreadable),
                "{source}"
            );
        }
    }

    #[test]
    fn a_pack_without_a_stage_is_empty() {
        let source = json!({ "stageNodes": [], "actionNodes": [] });
        assert_eq!(
            prepare_story_archive_descriptor(source, "T"),
            Err(StoryArchiveDescriptorError::Empty)
        );
    }

    #[test]
    fn an_entry_stage_without_a_uuid_identity_is_refused() {
        let mut source = retained_source();
        source["stageNodes"][0]["uuid"] = json!("ma-couverture");
        assert_eq!(
            prepare_story_archive_descriptor(source, "T"),
            Err(StoryArchiveDescriptorError::PackIdentity)
        );
    }

    #[test]
    fn a_dangling_transition_or_option_is_inconsistent() {
        let mut dangling_action = retained_source();
        dangling_action["stageNodes"][0]["okTransition"]["actionNode"] = json!("nowhere");
        assert_eq!(
            prepare_story_archive_descriptor(dangling_action, "T"),
            Err(StoryArchiveDescriptorError::Inconsistent)
        );
        let mut dangling_option = retained_source();
        dangling_option["actionNodes"][0]["options"] = json!(["missing-stage"]);
        assert_eq!(
            prepare_story_archive_descriptor(dangling_option, "T"),
            Err(StoryArchiveDescriptorError::Inconsistent)
        );
    }

    #[test]
    fn the_device_basename_of_every_asset_is_unique_even_on_a_clash() {
        let mut allocator = AssetNameAllocator::default();
        // Claim the basename the first reference would naturally get: the
        // allocator must move on rather than hand out a second asset that
        // lands on the same on-device file.
        let natural = asset_stem(StoryArchiveAssetKind::Audio, "a.mp3", 0);
        let natural_basename = natural[natural.len() - DEVICE_BASENAME_CHARS..].to_string();
        allocator.taken_basenames.insert(natural_basename.clone());
        let name = allocator.allocate(StoryArchiveAssetKind::Audio, "a.mp3");
        let stem = name.strip_suffix(".mp3").expect("mp3");
        assert_ne!(
            &stem[stem.len() - DEVICE_BASENAME_CHARS..],
            natural_basename
        );
        assert_eq!(stem.len(), ASSET_STEM_HEX_CHARS);
        // And an image and a sound of the same source name never collide.
        let image = allocator.allocate(StoryArchiveAssetKind::Image, "x");
        let audio = allocator.allocate(StoryArchiveAssetKind::Audio, "x");
        assert_ne!(image[..40], audio[..40]);
    }

    #[test]
    fn canvas_positions_always_fit_a_signed_16_bit_coordinate() {
        for index in [0usize, 7, 8, 500, 8191, 100_000] {
            let position = canvas_position(index, CANVAS_ROW_STEP / 2);
            for axis in ["x", "y"] {
                let value = position[axis].as_u64().expect("coordinate");
                assert!(value <= i16::MAX as u64, "{axis} of node {index}");
            }
        }
    }

    #[test]
    fn diagnostic_tags_are_the_frozen_closed_set() {
        assert_eq!(
            [
                StoryArchiveDescriptorError::Unreadable,
                StoryArchiveDescriptorError::Empty,
                StoryArchiveDescriptorError::PackIdentity,
                StoryArchiveDescriptorError::Inconsistent,
            ]
            .map(StoryArchiveDescriptorError::diagnostic_tag),
            [
                "descriptor_unreadable",
                "descriptor_empty",
                "pack_identity",
                "descriptor_inconsistent"
            ]
        );
    }
}
