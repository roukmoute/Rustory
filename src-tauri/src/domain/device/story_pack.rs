//! Synthesize a STUdio-format pack from a LIBRARY story — the bridge that
//! lets a story created in Rustory (from a web page, an RSS feed, a folder
//! or the editor — anything without a retained source `.zip`) reach a Lunii
//! V3 through the proven send engine (`transcode_pack` → device
//! normalization → assembly → write).
//!
//! PURE steps, all free of I/O:
//!
//! 1. [`linear_episodes`] — reads the canonical structure as an ORDERED
//!    episode list (the start node first, then the others in node order)
//!    and decides whether the story can be sent at all: every node must
//!    carry an audio (the device plays nothing else) and none may offer
//!    an option (a story with choices is not an episode list — it has its
//!    own layout, step 4, and its own reading, [`choice_layout`]). The
//!    refusal reasons are a closed set ([`StoryPackBlocker`]) the library
//!    card surfaces BEFORE any click.
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
//! 3. [`synthesize_menu_pack`] — the OTHER layout ([`StoryLayout::Menu`]):
//!    the child picks an episode on the wheel, like an official pack. Again
//!    the STUdio "menu" conventions: the cover leads into a QUESTION stage
//!    (autoplay only — the spoken question, e.g. « Quelle histoire veux-tu
//!    écouter ? ») whose OK transition enters the options action node; one
//!    OPTION stage per episode (wheel + OK + home; its image and its SPOKEN
//!    TITLE), whose OK leads to the episode's story stage; every story stage
//!    ends (OK) and homes back to the WHEEL POSITIONED ON THAT EPISODE (the
//!    options action node at the episode's own index — the child hears the
//!    episode's title again and scrolls on to the next one, like an
//!    official pack), never back to the first episode. With
//!    [`MenuPackAssets::auto_continue`], the end of an episode chains
//!    straight into the next one instead (the last episode still returns
//!    to the wheel, on itself); home keeps returning to the wheel. Those
//!    spoken announcements are audio assets the application synthesizes
//!    beforehand — [`menu_blocker`] refuses the layout while any is missing.
//!
//! 4. [`synthesize_choice_pack`] — a story WITH CHOICES (any node offering
//!    an option, read by [`choice_layout`]) is laid out by its OWN GRAPH, the
//!    exact inverse of the reading the structured-archive creation makes of
//!    a pack: every node is a stage, and the options of a node are the
//!    action node its OK transition enters — one option, its destination
//!    comes next; several, they are the entries of a WHEEL. What the
//!    canonical graph does not carry is settled by the same STUdio
//!    conventions as above: the start node is the cover; an entry of a wheel
//!    waits for the listener (wheel + OK + home); every other node plays
//!    through (autoplay, pause + home); a wheel is entered ON THE ENTRY THE
//!    LISTENER CAME THROUGH when that entry is one of its own (a story that
//!    returns to its wheel lands on its own title); HOME steps back to the
//!    last wheel the listener chose from — one level up from a wheel entry —
//!    and is the device's "back to the pack selection" when there is none.
//!    Nodes that offer the same options share one action node. A menu pack
//!    exported by step 3 and read back as a story lays out as that very
//!    graph again. The device send and the archive export write this SAME
//!    graph; they differ on one question only — may a stage be silent?
//!    ([`SilentNodes`]: a device plays nothing else, an archive may carry a
//!    picture alone).
//!
//! The pack UUID is the cover stage's uuid (see the send engine's
//! `pack_entry_uuid`): the caller passes the STORY id, a canonical lowercase
//! UUID, so re-sending a story REPLACES its pack on the device rather than
//! adding a copy.

use std::collections::{HashMap, HashSet};

use crate::domain::story::CanonicalStructure;

use super::library::is_canonical_pack_uuid;
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
    /// A node offers an option: the story is not an episode list. It is
    /// laid out by its own graph instead ([`choice_layout`],
    /// [`synthesize_choice_pack`]) — this is the reason the EPISODE reading
    /// gives, never a reason the story cannot become a pack.
    Branching,
    /// A node has no audio: the device has nothing to play for it.
    MissingAudio,
    /// The menu layout lacks a spoken announcement (the question, or an
    /// episode's spoken title) — they must be generated first.
    MissingAnnouncements,
}

impl StoryPackBlocker {
    /// Stable wire / diagnostic tag. Closed set.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Malformed => "malformed",
            Self::Branching => "branching",
            Self::MissingAudio => "missing_audio",
            Self::MissingAnnouncements => "missing_announcements",
        }
    }
}

/// How a story is laid out on the device. Persisted as a stable tag beside
/// the canonical structure (`story_layouts.layout`); absent = sequential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoryLayout {
    /// Episodes chained in order — the child presses OK once.
    #[default]
    Sequential,
    /// A spoken question, then the episodes on the wheel — the child picks.
    Menu,
}

