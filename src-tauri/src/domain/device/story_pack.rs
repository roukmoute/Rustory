//! Synthesize a STUdio-format pack from a LIBRARY story — the bridge that
//! lets a story created in Rustory (from a web page, an RSS feed, a folder
//! or the editor — anything without a retained source `.zip`) reach a Lunii
//! V3 through the proven send engine (`transcode_pack` → device
//! normalization → assembly → write).
//!
//! Two PURE steps, both free of I/O:
//!
//! 1. [`linear_episodes`] — reads the canonical structure as an ORDERED
//!    episode list (the start node first, then the others in node order)
//!    and decides whether the story can be sent at all: every node must
//!    carry an audio (the device plays nothing else) and none may branch
//!    (a story with choices needs a menu layout this synthesis does not
//!    produce yet). The refusal reasons are a closed set
//!    ([`StoryPackBlocker`]) the library card surfaces BEFORE any click.
//! 2. [`synthesize_sequential_pack`] — lays the episodes out as SEQUENTIAL
//!    PLAYBACK, mirroring the STUdio writer conventions validated on real
//!    devices: a cover stage (`squareOne`, wheel + OK, carrying the first
//!    episode's image and audio as the pack's prompt) leads into episode 1;
//!    every episode is an autoplay "story" stage (pause + home only) wrapped
//!    in its own single-option action node so it can be a transition target;
//!    each episode's OK transition chains to the next; the LAST episode has
//!    none, and no stage has a home transition — an absent transition is the
//!    device's own "back to the pack selection" (STUdio relies on exactly
//!    that for menus entered from the cover).
//!
//! The pack UUID is the cover stage's uuid (see the send engine's
//! `pack_entry_uuid`): the caller passes the STORY id, a canonical lowercase
//! UUID, so re-sending a story REPLACES its pack on the device rather than
//! adding a copy.

use crate::domain::story::CanonicalStructure;

use super::pack_transcode::{
    StudioActionNode, StudioControlSettings, StudioStageNode, StudioStoryPack, StudioTransition,
};

/// Why a story cannot be laid out as a device pack. Closed set, surfaced on
/// the library card (pre-click) and as the send refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryPackBlocker {
    /// The structure has no node.
    Empty,
    /// The structure's start node is not among its nodes.
    Malformed,
    /// A node offers choices — a menu layout, not produced by this synthesis.
    Branching,
    /// A node has no audio: the device has nothing to play for it.
    MissingAudio,
}

impl StoryPackBlocker {
    /// Stable wire / diagnostic tag. Closed set.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Malformed => "malformed",
            Self::Branching => "branching",
            Self::MissingAudio => "missing_audio",
        }
    }
}

/// One episode of the sequential layout: the node it comes from and its
/// media ASSET IDS (the application layer resolves them to stored files).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearEpisode<'a> {
    pub node_id: &'a str,
    pub label: &'a str,
    pub audio_asset_id: &'a str,
    pub image_asset_id: Option<&'a str>,
}

/// Read `structure` as an ordered, sendable episode list, or say why not.
pub fn linear_episodes(
    structure: &CanonicalStructure,
) -> Result<Vec<LinearEpisode<'_>>, StoryPackBlocker> {
    if structure.nodes.is_empty() {
        return Err(StoryPackBlocker::Empty);
    }
    let start = structure
        .nodes
        .iter()
        .position(|node| node.id == structure.start_node_id)
        .ok_or(StoryPackBlocker::Malformed)?;
    // The start node first, then the others in their node order.
    let ordered = std::iter::once(&structure.nodes[start]).chain(
        structure
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != start)
            .map(|(_, node)| node),
    );
    let mut episodes = Vec::with_capacity(structure.nodes.len());
    for node in ordered {
        if !node.options.is_empty() {
            return Err(StoryPackBlocker::Branching);
        }
        let audio_asset_id = node
            .audio_asset_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or(StoryPackBlocker::MissingAudio)?;
        episodes.push(LinearEpisode {
            node_id: &node.id,
            label: &node.label,
            audio_asset_id,
            image_asset_id: node.image_asset_id.as_deref().filter(|id| !id.is_empty()),
        });
    }
    Ok(episodes)
}

/// The media of one episode as PACK ASSET REFERENCES — the "filenames" the
/// STUdio model carries (here the media store's `<hash>.<ext>` names, whose
/// last 8 hex characters become the on-device basenames).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeAssets {
    pub audio_ref: String,
    pub image_ref: Option<String>,
}

