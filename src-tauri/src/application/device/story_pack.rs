//! Plan the device pack of a LIBRARY story (one without a retained source
//! archive) from the local database — the read side of
//! [`send_story_pack_to_device`](super::send::send_story_pack_to_device) and
//! of the story archive export, which shares the very same pack.
//!
//! Runs UNDER the SQLite lock and does no other I/O: it reads the story's
//! canonical structure, its presentation (layout + spoken announcements)
//! and the stored file names of the media they reference, then hands the
//! pure synthesis (`domain::device::story_pack`) the ordered episodes —
//! chained in sequence, or behind a spoken menu. Every refusal is decided
//! here, BEFORE any device or destination is touched, as a typed
//! [`StoryPackRefusal`]: the send words it with the copy the library card
//! announces pre-click ([`plan_story_pack`]), the export with its own.
//!
//! A story WITH CHOICES (a node offering an option) is not an episode list:
//! it is laid out by its own graph — every node a stage, the options of a
//! node the wheel that follows it — for the send and the export alike. The
//! caller only says whether a node may stay silent ([`SilentNodes`]): the
//! send refuses one (a device plays nothing else), the export carries it.

use std::collections::HashMap;

use crate::domain::device::{
    choice_layout, choice_stage_names, linear_episodes, menu_blocker, offers_choices, stage_names,
    synthesize_choice_pack, synthesize_menu_pack, synthesize_sequential_pack, ChoiceLayout,
    ChoiceLayoutBlocker, ChoiceNodeAssets, EpisodeAssets, MenuEpisode, MenuPackAssets, SilentNodes,
    StoryLayout, StoryPackBlocker, StudioStoryPack,
};
use crate::domain::shared::AppError;
use crate::domain::story::CanonicalStructure;
use crate::infrastructure::db::DbHandle;

/// Why a library story cannot be planned as a pack. A closed set carrying
/// only what a caller needs to word the refusal for its own gesture — never
/// a path, never a raw storage message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryPackRefusal {
    /// No story has this id.
    NotFound,
    /// The local database could not be read; the tag names the read.
    Storage(&'static str),
    /// A device-copied (V1/V2) pack: its content is the copied pack, which
    /// this engine does not convert.
    DevicePack,
    /// The structure does not lay out as a pack (see [`StoryPackBlocker`]).
    Blocked(StoryPackBlocker),
    /// An episode's audio is referenced by the structure but no longer
    /// backed by the library.
    AssetMissing { node_id: String },
    /// A spoken announcement is referenced but its asset is gone — the
    /// question (`"question"`) or an episode's spoken title (its node id).
    AnnouncementMissing { which: String },
    /// A story with choices whose node `node_id` offers an option that
    /// leads nowhere (not linked yet, or its destination is gone): a wheel
    /// entry must be a stage.
    OptionWithoutDestination { node_id: String },
    /// A story with choices whose node `node_id` carries no sound, in a
    /// plan that refuses a silent stage ([`SilentNodes::Refused`]).
    NodeWithoutAudio { node_id: String },
}

/// A planned pack with what a shared archive carries beside the graph: the
/// story's title and the names of its stages (keyed by stage uuid).
#[derive(Debug, Clone)]
pub struct PlannedStoryPack {
    pub pack: StudioStoryPack,
    pub title: String,
    pub stage_names: HashMap<String, String>,
}

/// Build the device pack of `story_id` (per its layout, or by its own graph
/// when it has choices), or refuse with the reason — worded for the SEND
/// gesture. A device plays nothing but sounds: a silent node is refused.
pub fn plan_story_pack(db: &DbHandle, story_id: &str) -> Result<StudioStoryPack, AppError> {
    plan_named_story_pack(db, story_id, SilentNodes::Refused)
        .map(|planned| planned.pack)
        .map_err(send_refusal_error)
}