impl StoryLayout {
    /// Stable persisted / wire tag.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Menu => "menu",
        }
    }

    /// Parse a persisted / wire tag. `None` for anything else.
    pub fn parse(tag: &str) -> Option<Self> {
        match tag {
            "sequential" => Some(Self::Sequential),
            "menu" => Some(Self::Menu),
            _ => None,
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

/// Stage uuid of episode `index` (0-based) — the story stage that plays it.
/// The ONE definition both layouts and [`stage_names`] share, so a name can
/// never be keyed by an id the synthesis did not produce.
fn episode_stage_uuid(pack_uuid: &str, index: usize) -> String {
    format!("{pack_uuid}-episode-{}", index + 1)
}

/// Id of the single-option action node that makes episode `index` a
/// transition target.
fn episode_action_id(pack_uuid: &str, index: usize) -> String {
    format!("{pack_uuid}-episode-{}-action", index + 1)
}

/// Stage uuid of the menu layout's spoken question.
fn question_stage_uuid(pack_uuid: &str) -> String {
    format!("{pack_uuid}-question")
}

/// Stage uuid of the menu layout's wheel entry for episode `index`.
fn option_stage_uuid(pack_uuid: &str, index: usize) -> String {
    format!("{pack_uuid}-option-{}", index + 1)
}

/// Name of the menu layout's question stage, as a shared archive shows it.
pub const MENU_QUESTION_STAGE_NAME: &str = "Question";

/// The name of episode `index`: its own label, or a numbered fallback when
/// the story left it blank (an unnamed stage would read as a hole in every
/// tool that lists the pack's stages).
fn episode_stage_name(label: &str, index: usize) -> String {
    let label = label.trim();
    if label.is_empty() {
        format!("Épisode {}", index + 1)
    } else {
        label.to_string()
    }
}

/// Human-readable NAMES of the stages of a synthesized pack, keyed by stage
/// uuid — what a shared archive carries next to the graph so another tool
/// (or another library) lists the cover, the episodes and the menu entries
/// by name instead of by opaque id. The device never reads them.
///
/// `episode_labels` are the episodes' labels in playback order (the order
/// [`linear_episodes`] returns); `title` names the cover. Every stage the
/// matching synthesis produces gets exactly one entry: the cover (the story
/// title), each episode (its label, numbered when blank) and, for the menu
/// layout, the question and one wheel entry per episode.
pub fn stage_names(
    layout: StoryLayout,
    pack_uuid: &str,
    title: &str,
    episode_labels: &[&str],
) -> HashMap<String, String> {
    let mut names = HashMap::with_capacity(2 + 2 * episode_labels.len());
    names.insert(pack_uuid.to_string(), title.to_string());
    if layout == StoryLayout::Menu {
        names.insert(
            question_stage_uuid(pack_uuid),
            MENU_QUESTION_STAGE_NAME.to_string(),
        );
    }
    for (index, label) in episode_labels.iter().enumerate() {
        let name = episode_stage_name(label, index);
        if layout == StoryLayout::Menu {
            names.insert(
                option_stage_uuid(pack_uuid, index),
                format!("Choix : {name}"),
            );
        }
        names.insert(episode_stage_uuid(pack_uuid, index), name);
    }
    names
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
    let episode_uuid = |index: usize| episode_stage_uuid(pack_uuid, index);
    let action_id = |index: usize| episode_action_id(pack_uuid, index);
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

/// Why the MENU layout cannot be sent yet, given the linear episodes and
/// which announcements exist: the spoken question and one spoken title per
/// episode are required (the spoken series title is optional — the cover
/// falls back to the first episode's spoken title).
pub fn menu_blocker(
    episodes: &[LinearEpisode<'_>],
    has_question: bool,
    has_prompt: impl Fn(&str) -> bool,
) -> Option<StoryPackBlocker> {
    if !has_question || episodes.iter().any(|e| !has_prompt(e.node_id)) {
        return Some(StoryPackBlocker::MissingAnnouncements);
    }
    None
}

/// One episode of the menu layout: its media plus its SPOKEN TITLE (the
/// wheel prompt), all as pack asset references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEpisode {
    pub audio_ref: String,
    pub image_ref: Option<String>,
    pub prompt_ref: String,
}

/// Everything the menu layout carries: the optional spoken series title (the
/// cover prompt), the spoken question, the episodes in wheel order, and
/// whether an episode's end chains into the next one (`auto_continue`)
/// rather than returning to the wheel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPackAssets {
    pub title_ref: Option<String>,
    pub question_ref: String,
    pub episodes: Vec<MenuEpisode>,
    pub auto_continue: bool,
}

/// Lay `assets` out as a menu STUdio pack whose entry uuid is `pack_uuid`.
/// See the module doc for the stage/action layout.
pub fn synthesize_menu_pack(pack_uuid: &str, assets: &MenuPackAssets) -> StudioStoryPack {
    let cover_controls = StudioControlSettings {
        wheel: true,
        ok: true,
        home: false,
        pause: false,
        autoplay: false,
    };
    let question_controls = StudioControlSettings {
        wheel: false,
        ok: false,
        home: false,
        pause: false,
        autoplay: true,
    };
    let option_controls = StudioControlSettings {
        wheel: true,
        ok: true,
        home: true,
        pause: false,
        autoplay: false,
    };
    let story_controls = StudioControlSettings {
        wheel: false,
        ok: false,
        home: true,
        pause: true,
        autoplay: true,
    };
    let question_uuid = question_stage_uuid(pack_uuid);
    let question_action = format!("{pack_uuid}-question-action");
    let options_action = format!("{pack_uuid}-options-action");
    let option_uuid = |index: usize| option_stage_uuid(pack_uuid, index);
    let episode_uuid = |index: usize| episode_stage_uuid(pack_uuid, index);
    let episode_action = |index: usize| episode_action_id(pack_uuid, index);
    let to = |action: &str, option_index: i32| {
        Some(StudioTransition {
            action_node: action.to_string(),
            option_index,
        })
    };

    let first = assets.episodes.first();
    let mut stage_nodes = Vec::with_capacity(2 + 2 * assets.episodes.len());
    stage_nodes.push(StudioStageNode {
        uuid: pack_uuid.to_string(),
        square_one: true,
        image: first.and_then(|e| e.image_ref.clone()),
        audio: assets
            .title_ref
            .clone()
            .or_else(|| first.map(|e| e.prompt_ref.clone())),
        ok_transition: to(&question_action, 0),
        home_transition: None,
        control_settings: cover_controls,
    });
    stage_nodes.push(StudioStageNode {
        uuid: question_uuid.clone(),
        square_one: false,
        image: None,
        audio: Some(assets.question_ref.clone()),
        ok_transition: to(&options_action, 0),
        home_transition: None,
        control_settings: question_controls,
    });
    for (index, episode) in assets.episodes.iter().enumerate() {
        stage_nodes.push(StudioStageNode {
            uuid: option_uuid(index),
            square_one: false,
            image: episode.image_ref.clone(),
            audio: Some(episode.prompt_ref.clone()),
            ok_transition: to(&episode_action(index), 0),
            // Home from the wheel: the device's own "back to the pack
            // selection" (STUdio: a menu entered from the cover).
            home_transition: None,
            control_settings: option_controls,
        });
    }
    let last = assets.episodes.len().saturating_sub(1);
    for (index, episode) in assets.episodes.iter().enumerate() {
        // Back to the wheel ON THIS EPISODE: the option stage plays the
        // episode's spoken title and the child scrolls on from there.
        let back_to_wheel = to(&options_action, index as i32);
        stage_nodes.push(StudioStageNode {
            uuid: episode_uuid(index),
            square_one: false,
            image: episode.image_ref.clone(),
            audio: Some(episode.audio_ref.clone()),
            // The end of an episode: the next one when chaining is on (the
            // last episode has none to chain into), else the wheel on the
            // episode just heard. Home always returns to that wheel spot.
            ok_transition: if assets.auto_continue && index < last {
                to(&episode_action(index + 1), 0)
            } else {
                back_to_wheel.clone()
            },
            home_transition: back_to_wheel,
            control_settings: story_controls,
        });
    }

    let mut action_nodes = Vec::with_capacity(2 + assets.episodes.len());
    action_nodes.push(StudioActionNode {
        id: question_action.clone(),
        options: vec![question_uuid],
    });
    action_nodes.push(StudioActionNode {
        id: options_action,
        options: (0..assets.episodes.len()).map(option_uuid).collect(),
    });
    action_nodes.extend((0..assets.episodes.len()).map(|index| StudioActionNode {
        id: episode_action(index),
        options: vec![episode_uuid(index)],
    }));

    StudioStoryPack {
        version: 1,
        night_mode_available: false,
        stage_nodes,
        action_nodes,
    }
}

// ===== Stories with choices =====

/// One node of a story WITH CHOICES as the pack layout reads it: the node it
/// comes from, its media ASSET IDS (the application layer resolves them to
/// stored files) and the nodes its options lead to, in option order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceNode<'a> {
    pub node_id: &'a str,
    pub label: &'a str,
    pub audio_asset_id: Option<&'a str>,
    pub image_asset_id: Option<&'a str>,
    /// Destination node ids of the node's options, in option order.
    pub destinations: Vec<&'a str>,
}

/// Why a story with choices cannot be laid out as a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceLayoutBlocker {
    /// The structure itself is unusable — [`StoryPackBlocker::Empty`] or
    /// [`StoryPackBlocker::Malformed`], the reasons every layout shares.
    Structure(StoryPackBlocker),
    /// An option of node `node_id` leads nowhere: it is not linked yet, or
    /// its destination is not a node of the story. A wheel entry must be a
    /// stage, so the pack cannot carry it.
    OptionWithoutDestination { node_id: String },
    /// Node `node_id` carries no sound, and the layout was asked to refuse
    /// a silent stage ([`SilentNodes::Refused`]): the device has nothing to
    /// play for it — the rule every episode follows
    /// ([`StoryPackBlocker::MissingAudio`]).
    NodeWithoutAudio { node_id: String },
}