/// Lay `episodes` (non-empty, in playback order) out as a sequential-playback
/// STUdio pack whose entry uuid is `pack_uuid`. See the module doc for the
/// exact stage/action layout.
pub fn synthesize_sequential_pack(pack_uuid: &str, episodes: &[EpisodeAssets]) -> StudioStoryPack {
    let cover_controls = StudioControlSettings {
        wheel: true,
        ok: true,
        home: false,
        pause: false,
        autoplay: false,
    };
    let episode_controls = StudioControlSettings {
        wheel: false,
        ok: false,
        home: true,
        pause: true,
        autoplay: true,
    };
    let episode_uuid = |index: usize| format!("{pack_uuid}-episode-{}", index + 1);
    let action_id = |index: usize| format!("{pack_uuid}-episode-{}-action", index + 1);
    let into_episode = |index: usize| {
        Some(StudioTransition {
            action_node: action_id(index),
            option_index: 0,
        })
    };

    let mut stage_nodes = Vec::with_capacity(episodes.len() + 1);
    let first = episodes.first();
    stage_nodes.push(StudioStageNode {
        uuid: pack_uuid.to_string(),
        square_one: true,
        image: first.and_then(|e| e.image_ref.clone()),
        audio: first.map(|e| e.audio_ref.clone()),
        ok_transition: first.and_then(|_| into_episode(0)),
        home_transition: None,
        control_settings: cover_controls,
    });
    for (index, episode) in episodes.iter().enumerate() {
        let is_last = index + 1 == episodes.len();
        stage_nodes.push(StudioStageNode {
            uuid: episode_uuid(index),
            square_one: false,
            image: episode.image_ref.clone(),
            audio: Some(episode.audio_ref.clone()),
            ok_transition: if is_last {
                None
            } else {
                into_episode(index + 1)
            },
            home_transition: None,
            control_settings: episode_controls,
        });
    }
    let action_nodes = (0..episodes.len())
        .map(|index| StudioActionNode {
            id: action_id(index),
            options: vec![episode_uuid(index)],
        })
        .collect();

    StudioStoryPack {
        version: 1,
        night_mode_available: false,
        stage_nodes,
        action_nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::transcode_pack;
    use crate::domain::story::{CanonicalNode, CanonicalOption};

    fn node(id: &str, audio: Option<&str>, image: Option<&str>) -> CanonicalNode {
        CanonicalNode {
            id: id.into(),
            text: String::new(),
            label: format!("label {id}"),
            image_asset_id: image.map(str::to_string),
            audio_asset_id: audio.map(str::to_string),
            options: Vec::new(),
        }
    }

    fn structure(start: &str, nodes: Vec<CanonicalNode>) -> CanonicalStructure {
        CanonicalStructure {
            schema_version: 3,
            start_node_id: start.into(),
            nodes,
        }
    }

    // ===== linear_episodes =====

    #[test]
    fn an_empty_structure_is_blocked_as_empty() {
        assert_eq!(
            linear_episodes(&structure("n1", vec![])),
            Err(StoryPackBlocker::Empty)
        );
    }

    #[test]
    fn a_start_node_absent_from_the_nodes_is_malformed() {
        let s = structure("nope", vec![node("n1", Some("a1"), None)]);
        assert_eq!(linear_episodes(&s), Err(StoryPackBlocker::Malformed));
    }

    #[test]
    fn a_node_with_choices_is_blocked_as_branching() {
        let mut n1 = node("n1", Some("a1"), None);
        n1.options.push(CanonicalOption {
            label: "suite".into(),
            target: Some("n2".into()),
        });
        let s = structure("n1", vec![n1, node("n2", Some("a2"), None)]);
        assert_eq!(linear_episodes(&s), Err(StoryPackBlocker::Branching));
    }

    #[test]
    fn any_node_without_audio_blocks_the_whole_story() {
        let s = structure(
            "n1",
            vec![
                node("n1", Some("a1"), Some("i1")),
                node("n2", None, Some("i2")),
            ],
        );
        assert_eq!(linear_episodes(&s), Err(StoryPackBlocker::MissingAudio));
        let s = structure("n1", vec![node("n1", Some(""), None)]);
        assert_eq!(linear_episodes(&s), Err(StoryPackBlocker::MissingAudio));
    }

    #[test]
    fn episodes_come_start_first_then_in_node_order_with_optional_images() {
        let s = structure(
            "n2",
            vec![
                node("n1", Some("a1"), Some("i1")),
                node("n2", Some("a2"), None),
                node("n3", Some("a3"), Some("")),
            ],
        );
        let episodes = linear_episodes(&s).expect("linear");
        assert_eq!(
            episodes.iter().map(|e| e.node_id).collect::<Vec<_>>(),
            vec!["n2", "n1", "n3"]
        );
        assert_eq!(episodes[0].audio_asset_id, "a2");
        assert_eq!(episodes[0].image_asset_id, None);
        assert_eq!(episodes[1].image_asset_id, Some("i1"));
        assert_eq!(episodes[2].image_asset_id, None, "an empty id is no image");
        assert_eq!(episodes[1].label, "label n1");
    }

    // ===== synthesize_sequential_pack =====

    fn assets(n: usize) -> Vec<EpisodeAssets> {
        (1..=n)
            .map(|i| EpisodeAssets {
                audio_ref: format!("{i:0>56}aaaa{i:04}.mp3"),
                image_ref: (i != 2).then(|| format!("{i:0>56}bbbb{i:04}.png")),
            })
            .collect()
    }

    const PACK: &str = "01a06ed9-2040-77c1-9e03-b8f429f4e954";

    #[test]
    fn the_cover_is_the_entry_and_carries_the_first_episode_media() {
        let pack = synthesize_sequential_pack(PACK, &assets(3));
        assert_eq!(pack.stage_nodes.len(), 4);
        assert_eq!(pack.action_nodes.len(), 3);
        let cover = &pack.stage_nodes[0];
        assert_eq!(cover.uuid, PACK, "pack uuid = story id");
        assert!(cover.square_one);
        assert_eq!(cover.image.as_deref(), assets(3)[0].image_ref.as_deref());
        assert_eq!(
            cover.audio.as_deref(),
            Some(assets(3)[0].audio_ref.as_str())
        );
        let cs = cover.control_settings;
        assert!(
            (cs.wheel, cs.ok, cs.home, cs.pause, cs.autoplay) == (true, true, false, false, false)
        );
        let ok = cover.ok_transition.as_ref().expect("cover ok");
        assert_eq!(ok.action_node, pack.action_nodes[0].id);
        assert_eq!(ok.option_index, 0);
        assert!(cover.home_transition.is_none());
        assert!(pack.stage_nodes.iter().skip(1).all(|s| !s.square_one));
    }

    #[test]
    fn episodes_are_autoplay_stages_chained_in_order_and_the_last_one_ends() {
        let pack = synthesize_sequential_pack(PACK, &assets(3));
        for (index, stage) in pack.stage_nodes.iter().skip(1).enumerate() {
            let cs = stage.control_settings;
            assert!(
                (cs.wheel, cs.ok, cs.home, cs.pause, cs.autoplay)
                    == (false, false, true, true, true),
                "episode {index} controls"
            );
            assert_eq!(
                stage.audio.as_deref(),
                Some(assets(3)[index].audio_ref.as_str())
            );
            assert_eq!(stage.image, assets(3)[index].image_ref);
            assert!(stage.home_transition.is_none(), "home = pack selection");
            // Its own single-option action node makes it a transition target.
            let action = &pack.action_nodes[index];
            assert_eq!(action.options, vec![stage.uuid.clone()]);
            match stage.ok_transition.as_ref() {
                Some(t) => {
                    assert!(index + 1 < 3, "only non-last episodes chain");
                    assert_eq!(t.action_node, pack.action_nodes[index + 1].id);
                    assert_eq!(t.option_index, 0);
                }
                None => assert_eq!(index, 2, "only the last episode ends"),
            }
        }
        // Every uuid / id is unique.
        let mut ids: Vec<&str> = pack.stage_nodes.iter().map(|s| s.uuid.as_str()).collect();
        ids.extend(pack.action_nodes.iter().map(|a| a.id.as_str()));
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn the_layout_transcodes_into_consistent_device_indices() {
        let pack = synthesize_sequential_pack(PACK, &assets(3));
        let out = transcode_pack(&pack).expect("transcode");
        // li: one option per action, in action order = episode stages 1..=3.
        let li: Vec<i32> = out
            .li
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(li, vec![1, 2, 3]);
        // Assets: 2 images (episode 2 has none), 3 audios, first-appearance
        // order — the cover reuses episode 1's media, so no duplicate.
        assert_eq!(out.images.len(), 2);
        assert_eq!(out.audios.len(), 3);
        // Cover record: ok → li offset 0, 1 option, option 0; home absent.
        let rec = |i: usize| &out.ni[512 + i * 44..512 + (i + 1) * 44];
        let field = |r: &[u8], o: usize| i32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
        assert_eq!(
            (field(rec(0), 8), field(rec(0), 12), field(rec(0), 16)),
            (0, 1, 0)
        );
        assert_eq!(
            (field(rec(0), 20), field(rec(0), 24), field(rec(0), 28)),
            (-1, -1, -1)
        );
        // Episode 1 → action 2 (offset 1); episode 3 → none.
        assert_eq!(
            (field(rec(1), 8), field(rec(1), 12), field(rec(1), 16)),
            (1, 1, 0)
        );
        assert_eq!(
            (field(rec(3), 8), field(rec(3), 12), field(rec(3), 16)),
            (-1, -1, -1)
        );
        // Episode 2 has no image (-1) but an audio.
        assert_eq!(field(rec(2), 0), -1);
        assert_eq!(field(rec(2), 4), 1);
    }

    #[test]
    fn a_single_episode_makes_a_cover_and_one_ending_stage() {
        let pack = synthesize_sequential_pack(PACK, &assets(1));
        assert_eq!(pack.stage_nodes.len(), 2);
        assert_eq!(pack.action_nodes.len(), 1);
        assert!(pack.stage_nodes[1].ok_transition.is_none());
        assert!(transcode_pack(&pack).is_ok());
    }
}