/// Build the pack of `story_id` together with its title and stage names, or
/// say why not as a typed refusal the caller words itself. `silent_nodes`
/// settles what becomes of a node without sound in a story with choices
/// (an episode always needs its audio).
pub fn plan_named_story_pack(
    db: &DbHandle,
    story_id: &str,
    silent_nodes: SilentNodes,
) -> Result<PlannedStoryPack, StoryPackRefusal> {
    let (title, structure_json): (String, String) = db
        .conn()
        .query_row(
            "SELECT title, structure_json FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => StoryPackRefusal::NotFound,
            _ => StoryPackRefusal::Storage("read_story"),
        })?;

    // A device-copied (V1/V2) pack: its content is the copied pack, which
    // this engine does not convert — refused by provenance, not structure.
    let device_pack: bool = db
        .conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM story_imports WHERE story_id = ?1)",
            rusqlite::params![story_id],
            |row| row.get(0),
        )
        .map_err(|_| StoryPackRefusal::Storage("read_provenance"))?;
    if device_pack {
        return Err(StoryPackRefusal::DevicePack);
    }

    let structure: CanonicalStructure = serde_json::from_str(&structure_json)
        .map_err(|_| StoryPackRefusal::Blocked(StoryPackBlocker::Malformed))?;
    if offers_choices(&structure) {
        return plan_choice_pack(db, story_id, title, &structure, silent_nodes);
    }
    let episodes = linear_episodes(&structure).map_err(StoryPackRefusal::Blocked)?;
    let episode_labels: Vec<&str> = episodes.iter().map(|episode| episode.label).collect();

    let mut assets = Vec::with_capacity(episodes.len());
    for episode in &episodes {
        let audio_ref = stored_file_name(db, story_id, episode.audio_asset_id, "audio")?
            .ok_or_else(|| StoryPackRefusal::AssetMissing {
                node_id: episode.node_id.to_string(),
            })?;
        let image_ref = match episode.image_asset_id {
            Some(id) => stored_file_name(db, story_id, id, "image")?,
            None => None,
        };
        assets.push(EpisodeAssets {
            audio_ref,
            image_ref,
        });
    }

    let presentation = read_presentation_rows(db, story_id)?;
    let names = stage_names(presentation.layout, story_id, &title, &episode_labels);
    let pack = match presentation.layout {
        StoryLayout::Sequential => synthesize_sequential_pack(story_id, &assets),
        StoryLayout::Menu => {
            // The spoken announcements: refused as a whole while any is
            // missing (the same rule the card projects).
            let has_prompt = |node_id: &str| presentation.prompts.iter().any(|(n, _)| n == node_id);
            if let Some(blocker) = menu_blocker(
                &episodes,
                presentation.question_asset_id.is_some(),
                has_prompt,
            ) {
                return Err(StoryPackRefusal::Blocked(blocker));
            }
            let question_asset =
                presentation
                    .question_asset_id
                    .as_deref()
                    .ok_or(StoryPackRefusal::Blocked(
                        StoryPackBlocker::MissingAnnouncements,
                    ))?;
            let question_ref = stored_file_name(db, story_id, question_asset, "audio")?
                .ok_or_else(|| StoryPackRefusal::AnnouncementMissing {
                    which: "question".to_string(),
                })?;
            let title_ref = match presentation.title_asset_id.as_deref() {
                Some(id) => stored_file_name(db, story_id, id, "audio")?,
                None => None,
            };
            let mut menu_episodes = Vec::with_capacity(episodes.len());
            for (episode, media) in episodes.iter().zip(assets) {
                let prompt_asset = presentation
                    .prompts
                    .iter()
                    .find(|(n, _)| n == episode.node_id)
                    .map(|(_, asset)| asset.as_str())
                    .ok_or(StoryPackRefusal::Blocked(
                        StoryPackBlocker::MissingAnnouncements,
                    ))?;
                let prompt_ref = stored_file_name(db, story_id, prompt_asset, "audio")?
                    .ok_or_else(|| StoryPackRefusal::AnnouncementMissing {
                        which: episode.node_id.to_string(),
                    })?;
                menu_episodes.push(MenuEpisode {
                    audio_ref: media.audio_ref,
                    image_ref: media.image_ref,
                    prompt_ref,
                });
            }
            synthesize_menu_pack(
                story_id,
                &MenuPackAssets {
                    title_ref,
                    question_ref,
                    episodes: menu_episodes,
                    auto_continue: presentation.auto_continue,
                },
            )
        }
    };
    Ok(PlannedStoryPack {
        pack,
        title,
        stage_names: names,
    })
}

/// The pack of a story WITH CHOICES: its own node graph, every node a stage
/// carrying the node's stored media. The presentation (`À la suite` /
/// `Au choix`) does not apply — the options ARE the navigation.
fn plan_choice_pack(
    db: &DbHandle,
    story_id: &str,
    title: String,
    structure: &CanonicalStructure,
    silent_nodes: SilentNodes,
) -> Result<PlannedStoryPack, StoryPackRefusal> {
    // The reading the library card makes before any click — one verdict.
    let ChoiceLayout { pack_uuid, nodes } = choice_layout(story_id, structure, silent_nodes)
        .map_err(|blocker| match blocker {
            ChoiceLayoutBlocker::Structure(blocker) => StoryPackRefusal::Blocked(blocker),
            ChoiceLayoutBlocker::OptionWithoutDestination { node_id } => {
                StoryPackRefusal::OptionWithoutDestination { node_id }
            }
            ChoiceLayoutBlocker::NodeWithoutAudio { node_id } => {
                StoryPackRefusal::NodeWithoutAudio { node_id }
            }
        })?;

    // A story created from a pack carries thousands of media: ONE read of
    // the story's asset rows, not two per node.
    let stored = stored_file_names(db, story_id)?;
    let mut assets = Vec::with_capacity(nodes.len());
    for node in &nodes {
        // A sound the structure references must still be backed by the
        // library (the same rule as an episode's); a picture that is gone
        // is decoration lost, never a refusal.
        let audio_ref = match node.audio_asset_id {
            Some(id) => Some(stored.audio.get(id).cloned().ok_or_else(|| {
                StoryPackRefusal::AssetMissing {
                    node_id: node.node_id.to_string(),
                }
            })?),
            None => None,
        };
        let image_ref = node
            .image_asset_id
            .and_then(|id| stored.image.get(id).cloned());
        assets.push(ChoiceNodeAssets {
            audio_ref,
            image_ref,
        });
    }

    let stage_names = choice_stage_names(&pack_uuid, &title, &nodes);
    Ok(PlannedStoryPack {
        pack: synthesize_choice_pack(&pack_uuid, &nodes, &assets),
        title,
        stage_names,
    })
}

/// Every stored media of the story, by kind: asset id → node-media store
/// file name. Kept apart by kind so an id never resolves to a media of the
/// other one.
#[derive(Default)]
struct StoredFileNames {
    audio: HashMap<String, String>,
    image: HashMap<String, String>,
}