/// What a layout does with a node WITHOUT SOUND in a story with choices.
/// The graph is the same either way; the two gestures that write it only
/// answer differently to "may a stage be silent?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilentNodes {
    /// Refused — the device send: a device plays nothing else, so a pack
    /// Rustory lays out for one never holds a stage it would stay mute on.
    Refused,
    /// Carried as silent stages — the archive export: a pack stage may be a
    /// picture alone (or nothing at all), and a story created from a pack
    /// goes back out with exactly the stages it had.
    Carried,
}

/// A story with choices read for the pack it lays out as: the pack identity
/// ([`choice_pack_uuid`]) and its nodes, in [`choice_nodes`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceLayout<'a> {
    pub pack_uuid: String,
    pub nodes: Vec<ChoiceNode<'a>>,
}

/// Read `structure` — a story with choices — for the pack it lays out as,
/// or say why it does not. The ONE verdict the library card (pre-click) and
/// the plan (at the gesture) share, so they can never disagree: the graph
/// first (a usable structure, every option a destination, a pack identity),
/// then — when `silent_nodes` refuses them — a sound on every node.
pub fn choice_layout<'a>(
    story_id: &str,
    structure: &'a CanonicalStructure,
    silent_nodes: SilentNodes,
) -> Result<ChoiceLayout<'a>, ChoiceLayoutBlocker> {
    let nodes = choice_nodes(structure)?;
    let pack_uuid = choice_pack_uuid(story_id, &nodes)
        .ok_or(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Malformed))?;
    if silent_nodes == SilentNodes::Refused {
        if let Some(silent) = nodes.iter().find(|node| node.audio_asset_id.is_none()) {
            return Err(ChoiceLayoutBlocker::NodeWithoutAudio {
                node_id: silent.node_id.to_string(),
            });
        }
    }
    Ok(ChoiceLayout { pack_uuid, nodes })
}

/// Whether any node of `structure` offers an option. Such a story is laid
/// out by its own graph ([`choice_layout`]), never as an episode list.
pub fn offers_choices(structure: &CanonicalStructure) -> bool {
    structure.nodes.iter().any(|node| !node.options.is_empty())
}

/// Read `structure` as the node graph of a story with choices — the start
/// node first, then the others in their node order — or say why it does not
/// lay out as a pack.
///
/// Unlike [`linear_episodes`], a node may carry no audio: a pack stage may
/// be a picture alone (or nothing at all), and a story created from a pack
/// keeps exactly the media its stages had.
pub fn choice_nodes(
    structure: &CanonicalStructure,
) -> Result<Vec<ChoiceNode<'_>>, ChoiceLayoutBlocker> {
    if structure.nodes.is_empty() {
        return Err(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Empty));
    }
    let start = structure
        .nodes
        .iter()
        .position(|node| node.id == structure.start_node_id)
        .ok_or(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Malformed))?;
    // Every node becomes a stage and a stage is found by its id: a blank id,
    // or two ids a reader would take for the same one, is not a graph.
    let mut folded_ids: HashSet<String> = HashSet::with_capacity(structure.nodes.len());
    let mut ids: HashSet<&str> = HashSet::with_capacity(structure.nodes.len());
    for node in &structure.nodes {
        if node.id.trim().is_empty() || !folded_ids.insert(node.id.to_ascii_lowercase()) {
            return Err(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Malformed));
        }
        ids.insert(node.id.as_str());
    }
    // The start node first, then the others in their node order.
    let ordered = std::iter::once(&structure.nodes[start]).chain(
        structure
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != start)
            .map(|(_, node)| node),
    );
    let mut nodes = Vec::with_capacity(structure.nodes.len());
    for node in ordered {
        let mut destinations = Vec::with_capacity(node.options.len());
        for option in &node.options {
            match option.target.as_deref() {
                Some(target) if ids.contains(target) => destinations.push(target),
                _ => {
                    return Err(ChoiceLayoutBlocker::OptionWithoutDestination {
                        node_id: node.id.clone(),
                    })
                }
            }
        }
        nodes.push(ChoiceNode {
            node_id: &node.id,
            label: &node.label,
            audio_asset_id: node.audio_asset_id.as_deref().filter(|id| !id.is_empty()),
            image_asset_id: node.image_asset_id.as_deref().filter(|id| !id.is_empty()),
            destinations,
        });
    }
    Ok(nodes)
}

