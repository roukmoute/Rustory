//! Plan the device pack of a LIBRARY story (one without a retained source
//! archive) from the local database — the read side of
//! [`send_story_pack_to_device`](super::send::send_story_pack_to_device).
//!
//! Runs UNDER the SQLite lock and does no other I/O: it reads the story's
//! canonical structure, its presentation (layout + spoken announcements)
//! and the stored file names of the media they reference, then hands the
//! pure synthesis (`domain::device::story_pack`) the ordered episodes —
//! chained in sequence, or behind a spoken menu. Every refusal is decided
//! here, BEFORE any device is touched, with the same actionable copy the
//! library card announces pre-click.

use crate::domain::device::{
    linear_episodes, menu_blocker, synthesize_menu_pack, synthesize_sequential_pack, EpisodeAssets,
    MenuEpisode, MenuPackAssets, StoryLayout, StoryPackBlocker, StudioStoryPack,
};
use crate::domain::shared::AppError;
use crate::domain::story::CanonicalStructure;
use crate::infrastructure::db::DbHandle;

/// Build the device pack of `story_id` (per its layout), or refuse with the reason.
pub fn plan_story_pack(db: &DbHandle, story_id: &str) -> Result<StudioStoryPack, AppError> {
    let structure_json: String = db
        .conn()
        .query_row(
            "SELECT structure_json FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| row.get(0),
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => story_not_found_error(),
            _ => storage_error("read_story"),
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
        .map_err(|_| storage_error("read_provenance"))?;
    if device_pack {
        return Err(device_pack_error());
    }

    let structure: CanonicalStructure = serde_json::from_str(&structure_json)
        .map_err(|_| blocker_error(StoryPackBlocker::Malformed))?;
    let episodes = linear_episodes(&structure).map_err(blocker_error)?;

    let mut assets = Vec::with_capacity(episodes.len());
    for episode in &episodes {
        let audio_ref = stored_file_name(db, story_id, episode.audio_asset_id, "audio")?
            .ok_or_else(|| asset_missing_error(episode.node_id))?;
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
    match presentation.layout {
        StoryLayout::Sequential => Ok(synthesize_sequential_pack(story_id, &assets)),
        StoryLayout::Menu => {
            // The spoken announcements: refused as a whole while any is
            // missing (the same rule the card projects).
            let has_prompt = |node_id: &str| presentation.prompts.iter().any(|(n, _)| n == node_id);
            if let Some(blocker) = menu_blocker(
                &episodes,
                presentation.question_asset_id.is_some(),
                has_prompt,
            ) {
                return Err(blocker_error(blocker));
            }
            let question_asset = presentation
                .question_asset_id
                .as_deref()
                .ok_or_else(|| blocker_error(StoryPackBlocker::MissingAnnouncements))?;
            let question_ref = stored_file_name(db, story_id, question_asset, "audio")?
                .ok_or_else(|| announcement_missing_error("question"))?;
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
                    .ok_or_else(|| blocker_error(StoryPackBlocker::MissingAnnouncements))?;
                let prompt_ref = stored_file_name(db, story_id, prompt_asset, "audio")?
                    .ok_or_else(|| announcement_missing_error(episode.node_id))?;
                menu_episodes.push(MenuEpisode {
                    audio_ref: media.audio_ref,
                    image_ref: media.image_ref,
                    prompt_ref,
                });
            }
            Ok(synthesize_menu_pack(
                story_id,
                &MenuPackAssets {
                    title_ref,
                    question_ref,
                    episodes: menu_episodes,
                },
            ))
        }
    }
}

/// The presentation rows a plan needs: the layout, the spoken title and
/// question asset ids, and the (node id → asset id) spoken titles.
struct PresentationRows {
    layout: StoryLayout,
    title_asset_id: Option<String>,
    question_asset_id: Option<String>,
    prompts: Vec<(String, String)>,
}

fn read_presentation_rows(db: &DbHandle, story_id: &str) -> Result<PresentationRows, AppError> {
    use rusqlite::OptionalExtension;
    let layout_row: Option<(String, Option<String>, Option<String>)> = db
        .conn()
        .query_row(
            "SELECT layout, title_audio_asset_id, question_audio_asset_id FROM story_layouts WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|_| storage_error("read_layout"))?;
    let (layout, title_asset_id, question_asset_id) = match layout_row {
        Some((tag, title, question)) => (
            StoryLayout::parse(&tag).unwrap_or_default(),
            title,
            question,
        ),
        None => (StoryLayout::Sequential, None, None),
    };
    let mut stmt = db
        .conn()
        .prepare("SELECT node_id, audio_asset_id FROM story_node_prompts WHERE story_id = ?1")
        .map_err(|_| storage_error("read_prompts"))?;
    let prompts = stmt
        .query_map(rusqlite::params![story_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| storage_error("read_prompts"))?
        .collect::<Result<Vec<(String, String)>, _>>()
        .map_err(|_| storage_error("read_prompts"))?;
    Ok(PresentationRows {
        layout,
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
) -> Result<Option<String>, AppError> {
    db.conn()
        .query_row(
            "SELECT file_name FROM assets WHERE id = ?1 AND story_id = ?2 AND media_type = ?3",
            rusqlite::params![asset_id, story_id, media_type],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            _ => Err(storage_error("read_asset")),
        })
}

/// The story could not be laid out as a device pack — the same closed
/// reasons the library card announces, with an actionable next step.
fn blocker_error(blocker: StoryPackBlocker) -> AppError {
    let (message, action) = match blocker {
        StoryPackBlocker::Empty => (
            "Envoi impossible: cette histoire n'a aucun épisode.",
            "Ajoute au moins un épisode avec un audio, puis réessaie l'envoi.",
        ),
        StoryPackBlocker::Malformed => (
            "Envoi impossible: la structure de cette histoire est illisible.",
            "Rouvre l'histoire pour la vérifier, puis réessaie l'envoi.",
        ),
        StoryPackBlocker::Branching => (
            "Envoi impossible: les histoires à choix ne sont pas encore prises en charge pour l'envoi.",
            "Envoie une histoire dont les épisodes s'enchaînent sans choix.",
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
}