fn stored_file_names(db: &DbHandle, story_id: &str) -> Result<StoredFileNames, StoryPackRefusal> {
    let mut stmt = db
        .conn()
        .prepare("SELECT media_type, id, file_name FROM assets WHERE story_id = ?1")
        .map_err(|_| StoryPackRefusal::Storage("read_assets"))?;
    let rows = stmt
        .query_map(rusqlite::params![story_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| StoryPackRefusal::Storage("read_assets"))?;
    let mut stored = StoredFileNames::default();
    for row in rows {
        let (media_type, id, file_name) =
            row.map_err(|_| StoryPackRefusal::Storage("read_assets"))?;
        match media_type.as_str() {
            "audio" => {
                stored.audio.insert(id, file_name);
            }
            "image" => {
                stored.image.insert(id, file_name);
            }
            _ => {}
        }
    }
    Ok(stored)
}

/// The presentation rows a plan needs: the layout, the spoken title and
/// question asset ids, and the (node id → asset id) spoken titles.
struct PresentationRows {
    layout: StoryLayout,
    auto_continue: bool,
    title_asset_id: Option<String>,
    question_asset_id: Option<String>,
    prompts: Vec<(String, String)>,
}

fn read_presentation_rows(
    db: &DbHandle,
    story_id: &str,
) -> Result<PresentationRows, StoryPackRefusal> {
    use rusqlite::OptionalExtension;
    let layout_row: Option<(String, bool, Option<String>, Option<String>)> = db
        .conn()
        .query_row(
            "SELECT layout, auto_continue, title_audio_asset_id, question_audio_asset_id \
             FROM story_layouts WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|_| StoryPackRefusal::Storage("read_layout"))?;
    let (layout, auto_continue, title_asset_id, question_asset_id) = match layout_row {
        Some((tag, auto_continue, title, question)) => (
            StoryLayout::parse(&tag).unwrap_or_default(),
            auto_continue,
            title,
            question,
        ),
        None => (StoryLayout::Sequential, false, None, None),
    };
    let mut stmt = db
        .conn()
        .prepare("SELECT node_id, audio_asset_id FROM story_node_prompts WHERE story_id = ?1")
        .map_err(|_| StoryPackRefusal::Storage("read_prompts"))?;
    let prompts = stmt
        .query_map(rusqlite::params![story_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| StoryPackRefusal::Storage("read_prompts"))?
        .collect::<Result<Vec<(String, String)>, _>>()
        .map_err(|_| StoryPackRefusal::Storage("read_prompts"))?;
    Ok(PresentationRows {
        layout,
        auto_continue,
        title_asset_id,
        question_asset_id,
        prompts,
    })
}

/// The node-media store file name (`<hash>.<ext>`) of one of the story's
/// assets, by asset id and kind. `None` when the row is absent — a
/// reference the structure carries but the store no longer backs.
fn stored_file_name(
    db: &DbHandle,
    story_id: &str,
    asset_id: &str,
    media_type: &str,
) -> Result<Option<String>, StoryPackRefusal> {
    db.conn()
        .query_row(
            "SELECT file_name FROM assets WHERE id = ?1 AND story_id = ?2 AND media_type = ?3",
            rusqlite::params![asset_id, story_id, media_type],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            _ => Err(StoryPackRefusal::Storage("read_asset")),
        })
}

/// The refusal worded for the SEND gesture — the copy the library card
/// announces pre-click, with its actionable next step.
fn send_refusal_error(refusal: StoryPackRefusal) -> AppError {
    match refusal {
        StoryPackRefusal::NotFound => story_not_found_error(),
        StoryPackRefusal::Storage(stage) => storage_error(stage),
        StoryPackRefusal::DevicePack => device_pack_error(),
        StoryPackRefusal::Blocked(blocker) => blocker_error(blocker),
        StoryPackRefusal::AssetMissing { node_id } => asset_missing_error(&node_id),
        StoryPackRefusal::AnnouncementMissing { which } => announcement_missing_error(&which),
        StoryPackRefusal::OptionWithoutDestination { node_id } => {
            option_without_destination_error(&node_id)
        }
        StoryPackRefusal::NodeWithoutAudio { node_id } => node_without_audio_error(&node_id),
    }
}

/// The story could not be laid out as a device pack — the same closed
/// reasons the library card announces, with an actionable next step.
fn blocker_error(blocker: StoryPackBlocker) -> AppError {
    let (message, action) = match blocker {
        StoryPackBlocker::Empty => (
            "Envoi impossible: cette histoire n'a aucun épisode.",
            "Ajoute au moins un épisode avec un audio, puis réessaie l'envoi.",
        ),
        // A story with choices is laid out by its own graph: the plan never
        // returns `Branching`. Should it ever, the structure did not read
        // as the plan expected — the same next step.
        StoryPackBlocker::Malformed | StoryPackBlocker::Branching => (
            "Envoi impossible: la structure de cette histoire est illisible.",
            "Rouvre l'histoire pour la vérifier, puis réessaie l'envoi.",
        ),
        StoryPackBlocker::MissingAudio => (
            "Envoi impossible: un ou plusieurs épisodes de cette histoire n'ont pas d'audio.",
            "Ajoute un audio à chaque épisode, puis réessaie l'envoi.",
        ),
        StoryPackBlocker::MissingAnnouncements => (
            "Envoi impossible: les annonces du menu (question et titres des épisodes) ne sont pas encore générées.",
            "Génère les annonces depuis l'histoire, puis réessaie l'envoi.",
        ),
    };
    AppError::device_write_failed(message, action).with_details(serde_json::json!({
        "source": "story_pack",
        "cause": blocker.diagnostic_tag(),
    }))
}

/// A story with choices offers an option that leads nowhere — surfaced by
/// node, with the gesture that settles it.
fn option_without_destination_error(node_id: &str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: une option de cette histoire ne mène à aucun nœud.",
        "Rouvre l'histoire pour lier chaque option à un nœud (ou la retirer), puis réessaie l'envoi.",
    )
    .with_details(serde_json::json!({
        "source": "story_pack",
        "cause": "option_without_destination",
        "node": node_id,
    }))
}

/// A story with choices has a node without sound: the device would stay
/// mute on it. The card says so before the click; this names the node.
fn node_without_audio_error(node_id: &str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: un ou plusieurs nœuds de cette histoire n'ont pas d'audio.",
        "Ajoute un audio à chaque nœud, puis réessaie l'envoi.",
    )
    .with_details(serde_json::json!({
        "source": "story_pack",
        "cause": "node_without_audio",
        "node": node_id,
    }))
}