/// The identity of the pack a story with choices lays out as (the uuid of
/// its entry stage), given its nodes in [`choice_nodes`] order.
///
/// A start node whose own id is a well-formed pack UUID keeps it: a story
/// created from a pack is laid out as THAT pack again, under the identity
/// its source archive carried. Any other story is filed under its story id,
/// like the other layouts. `None` when the story id is needed but another
/// node already carries it — two stages cannot share an identity.
pub fn choice_pack_uuid(story_id: &str, nodes: &[ChoiceNode<'_>]) -> Option<String> {
    let start = nodes.first()?;
    if is_canonical_pack_uuid(&start.node_id.to_ascii_lowercase()) {
        return Some(start.node_id.to_string());
    }
    if nodes
        .iter()
        .skip(1)
        .any(|node| node.node_id.eq_ignore_ascii_case(story_id))
    {
        return None;
    }
    Some(story_id.to_string())
}

/// Stage uuid of node `index` of a story with choices: the pack identity for
/// the start node, the node's own id for every other one. The ONE definition
/// [`synthesize_choice_pack`] and [`choice_stage_names`] share.
fn choice_stage_uuid(pack_uuid: &str, nodes: &[ChoiceNode<'_>], index: usize) -> String {
    if index == 0 {
        pack_uuid.to_string()
    } else {
        nodes[index].node_id.to_string()
    }
}

/// Human-readable NAMES of the stages [`synthesize_choice_pack`] produces,
/// keyed by stage uuid: each node's own label, else the story `title` for
/// the start node and a numbered `Nœud N` for any other (an unnamed stage
/// would read as a hole in every tool that lists the pack's stages).
pub fn choice_stage_names(
    pack_uuid: &str,
    title: &str,
    nodes: &[ChoiceNode<'_>],
) -> HashMap<String, String> {
    let mut names = HashMap::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let label = node.label.trim();
        let name = if !label.is_empty() {
            label.to_string()
        } else if index == 0 {
            title.to_string()
        } else {
            format!("Nœud {}", index + 1)
        };
        names.insert(choice_stage_uuid(pack_uuid, nodes, index), name);
    }
    names
}

/// The media of one node of a story with choices, as PACK ASSET REFERENCES.
/// Either may be absent: the stage then shows, or plays, nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceNodeAssets {
    pub audio_ref: Option<String>,
    pub image_ref: Option<String>,
}

/// Lay a story with choices out as a STUdio pack whose entry uuid is
/// `pack_uuid`: `nodes` in [`choice_nodes`] order, `assets` alongside (one
/// per node). See the module doc for the stage/action layout.
pub fn synthesize_choice_pack(
    pack_uuid: &str,
    nodes: &[ChoiceNode<'_>],
    assets: &[ChoiceNodeAssets],
) -> StudioStoryPack {
    let cover_controls = StudioControlSettings {
        wheel: true,
        ok: true,
        home: false,
        pause: false,
        autoplay: false,
    };
    let wheel_entry_controls = StudioControlSettings {
        wheel: true,
        ok: true,
        home: true,
        pause: false,
        autoplay: false,
    };
    let played_controls = StudioControlSettings {
        wheel: false,
        ok: false,
        home: true,
        pause: true,
        autoplay: true,
    };

    // The options of every node as node indices. `choice_nodes` only lets
    // existing destinations through; anything else is dropped, never trusted.
    let index_of: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.node_id, index))
        .collect();
    let destinations: Vec<Vec<usize>> = nodes
        .iter()
        .map(|node| {
            node.destinations
                .iter()
                .filter_map(|destination| index_of.get(destination).copied())
                .collect()
        })
        .collect();

    // One action node per DISTINCT option list, in first-use order: nodes
    // that offer the same options share the list, as the tools that write
    // the format do (every story returning to one wheel points at it).
    let mut action_of: HashMap<&[usize], usize> = HashMap::new();
    let mut action_lists: Vec<&[usize]> = Vec::new();
    for list in destinations.iter().filter(|list| !list.is_empty()) {
        action_of.entry(list.as_slice()).or_insert_with(|| {
            action_lists.push(list.as_slice());
            action_lists.len() - 1
        });
    }
    let action_id = |action: usize| format!("{pack_uuid}-options-{}", action + 1);
    let transition = |action: usize, option_index: usize| StudioTransition {
        action_node: action_id(action),
        option_index: option_index as i32,
    };

    // A node offered beside at least one other is an entry of a WHEEL.
    let mut on_wheel = vec![false; nodes.len()];
    for list in destinations.iter().filter(|list| list.len() >= 2) {
        for &destination in list {
            on_wheel[destination] = true;
        }
    }

    // How the listener first reaches each node: a breadth-first walk from
    // the start node, options in their order.
    let mut parent: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut reached = vec![false; nodes.len()];
    let mut walk: Vec<usize> = Vec::with_capacity(nodes.len());
    if !nodes.is_empty() {
        reached[0] = true;
        walk.push(0);
    }
    let mut head = 0;
    while head < walk.len() {
        let current = walk[head];
        head += 1;
        for &destination in &destinations[current] {
            if !reached[destination] {
                reached[destination] = true;
                parent[destination] = Some(current);
                walk.push(destination);
            }
        }
    }

    // The wheel spot each node was reached THROUGH: the last wheel the
    // listener chose from on the way, positioned on the entry chosen. A node
    // reached without a choice inherits its parent's spot.
    let mut wheel_spot: Vec<Option<(usize, usize)>> = vec![None; nodes.len()];
    for &node in walk.iter().skip(1) {
        let Some(from) = parent[node] else { continue };
        let list = destinations[from].as_slice();
        wheel_spot[node] = if list.len() >= 2 {
            let position = list.iter().position(|&d| d == node).unwrap_or(0);
            action_of.get(list).map(|&action| (action, position))
        } else {
            wheel_spot[from]
        };
    }

    let stage_nodes = (0..nodes.len())
        .map(|node| {
            let list = destinations[node].as_slice();
            // OK (or the end of an autoplayed sound) enters the node's own
            // options. A wheel is entered ON THE ENTRY THE LISTENER CAME
            // THROUGH when it is one of them — a story that returns to its
            // wheel lands on its own title, not on the first one.
            let ok_transition = action_of.get(list).map(|&action| {
                let mut option_index = 0;
                if list.len() >= 2 {
                    let mut ancestor = parent[node];
                    while let Some(through) = ancestor {
                        if let Some(position) = list.iter().position(|&d| d == through) {
                            option_index = position;
                            break;
                        }
                        ancestor = parent[through];
                    }
                }
                transition(action, option_index)
            });
            // HOME steps back to the wheel spot the node's PARENT was
            // reached through: from a story, the wheel it was picked on;
            // from a wheel entry, one level up. No spot above = the
            // device's own "back to the pack selection".
            let home_transition = parent[node]
                .and_then(|from| wheel_spot[from])
                .map(|(action, option_index)| transition(action, option_index));
            let media = assets.get(node);
            StudioStageNode {
                uuid: choice_stage_uuid(pack_uuid, nodes, node),
                square_one: node == 0,
                image: media.and_then(|m| m.image_ref.clone()),
                audio: media.and_then(|m| m.audio_ref.clone()),
                ok_transition,
                home_transition,
                control_settings: if node == 0 {
                    cover_controls
                } else if on_wheel[node] {
                    wheel_entry_controls
                } else {
                    played_controls
                },
            }
        })
        .collect();
    let action_nodes = action_lists
        .iter()
        .enumerate()
        .map(|(action, list)| StudioActionNode {
            id: action_id(action),
            options: list
                .iter()
                .map(|&destination| choice_stage_uuid(pack_uuid, nodes, destination))
                .collect(),
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

    // ===== menu layout =====

    fn menu_assets(n: usize) -> MenuPackAssets {
        MenuPackAssets {
            title_ref: Some(format!("{}.wav", "t".repeat(56) + "7777aaaa")),
            question_ref: format!("{}.wav", "q".repeat(56) + "8888bbbb"),
            episodes: (1..=n)
                .map(|i| MenuEpisode {
                    audio_ref: format!("{i:0>56}aaaa{i:04}.mp3"),
                    image_ref: (i != 2).then(|| format!("{i:0>56}bbbb{i:04}.png")),
                    prompt_ref: format!("{i:0>56}cccc{i:04}.wav"),
                })
                .collect(),
            auto_continue: false,
        }
    }

    #[test]
    fn story_layout_tags_round_trip_and_default_to_sequential() {
        assert_eq!(StoryLayout::default(), StoryLayout::Sequential);
        for layout in [StoryLayout::Sequential, StoryLayout::Menu] {
            assert_eq!(StoryLayout::parse(layout.as_str()), Some(layout));
        }
        assert_eq!(StoryLayout::parse("carousel"), None);
    }

    #[test]
    fn the_menu_layout_is_blocked_until_every_announcement_exists() {
        let s = structure(
            "n1",
            vec![node("n1", Some("a1"), None), node("n2", Some("a2"), None)],
        );
        let episodes = linear_episodes(&s).expect("linear");
        assert_eq!(
            menu_blocker(&episodes, false, |_| true),
            Some(StoryPackBlocker::MissingAnnouncements),
            "no spoken question"
        );
        assert_eq!(
            menu_blocker(&episodes, true, |id| id == "n1"),
            Some(StoryPackBlocker::MissingAnnouncements),
            "an episode without its spoken title"
        );
        assert_eq!(menu_blocker(&episodes, true, |_| true), None);
        assert_eq!(
            StoryPackBlocker::MissingAnnouncements.diagnostic_tag(),
            "missing_announcements"
        );
    }

    #[test]
    fn the_menu_pack_follows_the_studio_menu_conventions() {
        let assets = menu_assets(3);
        let pack = synthesize_menu_pack(PACK, &assets);
        // cover + question + 3 options + 3 stories; question + options + 3 story actions.
        assert_eq!(pack.stage_nodes.len(), 8);
        assert_eq!(pack.action_nodes.len(), 5);

        let cover = &pack.stage_nodes[0];
        assert_eq!(cover.uuid, PACK);
        assert!(cover.square_one);
        assert_eq!(
            cover.audio, assets.title_ref,
            "spoken series title on the wheel"
        );
        assert_eq!(cover.image, assets.episodes[0].image_ref);
        let cs = cover.control_settings;
        assert!(
            (cs.wheel, cs.ok, cs.home, cs.pause, cs.autoplay) == (true, true, false, false, false)
        );
        assert_eq!(
            cover.ok_transition.as_ref().unwrap().action_node,
            pack.action_nodes[0].id
        );

        let question = &pack.stage_nodes[1];
        assert_eq!(
            question.audio.as_deref(),
            Some(assets.question_ref.as_str())
        );
        assert_eq!(question.image, None);
        let cs = question.control_settings;
        assert!(
            (cs.wheel, cs.ok, cs.home, cs.pause, cs.autoplay) == (false, false, false, false, true)
        );
        assert_eq!(
            question.ok_transition.as_ref().unwrap().action_node,
            pack.action_nodes[1].id
        );
        assert_eq!(question.ok_transition.as_ref().unwrap().option_index, 0);
        assert_eq!(pack.action_nodes[0].options, vec![question.uuid.clone()]);

        // The options action lists the option stages in wheel order.
        let option_uuids: Vec<String> = pack.stage_nodes[2..5]
            .iter()
            .map(|s| s.uuid.clone())
            .collect();
        assert_eq!(pack.action_nodes[1].options, option_uuids);
        for (index, option) in pack.stage_nodes[2..5].iter().enumerate() {
            assert_eq!(
                option.audio.as_deref(),
                Some(assets.episodes[index].prompt_ref.as_str())
            );
            assert_eq!(option.image, assets.episodes[index].image_ref);
            let cs = option.control_settings;
            assert!(
                (cs.wheel, cs.ok, cs.home, cs.pause, cs.autoplay)
                    == (true, true, true, false, false)
            );
            assert!(option.home_transition.is_none(), "home = pack selection");
            let ok = option.ok_transition.as_ref().unwrap();
            assert_eq!(ok.action_node, pack.action_nodes[2 + index].id);
            assert_eq!(
                pack.action_nodes[2 + index].options,
                vec![pack.stage_nodes[5 + index].uuid.clone()]
            );
        }
        for (index, story) in pack.stage_nodes[5..8].iter().enumerate() {
            assert_eq!(
                story.audio.as_deref(),
                Some(assets.episodes[index].audio_ref.as_str())
            );
            let cs = story.control_settings;
            assert!(
                (cs.wheel, cs.ok, cs.home, cs.pause, cs.autoplay)
                    == (false, false, true, true, true)
            );
            // End and home both return to the WHEEL, positioned on this
            // very episode — never on the first one.
            for t in [story.ok_transition.as_ref(), story.home_transition.as_ref()] {
                let t = t.expect("back to the wheel");
                assert_eq!(t.action_node, pack.action_nodes[1].id);
                assert_eq!(t.option_index, index as i32);
            }
        }
        // Unique ids, and the whole graph transcodes.
        let mut ids: Vec<&str> = pack.stage_nodes.iter().map(|s| s.uuid.as_str()).collect();
        ids.extend(pack.action_nodes.iter().map(|a| a.id.as_str()));
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before);
        let out = transcode_pack(&pack).expect("transcode");
        let li: Vec<i32> = out
            .li
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(
            li,
            vec![1, 2, 3, 4, 5, 6, 7],
            "question, then options, then stories"
        );
        assert_eq!(
            out.audios.len(),
            1 + 1 + 3 + 3,
            "title + question + prompts + episodes"
        );
        assert_eq!(out.images.len(), 2);
    }

    #[test]
    fn with_auto_continue_an_episode_chains_into_the_next_and_the_last_returns_to_the_wheel() {
        let mut assets = menu_assets(3);
        assets.auto_continue = true;
        let pack = synthesize_menu_pack(PACK, &assets);
        let stories = &pack.stage_nodes[5..8];
        for (index, story) in stories.iter().enumerate() {
            let ok = story.ok_transition.as_ref().expect("end transition");
            if index < 2 {
                // Straight into the next episode's own action node.
                assert_eq!(ok.action_node, pack.action_nodes[2 + index + 1].id);
                assert_eq!(ok.option_index, 0);
            } else {
                // The last one has nothing to chain into: the wheel, on itself.
                assert_eq!(ok.action_node, pack.action_nodes[1].id);
                assert_eq!(ok.option_index, 2);
            }
            // Home never chains: back to the wheel on this episode.
            let home = story.home_transition.as_ref().expect("home transition");
            assert_eq!(home.action_node, pack.action_nodes[1].id);
            assert_eq!(home.option_index, index as i32);
        }
        // The wheel itself is untouched by the chaining.
        for (index, option) in pack.stage_nodes[2..5].iter().enumerate() {
            let ok = option.ok_transition.as_ref().unwrap();
            assert_eq!(ok.action_node, pack.action_nodes[2 + index].id);
        }
        transcode_pack(&pack).expect("the chained graph transcodes");

        // A single-episode series: chaining has nothing to do.
        let mut single = menu_assets(1);
        single.auto_continue = true;
        let pack = synthesize_menu_pack(PACK, &single);
        let ok = pack.stage_nodes[3].ok_transition.as_ref().unwrap();
        assert_eq!(ok.action_node, pack.action_nodes[1].id);
        assert_eq!(ok.option_index, 0);
    }

    // ===== stage names =====

    #[test]
    fn every_stage_of_a_sequential_pack_is_named_and_nothing_else() {
        let pack = synthesize_sequential_pack(PACK, &assets(3));
        let names = stage_names(
            StoryLayout::Sequential,
            PACK,
            "Les Bestioles",
            &["Le loup", "  ", "La pie"],
        );
        let uuids: std::collections::BTreeSet<&str> =
            pack.stage_nodes.iter().map(|s| s.uuid.as_str()).collect();
        let named: std::collections::BTreeSet<&str> = names.keys().map(String::as_str).collect();
        assert_eq!(named, uuids, "one name per synthesized stage");
        assert_eq!(names[PACK], "Les Bestioles", "the cover carries the title");
        assert_eq!(names[&pack.stage_nodes[1].uuid], "Le loup");
        assert_eq!(
            names[&pack.stage_nodes[2].uuid], "Épisode 2",
            "a blank label falls back to its position"
        );
        assert_eq!(names[&pack.stage_nodes[3].uuid], "La pie");
    }

    #[test]
    fn every_stage_of_a_menu_pack_is_named_and_nothing_else() {
        let pack = synthesize_menu_pack(PACK, &menu_assets(2));
        let names = stage_names(StoryLayout::Menu, PACK, "Série", &["Un", "Deux"]);
        let uuids: std::collections::BTreeSet<&str> =
            pack.stage_nodes.iter().map(|s| s.uuid.as_str()).collect();
        let named: std::collections::BTreeSet<&str> = names.keys().map(String::as_str).collect();
        assert_eq!(named, uuids, "one name per synthesized stage");
        assert_eq!(names[PACK], "Série");
        assert_eq!(names[&pack.stage_nodes[1].uuid], MENU_QUESTION_STAGE_NAME);
        // Wheel entries are told apart from the episodes they lead to.
        assert_eq!(names[&pack.stage_nodes[2].uuid], "Choix : Un");
        assert_eq!(names[&pack.stage_nodes[3].uuid], "Choix : Deux");
        assert_eq!(names[&pack.stage_nodes[4].uuid], "Un");
        assert_eq!(names[&pack.stage_nodes[5].uuid], "Deux");
    }

    #[test]
    fn without_a_spoken_series_title_the_cover_speaks_the_first_episode_title() {
        let mut assets = menu_assets(2);
        assets.title_ref = None;
        let pack = synthesize_menu_pack(PACK, &assets);
        assert_eq!(
            pack.stage_nodes[0].audio.as_deref(),
            Some(assets.episodes[0].prompt_ref.as_str())
        );
        assert!(transcode_pack(&pack).is_ok());
    }

    // ===== stories with choices =====

    /// `node` offering one option per destination, in order.
    fn offering(mut node: CanonicalNode, destinations: &[&str]) -> CanonicalNode {
        for destination in destinations {
            node.options.push(CanonicalOption {
                label: format!("vers {destination}"),
                target: Some((*destination).to_string()),
            });
        }
        node
    }

    /// One stage with its transitions RESOLVED to the option lists they
    /// enter — what two packs must agree on to be the same graph, whatever
    /// ids their action nodes carry.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ResolvedStage {
        uuid: String,
        image: Option<String>,
        audio: Option<String>,
        ok: Option<(Vec<String>, i32)>,
        home: Option<(Vec<String>, i32)>,
        /// wheel, ok, home, pause, autoplay.
        controls: (bool, bool, bool, bool, bool),
    }

    fn resolved(pack: &StudioStoryPack) -> Vec<ResolvedStage> {
        let entered = |transition: &Option<StudioTransition>| {
            transition.as_ref().map(|t| {
                let action = pack
                    .action_nodes
                    .iter()
                    .find(|action| action.id == t.action_node)
                    .expect("the action exists");
                (action.options.clone(), t.option_index)
            })
        };
        pack.stage_nodes
            .iter()
            .map(|stage| {
                let cs = stage.control_settings;
                ResolvedStage {
                    uuid: stage.uuid.clone(),
                    image: stage.image.clone(),
                    audio: stage.audio.clone(),
                    ok: entered(&stage.ok_transition),
                    home: entered(&stage.home_transition),
                    controls: (cs.wheel, cs.ok, cs.home, cs.pause, cs.autoplay),
                }
            })
            .collect()
    }

    /// The story the structured-archive creation makes of `pack`: one node
    /// per stage, in stage order, offering the targets of the stage's OK
    /// action. The media ids ARE the pack's references, so the same pack can
    /// be laid out again from the story alone.
    fn story_of_pack(pack: &StudioStoryPack) -> CanonicalStructure {
        let nodes = pack
            .stage_nodes
            .iter()
            .map(|stage| {
                let destinations: Vec<String> = stage
                    .ok_transition
                    .as_ref()
                    .map(|t| {
                        pack.action_nodes
                            .iter()
                            .find(|action| action.id == t.action_node)
                            .expect("the action exists")
                            .options
                            .clone()
                    })
                    .unwrap_or_default();
                CanonicalNode {
                    id: stage.uuid.clone(),
                    text: String::new(),
                    label: String::new(),
                    image_asset_id: stage.image.clone(),
                    audio_asset_id: stage.audio.clone(),
                    options: destinations
                        .into_iter()
                        .map(|target| CanonicalOption {
                            label: String::new(),
                            target: Some(target),
                        })
                        .collect(),
                }
            })
            .collect();
        structure(&pack.stage_nodes[0].uuid, nodes)
    }

    /// Lay `story` out as a pack for the story `story_id`, every media
    /// reference being the node's own asset id.
    fn laid_out(story_id: &str, story: &CanonicalStructure) -> StudioStoryPack {
        let nodes = choice_nodes(story).expect("a story with choices");
        let media: Vec<ChoiceNodeAssets> = nodes
            .iter()
            .map(|node| ChoiceNodeAssets {
                audio_ref: node.audio_asset_id.map(str::to_string),
                image_ref: node.image_asset_id.map(str::to_string),
            })
            .collect();
        let pack_uuid = choice_pack_uuid(story_id, &nodes).expect("an identity");
        synthesize_choice_pack(&pack_uuid, &nodes, &media)
    }

    const COVER: (bool, bool, bool, bool, bool) = (true, true, false, false, false);
    const WHEEL_ENTRY: (bool, bool, bool, bool, bool) = (true, true, true, false, false);
    const PLAYED: (bool, bool, bool, bool, bool) = (false, false, true, true, true);

    #[test]
    fn a_story_offers_choices_as_soon_as_one_node_has_an_option() {
        let plain = structure(
            "n1",
            vec![node("n1", Some("a1"), None), node("n2", Some("a2"), None)],
        );
        assert!(!offers_choices(&plain));
        let chained = structure(
            "n1",
            vec![
                offering(node("n1", Some("a1"), None), &["n2"]),
                node("n2", Some("a2"), None),
            ],
        );
        assert!(offers_choices(&chained));
        // An option that is not linked yet is an option all the same.
        let mut unlinked = node("n1", Some("a1"), None);
        unlinked.options.push(CanonicalOption {
            label: "plus tard".into(),
            target: None,
        });
        assert!(offers_choices(&structure("n1", vec![unlinked])));
    }

    #[test]
    fn choice_nodes_come_start_first_with_their_destinations_in_option_order() {
        let s = structure(
            "n2",
            vec![
                offering(node("n1", Some("a1"), Some("i1")), &["n3"]),
                offering(node("n2", None, Some("")), &["n3", "n1", "n3"]),
                node("n3", Some(""), None),
            ],
        );
        let nodes = choice_nodes(&s).expect("laid out");
        assert_eq!(
            nodes.iter().map(|n| n.node_id).collect::<Vec<_>>(),
            vec!["n2", "n1", "n3"]
        );
        assert_eq!(nodes[0].destinations, vec!["n3", "n1", "n3"]);
        assert_eq!(nodes[1].destinations, vec!["n3"]);
        assert!(nodes[2].destinations.is_empty());
        // A node may carry no sound, and a blank reference is no reference.
        assert_eq!(
            (nodes[0].audio_asset_id, nodes[0].image_asset_id),
            (None, None)
        );
        assert_eq!(
            (nodes[1].audio_asset_id, nodes[1].image_asset_id),
            (Some("a1"), Some("i1"))
        );
        assert_eq!(nodes[2].audio_asset_id, None);
        assert_eq!(nodes[1].label, "label n1");
    }

    #[test]
    fn an_option_that_leads_nowhere_blocks_the_layout_and_names_its_node() {
        // Not linked yet.
        let mut unlinked = node("n2", Some("a2"), None);
        unlinked.options.push(CanonicalOption {
            label: "plus tard".into(),
            target: None,
        });
        let s = structure(
            "n1",
            vec![offering(node("n1", Some("a1"), None), &["n2"]), unlinked],
        );
        assert_eq!(
            choice_nodes(&s),
            Err(ChoiceLayoutBlocker::OptionWithoutDestination {
                node_id: "n2".into()
            })
        );
        // Linked to a node that is gone.
        let s = structure(
            "n1",
            vec![offering(node("n1", Some("a1"), None), &["ghost"])],
        );
        assert_eq!(
            choice_nodes(&s),
            Err(ChoiceLayoutBlocker::OptionWithoutDestination {
                node_id: "n1".into()
            })
        );
    }

    #[test]
    fn a_structure_that_is_not_a_graph_blocks_the_layout() {
        assert_eq!(
            choice_nodes(&structure("n1", vec![])),
            Err(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Empty))
        );
        let no_start = structure("nope", vec![offering(node("n1", None, None), &["n1"])]);
        assert_eq!(
            choice_nodes(&no_start),
            Err(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Malformed))
        );
        // Two nodes a reader would take for the same stage.
        let twins = structure(
            "n1",
            vec![
                offering(node("n1", None, None), &["N1"]),
                node("N1", None, None),
            ],
        );
        assert_eq!(
            choice_nodes(&twins),
            Err(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Malformed))
        );
        let blank = structure(
            "n1",
            vec![
                offering(node("n1", None, None), &["n1"]),
                node(" ", None, None),
            ],
        );
        assert_eq!(
            choice_nodes(&blank),
            Err(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Malformed))
        );
    }

    #[test]
    fn a_story_created_from_a_pack_keeps_its_identity_and_any_other_takes_the_story_id() {
        const STORY: &str = "0197a5d0-1111-7222-8333-444455556666";
        let two_nodes = |start: &str| {
            structure(
                start,
                vec![
                    offering(node(start, None, None), &["n2"]),
                    node("n2", None, None),
                ],
            )
        };
        let from_pack = two_nodes(PACK);
        let nodes = choice_nodes(&from_pack).expect("nodes");
        assert_eq!(choice_pack_uuid(STORY, &nodes).as_deref(), Some(PACK));
        // Whatever its case, a well-formed id is the pack's own.
        let upper = PACK.to_ascii_uppercase();
        let shouted = two_nodes(&upper);
        let nodes = choice_nodes(&shouted).expect("nodes");
        assert_eq!(
            choice_pack_uuid(STORY, &nodes).as_deref(),
            Some(upper.as_str())
        );
        // Written in the editor: the story id, like the other layouts.
        let authored = two_nodes("n1");
        let nodes = choice_nodes(&authored).expect("nodes");
        assert_eq!(choice_pack_uuid(STORY, &nodes).as_deref(), Some(STORY));
        // The story id cannot name two stages.
        let clash = structure(
            "n1",
            vec![
                offering(node("n1", None, None), &[STORY]),
                node(STORY, None, None),
            ],
        );
        let nodes = choice_nodes(&clash).expect("nodes");
        assert_eq!(choice_pack_uuid(STORY, &nodes), None);
    }

    #[test]
    fn the_layout_reading_is_one_verdict_the_graph_first_then_the_sounds() {
        const STORY: &str = "0197a5d0-1111-7222-8333-444455556666";
        // Every option a destination, every node a sound: the pack identity
        // and the nodes, the start node first.
        let ready = structure(
            "n2",
            vec![
                node("n1", Some("a1"), None),
                offering(node("n2", Some("a2"), None), &["n1", "n3"]),
                node("n3", Some("a3"), Some("i3")),
            ],
        );
        for silent_nodes in [SilentNodes::Refused, SilentNodes::Carried] {
            let layout = choice_layout(STORY, &ready, silent_nodes).expect("laid out");
            assert_eq!(layout.pack_uuid, STORY);
            assert_eq!(
                layout
                    .nodes
                    .iter()
                    .map(|node| node.node_id)
                    .collect::<Vec<_>>(),
                vec!["n2", "n1", "n3"]
            );
        }

        // A node without sound: refused for a device (the FIRST one, in
        // layout order), carried for an archive.
        let silent = structure(
            "n1",
            vec![
                offering(node("n1", Some("a1"), None), &["n2", "n3"]),
                node("n2", None, Some("i2")),
                node("n3", Some(""), None),
            ],
        );
        assert_eq!(
            choice_layout(STORY, &silent, SilentNodes::Refused),
            Err(ChoiceLayoutBlocker::NodeWithoutAudio {
                node_id: "n2".into()
            })
        );
        let carried = choice_layout(STORY, &silent, SilentNodes::Carried).expect("carried");
        assert_eq!(carried.nodes[1].audio_asset_id, None);
        assert_eq!(
            carried.nodes[2].audio_asset_id, None,
            "a blank asset id is no sound"
        );

        // The graph comes first: an option that leads nowhere is the reason,
        // whatever the sounds — and whatever the policy.
        let unlinked = structure(
            "n1",
            vec![
                node("n0", None, None),
                offering(node("n1", None, None), &["ghost"]),
            ],
        );
        for silent_nodes in [SilentNodes::Refused, SilentNodes::Carried] {
            assert_eq!(
                choice_layout(STORY, &unlinked, silent_nodes),
                Err(ChoiceLayoutBlocker::OptionWithoutDestination {
                    node_id: "n1".into()
                })
            );
        }
        // …then the identity: the story id cannot name two stages.
        let clash = structure(
            "n1",
            vec![
                offering(node("n1", None, None), &[STORY]),
                node(STORY, None, None),
            ],
        );
        for silent_nodes in [SilentNodes::Refused, SilentNodes::Carried] {
            assert_eq!(
                choice_layout(STORY, &clash, silent_nodes),
                Err(ChoiceLayoutBlocker::Structure(StoryPackBlocker::Malformed))
            );
        }
        // A story created from a pack keeps that pack's identity.
        let from_pack = structure(
            PACK,
            vec![
                offering(node(PACK, Some("a"), None), &["n2"]),
                node("n2", Some("b"), None),
            ],
        );
        assert_eq!(
            choice_layout(STORY, &from_pack, SilentNodes::Refused)
                .expect("laid out")
                .pack_uuid,
            PACK
        );
    }

    #[test]
    fn a_sequential_pack_read_back_as_a_story_lays_out_as_the_same_graph() {
        let pack = synthesize_sequential_pack(PACK, &assets(3));
        let again = laid_out("another-story", &story_of_pack(&pack));
        assert_eq!(resolved(&again), resolved(&pack));
        assert!(transcode_pack(&again).is_ok());
    }

    #[test]
    fn a_menu_pack_read_back_as_a_story_lays_out_as_the_same_graph() {
        for auto_continue in [false, true] {
            let mut menu = menu_assets(3);
            menu.auto_continue = auto_continue;
            let pack = synthesize_menu_pack(PACK, &menu);
            let again = laid_out("another-story", &story_of_pack(&pack));
            let mut expected = resolved(&pack);
            // The one thing the story does not carry: the question played
            // with every control off. Read back it is a node that plays
            // through like any other — pause and home on, which takes
            // nothing away from the listener.
            assert_eq!(expected[1].controls, (false, false, false, false, true));
            expected[1].controls = PLAYED;
            assert_eq!(
                resolved(&again),
                expected,
                "auto_continue = {auto_continue}"
            );
            assert!(transcode_pack(&again).is_ok());
        }
    }

    #[test]
    fn home_steps_back_one_wheel_at_a_time_and_a_wheel_is_entered_where_it_was_left() {
        // cover → wheel A [a1, a2]. a1 → a question → wheel B [b1, b2].
        // b1 → story1 → back to wheel B. b2 → story2 → back to wheel A.
        // a2 → a tale that ends the pack.
        let s = structure(
            "cover",
            vec![
                offering(node("cover", Some("c"), Some("ci")), &["a1", "a2"]),
                offering(node("a1", Some("a1s"), None), &["q"]),
                offering(node("a2", Some("a2s"), None), &["tale"]),
                offering(node("q", Some("qs"), None), &["b1", "b2"]),
                offering(node("b1", Some("b1s"), None), &["story1"]),
                offering(node("b2", Some("b2s"), None), &["story2"]),
                offering(node("story1", Some("s1"), None), &["b1", "b2"]),
                offering(node("story2", Some("s2"), None), &["a1", "a2"]),
                node("tale", Some("t"), None),
            ],
        );
        let pack = laid_out(PACK, &s);
        let stages = resolved(&pack);
        let stage = |id: &str| {
            stages
                .iter()
                .find(|stage| stage.uuid == id)
                .unwrap_or_else(|| panic!("stage {id}"))
                .clone()
        };
        let wheel_a = vec!["a1".to_string(), "a2".to_string()];
        let wheel_b = vec!["b1".to_string(), "b2".to_string()];

        // The start node is the cover, filed under the story id.
        let cover = stage(PACK);
        assert_eq!(cover.controls, COVER);
        assert_eq!(cover.ok, Some((wheel_a.clone(), 0)));
        assert_eq!(cover.home, None);

        // An entry of a wheel waits for the listener.
        for id in ["a1", "a2", "b1", "b2"] {
            assert_eq!(stage(id).controls, WHEEL_ENTRY, "{id}");
        }
        // Every other node plays through.
        for id in ["q", "story1", "story2", "tale"] {
            assert_eq!(stage(id).controls, PLAYED, "{id}");
        }

        // First-level entries: nothing above, home leaves the pack.
        assert_eq!(stage("a1").home, None);
        assert_eq!(stage("a2").home, None);
        // The question between the two wheels homes to the first one, on
        // the entry that led to it — and so do the second-level entries:
        // one level up, not out of the pack.
        assert_eq!(stage("q").ok, Some((wheel_b.clone(), 0)));
        assert_eq!(stage("q").home, Some((wheel_a.clone(), 0)));
        assert_eq!(stage("b1").home, Some((wheel_a.clone(), 0)));
        assert_eq!(stage("b2").home, Some((wheel_a.clone(), 0)));

        // A story picked on a wheel homes to that wheel, on its own entry.
        assert_eq!(stage("story1").home, Some((wheel_b.clone(), 0)));
        assert_eq!(stage("story2").home, Some((wheel_b.clone(), 1)));
        assert_eq!(stage("tale").home, Some((wheel_a.clone(), 1)));
        // …and its end enters the wheel it offers ON THE ENTRY THE LISTENER
        // CAME THROUGH: story1 returns to wheel B on b1, story2 to wheel A
        // on a1 (the entry that opened the second menu).
        assert_eq!(stage("story1").ok, Some((wheel_b, 0)));
        assert_eq!(stage("story2").ok, Some((wheel_a, 0)));
        // A node without options ends the pack.
        assert_eq!(stage("tale").ok, None);

        assert!(transcode_pack(&pack).is_ok());
    }

    #[test]
    fn a_story_returns_to_its_wheel_on_its_own_entry_not_on_the_first_one() {
        // cover → wheel [o1, o2, o3]; each entry leads to its story, each
        // story back to the wheel.
        let wheel = ["o1", "o2", "o3"];
        let mut nodes = vec![offering(node("cover", Some("c"), None), &wheel)];
        for i in 1..=3 {
            let story = format!("s{i}");
            nodes.push(offering(
                node(&format!("o{i}"), Some("p"), None),
                &[story.as_str()],
            ));
        }
        for i in 1..=3 {
            nodes.push(offering(node(&format!("s{i}"), Some("a"), None), &wheel));
        }
        let pack = laid_out(PACK, &structure("cover", nodes));
        let stages = resolved(&pack);
        for i in 1..=3 {
            let story = stages
                .iter()
                .find(|stage| stage.uuid == format!("s{i}"))
                .expect("story stage");
            let entries: Vec<String> = wheel.iter().map(|id| id.to_string()).collect();
            assert_eq!(story.ok, Some((entries.clone(), i - 1)), "story {i}");
            assert_eq!(story.home, Some((entries, i - 1)), "story {i}");
        }
    }

    #[test]
    fn nodes_offering_the_same_options_share_one_action_node() {
        let s = structure(
            "n1",
            vec![
                offering(node("n1", Some("a1"), None), &["n2", "n3"]),
                offering(node("n2", Some("a2"), None), &["n4"]),
                offering(node("n3", Some("a3"), None), &["n4"]),
                offering(node("n4", Some("a4"), None), &["n2", "n3"]),
                // Never reached from the start: a stage all the same, with
                // nothing above it.
                offering(node("n5", None, None), &["n5"]),
            ],
        );
        let pack = laid_out(PACK, &s);
        assert_eq!(pack.stage_nodes.len(), 5);
        let lists: Vec<Vec<&str>> = pack
            .action_nodes
            .iter()
            .map(|action| action.options.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(lists, vec![vec!["n2", "n3"], vec!["n4"], vec!["n5"]]);
        assert_eq!(
            pack.action_nodes
                .iter()
                .map(|action| action.id.clone())
                .collect::<Vec<_>>(),
            vec![
                format!("{PACK}-options-1"),
                format!("{PACK}-options-2"),
                format!("{PACK}-options-3"),
            ]
        );
        let ok_action = |index: usize| {
            pack.stage_nodes[index]
                .ok_transition
                .as_ref()
                .map(|t| t.action_node.clone())
        };
        assert_eq!(ok_action(0), ok_action(3), "n1 and n4 offer the same wheel");
        assert_eq!(
            ok_action(1),
            ok_action(2),
            "n2 and n3 lead to the same node"
        );

        let lost = &pack.stage_nodes[4];
        assert!(lost.home_transition.is_none());
        assert_eq!(lost.ok_transition.as_ref().map(|t| t.option_index), Some(0));
        // A node with neither picture nor sound is a stage all the same.
        assert_eq!((lost.image.as_deref(), lost.audio.as_deref()), (None, None));
        assert!(transcode_pack(&pack).is_ok());
    }

    #[test]
    fn an_option_back_to_the_start_node_points_at_the_pack_entry() {
        let s = structure(
            "n1",
            vec![
                offering(node("n1", Some("a1"), Some("i1")), &["n2"]),
                offering(node("n2", Some("a2"), None), &["n1"]),
            ],
        );
        let pack = laid_out(PACK, &s);
        assert_eq!(pack.stage_nodes[0].uuid, PACK);
        assert!(pack.stage_nodes[0].square_one && !pack.stage_nodes[1].square_one);
        assert_eq!(pack.stage_nodes[0].image.as_deref(), Some("i1"));
        assert_eq!(pack.stage_nodes[1].uuid, "n2");
        assert_eq!(pack.action_nodes[1].options, vec![PACK.to_string()]);
        assert!(transcode_pack(&pack).is_ok());
    }

    #[test]
    fn every_stage_of_a_story_with_choices_is_named_and_nothing_else() {
        let mut cover = offering(node("n1", Some("a1"), None), &["n2", "n3"]);
        cover.label = String::new();
        let mut unnamed = node("n3", Some("a3"), None);
        unnamed.label = "  ".into();
        let s = structure("n1", vec![cover, node("n2", Some("a2"), None), unnamed]);
        let nodes = choice_nodes(&s).expect("nodes");
        let pack = laid_out(PACK, &s);
        let names = choice_stage_names(PACK, "Série", &nodes);
        let uuids: std::collections::BTreeSet<&str> =
            pack.stage_nodes.iter().map(|s| s.uuid.as_str()).collect();
        let named: std::collections::BTreeSet<&str> = names.keys().map(String::as_str).collect();
        assert_eq!(named, uuids, "one name per stage");
        assert_eq!(
            names[PACK], "Série",
            "an unnamed start node takes the story title"
        );
        assert_eq!(names["n2"], "label n2");
        assert_eq!(names["n3"], "Nœud 3");
    }
}
