//! Plan the device pack of a LIBRARY story (one without a retained source
//! archive) from the local database — the read side of
//! [`send_story_pack_to_device`](super::send::send_story_pack_to_device).
//!
//! Runs UNDER the SQLite lock and does no other I/O: it reads the story's
//! canonical structure and the stored file names of the media it
//! references, then hands the pure synthesis
//! (`domain::device::story_pack`) the ordered episodes. Every refusal is
//! decided here, BEFORE any device is touched, with the same actionable
//! copy the library card announces pre-click.

use crate::domain::device::{
    linear_episodes, synthesize_sequential_pack, EpisodeAssets, StoryPackBlocker, StudioStoryPack,
};
use crate::domain::shared::AppError;
use crate::domain::story::CanonicalStructure;
use crate::infrastructure::db::DbHandle;

/// Build the sequential device pack of `story_id`, or refuse with the reason.
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
    Ok(synthesize_sequential_pack(story_id, &assets))
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
    };
    AppError::device_write_failed(message, action).with_details(serde_json::json!({
        "source": "story_pack",
        "cause": blocker.diagnostic_tag(),
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
}