/// A spoken announcement is referenced but its asset row is gone.
fn announcement_missing_error(which: &str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: une annonce du menu est introuvable dans la bibliothèque.",
        "Régénère les annonces depuis l'histoire, puis réessaie l'envoi.",
    )
    .with_details(serde_json::json!({
        "source": "story_pack",
        "cause": "announcement_missing",
        "announcement": which,
    }))
}

fn device_pack_error() -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: une histoire copiée depuis une Lunii ne peut pas être envoyée vers ce modèle.",
        "Envoie l'histoire depuis sa source d'origine (archive .zip, page web ou flux RSS).",
    )
    .with_details(serde_json::json!({ "source": "story_pack", "cause": "device_pack" }))
}

/// A media the structure references is not in the library (its `assets`
/// row is gone) — surfaced by node, never by path.
fn asset_missing_error(node_id: &str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: un média de l'histoire est introuvable dans la bibliothèque.",
        "Rouvre l'histoire pour vérifier ses épisodes (audio et image), puis réessaie l'envoi.",
    )
    .with_details(serde_json::json!({
        "source": "story_pack",
        "cause": "asset_missing",
        "node": node_id,
    }))
}

fn story_not_found_error() -> AppError {
    AppError::library_inconsistent(
        "Histoire introuvable, recharge la bibliothèque.",
        "Retourne à la bibliothèque et recharge la liste.",
    )
    .with_details(serde_json::json!({ "source": "story_pack", "cause": "not_found" }))
}

fn storage_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu lire ta bibliothèque locale.",
        "Relance l'application ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "story_pack", "stage": stage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::{create_story, CreateStoryInput};
    use crate::infrastructure::db::{open_in_memory, run_migrations};

    fn fresh_db() -> DbHandle {
        let mut db = open_in_memory().expect("open");
        run_migrations(&mut db).expect("migrate");
        db
    }

    fn insert_asset(db: &DbHandle, story_id: &str, id: &str, media_type: &str, file_name: &str) {
        let (format, hash_seed) = match media_type {
            "audio" => ("mp3", "a"),
            _ => ("png", "b"),
        };
        db.conn()
            .execute(
                "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 10, ?6, '2026-01-01T00:00:00Z')",
                rusqlite::params![id, story_id, hash_seed.repeat(64), media_type, format, file_name],
            )
            .expect("insert asset");
    }

    fn set_structure(db: &DbHandle, story_id: &str, nodes: &[(&str, Option<&str>, Option<&str>)]) {
        let nodes: Vec<serde_json::Value> = nodes
            .iter()
            .map(|(id, audio, image)| {
                serde_json::json!({
                    "id": id, "text": "", "label": id,
                    "imageAssetId": image, "audioAssetId": audio, "options": [],
                })
            })
            .collect();
        let json = serde_json::json!({ "schemaVersion": 3, "startNodeId": "n1", "nodes": nodes });
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = ?1 WHERE id = ?2",
                rusqlite::params![json.to_string(), story_id],
            )
            .expect("update structure");
    }

    fn story(db: &mut DbHandle) -> String {
        create_story(
            db,
            CreateStoryInput {
                title: "Série".into(),
            },
        )
        .expect("create")
        .id
    }

    #[test]
    fn plans_a_sequential_pack_from_the_structure_and_the_stored_file_names() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(
            &db,
            &id,
            &[
                ("n1", Some("aud-1"), Some("img-1")),
                ("n2", Some("aud-2"), None),
            ],
        );
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        insert_asset(&db, &id, "aud-2", "audio", "aaaa2222.mp3");
        insert_asset(&db, &id, "img-1", "image", "bbbb1111.png");

        let pack = plan_story_pack(&db, &id).expect("plan");
        assert_eq!(pack.stage_nodes.len(), 3);
        assert_eq!(pack.stage_nodes[0].uuid, id, "pack uuid = story id");
        assert_eq!(pack.stage_nodes[1].audio.as_deref(), Some("aaaa1111.mp3"));
        assert_eq!(pack.stage_nodes[1].image.as_deref(), Some("bbbb1111.png"));
        assert_eq!(pack.stage_nodes[2].audio.as_deref(), Some("aaaa2222.mp3"));
        assert_eq!(pack.stage_nodes[2].image, None);
    }

    #[test]
    fn a_fresh_story_is_refused_for_its_missing_audio_before_any_device_touch() {
        let mut db = fresh_db();
        let id = story(&mut db);
        let err = plan_story_pack(&db, &id).expect_err("refused");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(v["details"]["source"], "story_pack");
        assert_eq!(v["details"]["cause"], "missing_audio");
    }

    #[test]
    fn an_audio_reference_without_its_assets_row_is_refused_by_node() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(&db, &id, &[("n1", Some("aud-gone"), None)]);
        let err = plan_story_pack(&db, &id).expect_err("refused");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["details"]["cause"], "asset_missing");
        assert_eq!(v["details"]["node"], "n1");
        assert!(v["details"].get("path").is_none());
    }

    #[test]
    fn an_image_reference_without_its_row_degrades_to_no_image() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(&db, &id, &[("n1", Some("aud-1"), Some("img-gone"))]);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        let pack = plan_story_pack(&db, &id).expect("plan");
        assert_eq!(pack.stage_nodes[0].image, None, "cover without image");
        assert_eq!(pack.stage_nodes[1].image, None);
    }

    #[test]
    fn a_device_pack_story_is_refused_by_provenance() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(&db, &id, &[("n1", Some("aud-1"), None)]);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        db.conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, pack_file_count, pack_total_bytes, pack_checksum, imported_at, source_family) \
                 VALUES (?1, '01a06ed9-2040-77c1-9e03-b8f429f4e954', '0123456789abcdef0123456789abcdef', 5, 18, ?2, '2026-01-01T00:00:00Z', 'lunii')",
                rusqlite::params![&id, "c".repeat(64)],
            )
            .expect("insert provenance");
        let err = plan_story_pack(&db, &id).expect_err("refused");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["details"]["cause"], "device_pack");
    }

    #[test]
    fn an_unknown_story_is_a_library_inconsistency() {
        let db = fresh_db();
        let err = plan_story_pack(&db, "nope").expect_err("refused");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "LIBRARY_INCONSISTENT");
        assert_eq!(v["details"]["cause"], "not_found");
    }

    fn set_menu(db: &DbHandle, story_id: &str, question: Option<&str>, prompts: &[(&str, &str)]) {
        db.conn()
            .execute(
                "INSERT INTO story_layouts (story_id, layout, question_audio_asset_id, question_spoken_text, voice_id, updated_at) \
                 VALUES (?1, 'menu', ?2, 'q', 'v', '2026-01-01T00:00:00Z')",
                rusqlite::params![story_id, question],
            )
            .expect("layout");
        for (node_id, asset) in prompts {
            db.conn()
                .execute(
                    "INSERT INTO story_node_prompts (story_id, node_id, audio_asset_id, spoken_text, voice_id, updated_at) \
                     VALUES (?1, ?2, ?3, 't', 'v', '2026-01-01T00:00:00Z')",
                    rusqlite::params![story_id, node_id, asset],
                )
                .expect("prompt");
        }
    }

    #[test]
    fn a_menu_layout_plans_a_menu_pack_from_the_announcements() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(
            &db,
            &id,
            &[
                ("n1", Some("aud-1"), Some("img-1")),
                ("n2", Some("aud-2"), None),
            ],
        );
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        insert_asset(&db, &id, "aud-2", "audio", "aaaa2222.mp3");
        insert_asset(&db, &id, "img-1", "image", "bbbb1111.png");
        insert_asset(&db, &id, "q-1", "audio", "cccc0001.wav");
        insert_asset(&db, &id, "p-1", "audio", "cccc1111.wav");
        insert_asset(&db, &id, "p-2", "audio", "cccc2222.wav");
        set_menu(&db, &id, Some("q-1"), &[("n1", "p-1"), ("n2", "p-2")]);

        let pack = plan_story_pack(&db, &id).expect("plan");
        // cover + question + 2 options + 2 stories.
        assert_eq!(pack.stage_nodes.len(), 6);
        assert_eq!(
            pack.stage_nodes[0].audio.as_deref(),
            Some("cccc1111.wav"),
            "no spoken title: the first prompt"
        );
        assert_eq!(pack.stage_nodes[1].audio.as_deref(), Some("cccc0001.wav"));
        assert_eq!(pack.stage_nodes[2].audio.as_deref(), Some("cccc1111.wav"));
        assert_eq!(pack.stage_nodes[2].image.as_deref(), Some("bbbb1111.png"));
        assert_eq!(pack.stage_nodes[3].audio.as_deref(), Some("cccc2222.wav"));
        assert_eq!(pack.stage_nodes[4].audio.as_deref(), Some("aaaa1111.mp3"));
        assert_eq!(pack.stage_nodes[5].audio.as_deref(), Some("aaaa2222.mp3"));
        // Off by default: the end of an episode returns to the wheel on it.
        let end = pack.stage_nodes[4].ok_transition.as_ref().unwrap();
        assert_eq!(end.action_node, pack.action_nodes[1].id);
        assert_eq!(end.option_index, 0);
        let end = pack.stage_nodes[5].ok_transition.as_ref().unwrap();
        assert_eq!(end.action_node, pack.action_nodes[1].id);
        assert_eq!(end.option_index, 1);

        // With the per-story flag, the first episode chains into the second.
        db.conn()
            .execute(
                "UPDATE story_layouts SET auto_continue = 1 WHERE story_id = ?1",
                rusqlite::params![&id],
            )
            .expect("flag");
        let pack = plan_story_pack(&db, &id).expect("plan");
        let end = pack.stage_nodes[4].ok_transition.as_ref().unwrap();
        assert_eq!(end.action_node, pack.action_nodes[3].id, "next episode");
        let end = pack.stage_nodes[5].ok_transition.as_ref().unwrap();
        assert_eq!(end.action_node, pack.action_nodes[1].id, "last: the wheel");
        assert_eq!(end.option_index, 1);
    }

    #[test]
    fn a_menu_layout_without_its_announcements_is_refused_before_any_device_touch() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(
            &db,
            &id,
            &[("n1", Some("aud-1"), None), ("n2", Some("aud-2"), None)],
        );
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        insert_asset(&db, &id, "aud-2", "audio", "aaaa2222.mp3");
        insert_asset(&db, &id, "q-1", "audio", "cccc0001.wav");
        insert_asset(&db, &id, "p-1", "audio", "cccc1111.wav");
        // One prompt short.
        set_menu(&db, &id, Some("q-1"), &[("n1", "p-1")]);
        let err = plan_story_pack(&db, &id).expect_err("refused");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["details"]["cause"], "missing_announcements");
        assert!(!err.user_action.as_deref().unwrap_or("").is_empty());
    }

    #[test]
    fn the_named_plan_carries_the_title_and_one_name_per_stage() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(
            &db,
            &id,
            &[
                ("n1", Some("aud-1"), Some("img-1")),
                ("n2", Some("aud-2"), None),
            ],
        );
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        insert_asset(&db, &id, "aud-2", "audio", "aaaa2222.mp3");
        insert_asset(&db, &id, "img-1", "image", "bbbb1111.png");

        let planned = plan_named_story_pack(&db, &id, SilentNodes::Refused).expect("plan");
        assert_eq!(planned.title, "Série");
        assert_eq!(planned.stage_names.len(), planned.pack.stage_nodes.len());
        // The cover is named after the story, the episodes after their labels
        // (`set_structure` labels each node with its id).
        assert_eq!(
            planned.stage_names[&planned.pack.stage_nodes[0].uuid],
            "Série"
        );
        assert_eq!(planned.stage_names[&planned.pack.stage_nodes[1].uuid], "n1");
        assert_eq!(planned.stage_names[&planned.pack.stage_nodes[2].uuid], "n2");
        // The send plan is the very same pack.
        let pack = plan_story_pack(&db, &id).expect("send plan");
        assert_eq!(
            pack.stage_nodes.iter().map(|s| &s.uuid).collect::<Vec<_>>(),
            planned
                .pack
                .stage_nodes
                .iter()
                .map(|s| &s.uuid)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_named_plan_of_a_menu_story_names_the_question_and_the_wheel_entries() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(&db, &id, &[("n1", Some("aud-1"), None)]);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        insert_asset(&db, &id, "q-1", "audio", "cccc0001.wav");
        insert_asset(&db, &id, "p-1", "audio", "cccc1111.wav");
        set_menu(&db, &id, Some("q-1"), &[("n1", "p-1")]);

        let planned = plan_named_story_pack(&db, &id, SilentNodes::Refused).expect("plan");
        let names: Vec<&str> = planned
            .pack
            .stage_nodes
            .iter()
            .map(|stage| planned.stage_names[&stage.uuid].as_str())
            .collect();
        assert_eq!(names, vec!["Série", "Question", "Choix : n1", "n1"]);
    }

    #[test]
    fn refusals_are_typed_so_each_gesture_words_its_own() {
        let mut db = fresh_db();
        assert_eq!(
            plan_named_story_pack(&db, "nope", SilentNodes::Refused).unwrap_err(),
            StoryPackRefusal::NotFound
        );
        let id = story(&mut db);
        assert_eq!(
            plan_named_story_pack(&db, &id, SilentNodes::Refused).unwrap_err(),
            StoryPackRefusal::Blocked(StoryPackBlocker::MissingAudio)
        );
        set_structure(&db, &id, &[("n1", Some("aud-gone"), None)]);
        assert_eq!(
            plan_named_story_pack(&db, &id, SilentNodes::Refused).unwrap_err(),
            StoryPackRefusal::AssetMissing {
                node_id: "n1".into()
            }
        );
        insert_asset(&db, &id, "aud-gone", "audio", "aaaa1111.mp3");
        set_menu(&db, &id, Some("q-gone"), &[("n1", "p-1")]);
        insert_asset(&db, &id, "p-1", "audio", "cccc1111.wav");
        assert_eq!(
            plan_named_story_pack(&db, &id, SilentNodes::Refused).unwrap_err(),
            StoryPackRefusal::AnnouncementMissing {
                which: "question".into()
            }
        );
    }

    // ===== stories with choices =====

    /// One node of a story with choices: id, audio, image, and the
    /// destination of each of its options (`None` = not linked yet).
    type ChoiceNodeSpec<'a> = (
        &'a str,
        Option<&'a str>,
        Option<&'a str>,
        &'a [Option<&'a str>],
    );

    fn set_choice_structure(
        db: &DbHandle,
        story_id: &str,
        start: &str,
        nodes: &[ChoiceNodeSpec<'_>],
    ) {
        let nodes: Vec<serde_json::Value> = nodes
            .iter()
            .map(|(id, audio, image, destinations)| {
                let options: Vec<serde_json::Value> = destinations
                    .iter()
                    .map(|target| serde_json::json!({ "label": "suite", "target": target }))
                    .collect();
                serde_json::json!({
                    "id": id, "text": "", "label": id,
                    "imageAssetId": image, "audioAssetId": audio, "options": options,
                })
            })
            .collect();
        let json = serde_json::json!({ "schemaVersion": 3, "startNodeId": start, "nodes": nodes });
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = ?1 WHERE id = ?2",
                rusqlite::params![json.to_string(), story_id],
            )
            .expect("update structure");
    }

    #[test]
    fn a_story_with_choices_is_laid_out_by_its_own_graph_for_the_send_and_the_export() {
        let mut db = fresh_db();
        let id = story(&mut db);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        insert_asset(&db, &id, "aud-2", "audio", "aaaa2222.mp3");
        insert_asset(&db, &id, "aud-3", "audio", "aaaa3333.mp3");
        insert_asset(&db, &id, "img-1", "image", "bbbb1111.png");
        set_choice_structure(
            &db,
            &id,
            "n1",
            &[
                (
                    "n1",
                    Some("aud-1"),
                    Some("img-1"),
                    &[Some("n2"), Some("n3")],
                ),
                ("n2", Some("aud-2"), None, &[Some("n1")]),
                ("n3", Some("aud-3"), None, &[]),
            ],
        );

        // The send: the story's own graph, one stage per node.
        let pack = plan_story_pack(&db, &id).expect("send plan");
        let stages = &pack.stage_nodes;
        assert_eq!(
            stages.iter().map(|s| s.uuid.as_str()).collect::<Vec<_>>(),
            vec![id.as_str(), "n2", "n3"],
            "the start node is filed under the story id"
        );
        assert!(stages[0].square_one);
        assert_eq!(stages[0].audio.as_deref(), Some("aaaa1111.mp3"));
        assert_eq!(stages[0].image.as_deref(), Some("bbbb1111.png"));
        assert_eq!(stages[1].audio.as_deref(), Some("aaaa2222.mp3"));
        assert_eq!(stages[1].image, None);
        assert_eq!(stages[2].audio.as_deref(), Some("aaaa3333.mp3"));
        // The options are the wheel that follows the node.
        let wheel = &pack.action_nodes[0];
        assert_eq!(wheel.options, vec!["n2".to_string(), "n3".to_string()]);
        assert_eq!(
            stages[0]
                .ok_transition
                .as_ref()
                .map(|t| t.action_node.as_str()),
            Some(wheel.id.as_str())
        );
        // …and an option back to the start node points at the pack entry.
        assert_eq!(pack.action_nodes[1].options, vec![id.clone()]);
        // The device engine takes the graph as it is.
        assert!(crate::domain::device::transcode_pack(&pack).is_ok());

        // The export writes that very pack, with its title and stage names.
        let planned = plan_named_story_pack(&db, &id, SilentNodes::Carried).expect("export plan");
        assert_eq!(format!("{:?}", planned.pack), format!("{pack:?}"));
        assert_eq!(planned.title, "Série");
        let names: Vec<&str> = planned
            .pack
            .stage_nodes
            .iter()
            .map(|stage| planned.stage_names[&stage.uuid].as_str())
            .collect();
        assert_eq!(names, vec!["n1", "n2", "n3"]);
    }

    #[test]
    fn a_node_without_sound_is_refused_by_the_send_and_carried_by_the_export() {
        let mut db = fresh_db();
        let id = story(&mut db);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        set_choice_structure(
            &db,
            &id,
            "n1",
            &[
                ("n1", Some("aud-1"), None, &[Some("n2"), Some("n3")]),
                ("n2", Some("aud-1"), None, &[]),
                ("n3", None, None, &[]),
            ],
        );

        // The send: a device plays nothing else — refused, by node, with the
        // reason the library card announces before the click.
        let err = plan_story_pack(&db, &id).expect_err("refused");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(
            v["message"],
            "Envoi impossible: un ou plusieurs nœuds de cette histoire n'ont pas d'audio."
        );
        assert_eq!(
            v["userAction"],
            "Ajoute un audio à chaque nœud, puis réessaie l'envoi."
        );
        assert_eq!(v["details"]["source"], "story_pack");
        assert_eq!(v["details"]["cause"], "node_without_audio");
        assert_eq!(v["details"]["node"], "n3");
        assert_eq!(
            plan_named_story_pack(&db, &id, SilentNodes::Refused).unwrap_err(),
            StoryPackRefusal::NodeWithoutAudio {
                node_id: "n3".into()
            }
        );

        // The export: a node without any media is a stage all the same.
        let planned = plan_named_story_pack(&db, &id, SilentNodes::Carried).expect("laid out");
        let silent = &planned.pack.stage_nodes[2];
        assert_eq!(
            (silent.audio.as_deref(), silent.image.as_deref()),
            (None, None)
        );
        assert!(crate::domain::device::transcode_pack(&planned.pack).is_ok());
    }

    #[test]
    fn a_story_created_from_a_pack_is_laid_out_under_that_packs_identity() {
        const PACK: &str = "01a06ed9-2040-77c1-9e03-b8f429f4e954";
        const ENTRY: &str = "9c1f0e52-77aa-4b1d-8f39-0d6b1e2a3c44";
        let mut db = fresh_db();
        let id = story(&mut db);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        set_choice_structure(
            &db,
            &id,
            PACK,
            &[
                (ENTRY, Some("aud-1"), None, &[]),
                (PACK, Some("aud-1"), None, &[Some(ENTRY)]),
            ],
        );
        let planned = plan_named_story_pack(&db, &id, SilentNodes::Carried).expect("laid out");
        assert_eq!(
            planned
                .pack
                .stage_nodes
                .iter()
                .map(|s| s.uuid.as_str())
                .collect::<Vec<_>>(),
            vec![PACK, ENTRY],
            "the start node comes first and keeps the pack's own identity"
        );
    }

    #[test]
    fn an_option_without_a_destination_is_refused_by_node() {
        let mut db = fresh_db();
        let id = story(&mut db);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        // Not linked yet.
        set_choice_structure(
            &db,
            &id,
            "n1",
            &[
                ("n1", Some("aud-1"), None, &[Some("n2")]),
                ("n2", Some("aud-1"), None, &[None]),
            ],
        );
        for silent_nodes in [SilentNodes::Refused, SilentNodes::Carried] {
            assert_eq!(
                plan_named_story_pack(&db, &id, silent_nodes).unwrap_err(),
                StoryPackRefusal::OptionWithoutDestination {
                    node_id: "n2".into()
                }
            );
        }
        // The send words it for its own gesture, naming the node.
        let err = plan_story_pack(&db, &id).expect_err("refused");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(
            v["message"],
            "Envoi impossible: une option de cette histoire ne mène à aucun nœud."
        );
        assert_eq!(
            v["userAction"],
            "Rouvre l'histoire pour lier chaque option à un nœud (ou la retirer), puis réessaie l'envoi."
        );
        assert_eq!(v["details"]["cause"], "option_without_destination");
        assert_eq!(v["details"]["node"], "n2");
        // Linked to a node that is gone.
        set_choice_structure(
            &db,
            &id,
            "n1",
            &[("n1", Some("aud-1"), None, &[Some("ghost")])],
        );
        assert_eq!(
            plan_named_story_pack(&db, &id, SilentNodes::Carried).unwrap_err(),
            StoryPackRefusal::OptionWithoutDestination {
                node_id: "n1".into()
            }
        );
    }

    #[test]
    fn a_story_with_choices_needs_its_sounds_backed_but_not_its_pictures() {
        let mut db = fresh_db();
        let id = story(&mut db);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        // An image id that names an AUDIO row is not a picture.
        set_choice_structure(
            &db,
            &id,
            "n1",
            &[
                ("n1", Some("aud-1"), Some("img-gone"), &[Some("n2")]),
                ("n2", Some("aud-1"), Some("aud-1"), &[]),
            ],
        );
        let planned = plan_named_story_pack(&db, &id, SilentNodes::Carried).expect("laid out");
        assert_eq!(planned.pack.stage_nodes[0].image, None);
        assert_eq!(planned.pack.stage_nodes[1].image, None);

        set_choice_structure(
            &db,
            &id,
            "n1",
            &[
                ("n1", Some("aud-1"), None, &[Some("n2")]),
                ("n2", Some("aud-gone"), None, &[]),
            ],
        );
        assert_eq!(
            plan_named_story_pack(&db, &id, SilentNodes::Carried).unwrap_err(),
            StoryPackRefusal::AssetMissing {
                node_id: "n2".into()
            }
        );
    }

    #[test]
    fn the_presentation_does_not_apply_to_a_story_with_choices() {
        let mut db = fresh_db();
        let id = story(&mut db);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        set_choice_structure(
            &db,
            &id,
            "n1",
            &[
                ("n1", Some("aud-1"), None, &[Some("n2")]),
                ("n2", Some("aud-1"), None, &[]),
            ],
        );
        // `Au choix` without a single announcement: it would block an
        // episode list, it is irrelevant here.
        set_menu(&db, &id, None, &[]);
        let planned = plan_named_story_pack(&db, &id, SilentNodes::Carried).expect("laid out");
        assert_eq!(planned.pack.stage_nodes.len(), 2);
    }

    #[test]
    fn the_silent_node_policy_only_concerns_stories_with_choices() {
        let mut db = fresh_db();
        let id = story(&mut db);
        set_structure(&db, &id, &[("n1", Some("aud-1"), None)]);
        insert_asset(&db, &id, "aud-1", "audio", "aaaa1111.mp3");
        let uuids = |silent_nodes: SilentNodes| -> Vec<String> {
            plan_named_story_pack(&db, &id, silent_nodes)
                .expect("plan")
                .pack
                .stage_nodes
                .iter()
                .map(|stage| stage.uuid.clone())
                .collect()
        };
        assert_eq!(uuids(SilentNodes::Carried), uuids(SilentNodes::Refused));

        // An episode always needs its audio: no policy carries a silent one.
        set_structure(&db, &id, &[("n1", Some("aud-1"), None), ("n2", None, None)]);
        for silent_nodes in [SilentNodes::Refused, SilentNodes::Carried] {
            assert_eq!(
                plan_named_story_pack(&db, &id, silent_nodes).unwrap_err(),
                StoryPackRefusal::Blocked(StoryPackBlocker::MissingAudio)
            );
        }
    }
}
