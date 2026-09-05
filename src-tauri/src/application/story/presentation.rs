//! How a story is PRESENTED on a device — its layout (episodes in sequence,
//! or a spoken menu) and the spoken ANNOUNCEMENTS the menu needs: the series
//! title on the cover, the question, one spoken title per episode.
//!
//! The announcements are ordinary story audio assets (node-media store +
//! `assets` rows), referenced from `story_layouts` / `story_node_prompts`
//! with the exact text that was spoken and the voice that spoke it, so the
//! state is always derivable: READY (text and voice match), STALE (the label
//! or the voice changed since) or MISSING. Generation is planned UNDER the
//! DB lock without I/O ([`plan_announcements`]), synthesized and promoted
//! OFF the lock by the command (the voice engine, then the media store),
//! and committed clip by clip UNDER the lock ([`commit_announcement`]) —
//! a failed clip leaves every other clip intact, never a half-written row.

use std::path::Path;

use rusqlite::OptionalExtension;

use crate::domain::device::{linear_episodes, StoryLayout};
use crate::domain::shared::AppError;
use crate::domain::speech::{spoken_episode_title, spoken_series_title, MENU_QUESTION};
use crate::domain::story::CanonicalStructure;
use crate::infrastructure::db::DbHandle;

use super::node::{gc_unreferenced_media_file, PreparedMedia};
use super::now_iso_ms;

/// The state of one announcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnouncementStatus {
    /// Generated for the current text with the current voice.
    Ready,
    /// Generated, but the text or the voice changed since.
    Stale,
    /// Never generated.
    Missing,
}

impl AnnouncementStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Stale => "stale",
            Self::Missing => "missing",
        }
    }
}

/// One announcement: what will be said, and whether it already is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub spoken_text: String,
    pub status: AnnouncementStatus,
    pub asset_id: Option<String>,
}

/// An episode's announcement, keyed by its node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterAnnouncement {
    pub node_id: String,
    pub label: String,
    pub announcement: Announcement,
}

/// The whole presentation read-model of a story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryPresentation {
    pub layout: StoryLayout,
    /// The voice the stored announcements were generated with, if any.
    pub voice_id: Option<String>,
    /// Whether the story's structure lays out as episodes at all (a story
    /// with choices, or without audio, has no menu to announce).
    pub linear: bool,
    pub title: Announcement,
    pub question: Announcement,
    pub chapters: Vec<ChapterAnnouncement>,
}

/// Which announcement a clip is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnouncementTarget {
    Title,
    Question,
    Chapter { node_id: String },
}

/// A clip to synthesize: its target and the exact text to speak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedClip {
    pub target: AnnouncementTarget,
    pub spoken_text: String,
}

/// Read the presentation of `story_id`, judging each announcement against
/// `selected_voice_id` (the voice the user would generate with now): a clip
/// spoken by another voice is STALE.
pub fn read_presentation(
    db: &DbHandle,
    story_id: &str,
    selected_voice_id: Option<&str>,
) -> Result<StoryPresentation, AppError> {
    let (title, structure) = read_story(db, story_id)?;
    let layout_row = read_layout_row(db, story_id)?;
    let layout = layout_row.as_ref().map(|r| r.layout).unwrap_or_default();
    let voice_id = layout_row.as_ref().and_then(|r| r.voice_id.clone());
    let judge = |spoken_now: &str, stored: Option<(&str, &str, &str)>| -> Announcement {
        match stored {
            None => Announcement {
                spoken_text: spoken_now.to_string(),
                status: AnnouncementStatus::Missing,
                asset_id: None,
            },
            Some((asset_id, spoken_then, voice_then)) => {
                let same_voice = selected_voice_id.is_none_or(|v| v == voice_then);
                Announcement {
                    spoken_text: spoken_now.to_string(),
                    status: if spoken_then == spoken_now && same_voice {
                        AnnouncementStatus::Ready
                    } else {
                        AnnouncementStatus::Stale
                    },
                    asset_id: Some(asset_id.to_string()),
                }
            }
        }
    };

    let title_now = spoken_series_title(&title);
    let title_ann = judge(
        &title_now,
        layout_row.as_ref().and_then(|r| {
            Some((
                r.title_audio_asset_id.as_deref()?,
                r.title_spoken_text.as_deref()?,
                r.voice_id.as_deref()?,
            ))
        }),
    );
    let question_ann = judge(
        MENU_QUESTION,
        layout_row.as_ref().and_then(|r| {
            Some((
                r.question_audio_asset_id.as_deref()?,
                r.question_spoken_text.as_deref()?,
                r.voice_id.as_deref()?,
            ))
        }),
    );

    let prompts = read_prompt_rows(db, story_id)?;
    let (linear, chapters) = match structure.as_ref().and_then(|s| linear_episodes(s).ok()) {
        Some(episodes) => (
            true,
            episodes
                .iter()
                .enumerate()
                .map(|(position, episode)| {
                    // A label that speaks to nothing still announces its
                    // position, so the wheel is never silent.
                    let mut spoken_now = spoken_episode_title(episode.label);
                    if spoken_now.is_empty() {
                        spoken_now = format!("Épisode {}.", position + 1);
                    }
                    let stored = prompts
                        .iter()
                        .find(|p| p.node_id == episode.node_id)
                        .map(|p| {
                            (
                                p.audio_asset_id.as_str(),
                                p.spoken_text.as_str(),
                                p.voice_id.as_str(),
                            )
                        });
                    ChapterAnnouncement {
                        node_id: episode.node_id.to_string(),
                        label: episode.label.to_string(),
                        announcement: judge(&spoken_now, stored),
                    }
                })
                .collect(),
        ),
        None => (false, Vec::new()),
    };

    Ok(StoryPresentation {
        layout,
        voice_id,
        linear,
        title: title_ann,
        question: question_ann,
        chapters,
    })
}

/// Set the layout of `story_id` (creates the row on first use).
pub fn set_layout(db: &DbHandle, story_id: &str, layout: StoryLayout) -> Result<(), AppError> {
    ensure_story(db, story_id)?;
    let now = now_iso_ms()?;
    db.conn()
        .execute(
            "INSERT INTO story_layouts (story_id, layout, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(story_id) DO UPDATE SET layout = excluded.layout, updated_at = excluded.updated_at",
            rusqlite::params![story_id, layout.as_str(), now],
        )
        .map_err(|e| storage_error(&e, "set_layout"))?;
    Ok(())
}

/// The clips to synthesize with `voice_id` so every announcement of
/// `story_id` is READY: the missing and stale ones (all of them when the
/// voice differs from the one on file, or when `force`).
pub fn plan_announcements(
    db: &DbHandle,
    story_id: &str,
    voice_id: &str,
    force: bool,
) -> Result<Vec<PlannedClip>, AppError> {
    let presentation = read_presentation(db, story_id, Some(voice_id))?;
    if !presentation.linear {
        return Err(not_linear_error());
    }
    let mut clips = Vec::new();
    let wants = |a: &Announcement| force || a.status != AnnouncementStatus::Ready;
    if wants(&presentation.title) && !presentation.title.spoken_text.is_empty() {
        clips.push(PlannedClip {
            target: AnnouncementTarget::Title,
            spoken_text: presentation.title.spoken_text.clone(),
        });
    }
    if wants(&presentation.question) {
        clips.push(PlannedClip {
            target: AnnouncementTarget::Question,
            spoken_text: presentation.question.spoken_text.clone(),
        });
    }
    for chapter in &presentation.chapters {
        if wants(&chapter.announcement) {
            clips.push(PlannedClip {
                target: AnnouncementTarget::Chapter {
                    node_id: chapter.node_id.clone(),
                },
                spoken_text: chapter.announcement.spoken_text.clone(),
            });
        }
    }
    Ok(clips)
}

/// Commit one synthesized, PROMOTED clip for `target`: the asset row, the
/// announcement row (replacing the previous clip, whose file is GC'd after
/// commit), all in one transaction. A failure compensates the promoted file.
pub fn commit_announcement(
    db: &mut DbHandle,
    story_id: &str,
    target: &AnnouncementTarget,
    prepared: PreparedMedia,
    spoken_text: &str,
    voice_id: &str,
) -> Result<String, AppError> {
    let PreparedMedia { stored, media_dir } = prepared;
    let promoted = (stored.content_hash.clone(), stored.file_name.clone());
    let asset_id = uuid::Uuid::now_v7().to_string();
    let now = now_iso_ms()?;

    let result = (|| -> Result<Option<(String, String)>, AppError> {
        ensure_story(db, story_id)?;
        let tx = db
            .conn_mut()
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| storage_error(&e, "begin"))?;
        // The previous clip for this target, if any — reclaimed below.
        let previous_asset: Option<String> = match target {
            AnnouncementTarget::Title => tx
                .query_row(
                    "SELECT title_audio_asset_id FROM story_layouts WHERE story_id = ?1",
                    rusqlite::params![story_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| storage_error(&e, "read_layout"))?
                .flatten(),
            AnnouncementTarget::Question => tx
                .query_row(
                    "SELECT question_audio_asset_id FROM story_layouts WHERE story_id = ?1",
                    rusqlite::params![story_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| storage_error(&e, "read_layout"))?
                .flatten(),
            AnnouncementTarget::Chapter { node_id } => tx
                .query_row(
                    "SELECT audio_asset_id FROM story_node_prompts WHERE story_id = ?1 AND node_id = ?2",
                    rusqlite::params![story_id, node_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| storage_error(&e, "read_prompt"))?,
        };
        let previous_info: Option<(String, String)> = match previous_asset.as_deref() {
            Some(old) => {
                let info = tx
                    .query_row(
                        "SELECT content_hash, file_name FROM assets WHERE id = ?1 AND story_id = ?2",
                        rusqlite::params![old, story_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()
                    .map_err(|e| storage_error(&e, "read_assets"))?;
                tx.execute(
                    "DELETE FROM assets WHERE id = ?1 AND story_id = ?2",
                    rusqlite::params![old, story_id],
                )
                .map_err(|e| storage_error(&e, "delete_assets"))?;
                info
            }
            None => None,
        };
        tx.execute(
            "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                asset_id,
                story_id,
                stored.content_hash,
                stored.kind.as_str(),
                stored.format,
                stored.byte_size,
                stored.file_name,
                now,
            ],
        )
        .map_err(|e| storage_error(&e, "insert_asset"))?;
        match target {
            AnnouncementTarget::Title => {
                tx.execute(
                    "INSERT INTO story_layouts (story_id, layout, title_audio_asset_id, title_spoken_text, voice_id, updated_at) \
                     VALUES (?1, 'sequential', ?2, ?3, ?4, ?5) \
                     ON CONFLICT(story_id) DO UPDATE SET title_audio_asset_id = excluded.title_audio_asset_id, \
                     title_spoken_text = excluded.title_spoken_text, voice_id = excluded.voice_id, updated_at = excluded.updated_at",
                    rusqlite::params![story_id, asset_id, spoken_text, voice_id, now],
                )
                .map_err(|e| storage_error(&e, "upsert_layout"))?;
            }
            AnnouncementTarget::Question => {
                tx.execute(
                    "INSERT INTO story_layouts (story_id, layout, question_audio_asset_id, question_spoken_text, voice_id, updated_at) \
                     VALUES (?1, 'sequential', ?2, ?3, ?4, ?5) \
                     ON CONFLICT(story_id) DO UPDATE SET question_audio_asset_id = excluded.question_audio_asset_id, \
                     question_spoken_text = excluded.question_spoken_text, voice_id = excluded.voice_id, updated_at = excluded.updated_at",
                    rusqlite::params![story_id, asset_id, spoken_text, voice_id, now],
                )
                .map_err(|e| storage_error(&e, "upsert_layout"))?;
            }
            AnnouncementTarget::Chapter { node_id } => {
                tx.execute(
                    "INSERT INTO story_node_prompts (story_id, node_id, audio_asset_id, spoken_text, voice_id, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                     ON CONFLICT(story_id, node_id) DO UPDATE SET audio_asset_id = excluded.audio_asset_id, \
                     spoken_text = excluded.spoken_text, voice_id = excluded.voice_id, updated_at = excluded.updated_at",
                    rusqlite::params![story_id, node_id, asset_id, spoken_text, voice_id, now],
                )
                .map_err(|e| storage_error(&e, "upsert_prompt"))?;
                // The voice on file for the story follows the last clip.
                tx.execute(
                    "INSERT INTO story_layouts (story_id, layout, voice_id, updated_at) VALUES (?1, 'sequential', ?2, ?3) \
                     ON CONFLICT(story_id) DO UPDATE SET voice_id = excluded.voice_id, updated_at = excluded.updated_at",
                    rusqlite::params![story_id, voice_id, now],
                )
                .map_err(|e| storage_error(&e, "upsert_layout"))?;
            }
        }
        tx.commit().map_err(|e| storage_error(&e, "commit"))?;
        Ok(previous_info)
    })();

    match result {
        Ok(previous_info) => {
            gc_unreferenced_media_file(db, &media_dir, previous_info);
            Ok(asset_id)
        }
        Err(err) => {
            gc_unreferenced_media_file(db, &media_dir, Some(promoted));
            Err(err)
        }
    }
}

/// Drop the prompt rows of nodes that no longer exist (a structure edited
/// outside `delete_node`, e.g. an artifact re-import) — their assets rows and
/// files with them. Best-effort housekeeping before a plan.
pub fn sweep_orphan_prompts(db: &mut DbHandle, app_data_dir: &Path, story_id: &str) {
    let Ok((_, structure)) = read_story(db, story_id) else {
        return;
    };
    let node_ids: Vec<String> = structure
        .map(|s| s.nodes.iter().map(|n| n.id.clone()).collect())
        .unwrap_or_default();
    let Ok(prompts) = read_prompt_rows(db, story_id) else {
        return;
    };
    let media_dir = crate::infrastructure::filesystem::resolve_node_media_dir(app_data_dir);
    for prompt in prompts.iter().filter(|p| !node_ids.contains(&p.node_id)) {
        let info: Option<(String, String)> = db
            .conn()
            .query_row(
                "SELECT content_hash, file_name FROM assets WHERE id = ?1 AND story_id = ?2",
                rusqlite::params![prompt.audio_asset_id, story_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap_or(None);
        let _ = db.conn().execute(
            "DELETE FROM assets WHERE id = ?1 AND story_id = ?2",
            rusqlite::params![prompt.audio_asset_id, story_id],
        );
        let _ = db.conn().execute(
            "DELETE FROM story_node_prompts WHERE story_id = ?1 AND node_id = ?2",
            rusqlite::params![story_id, prompt.node_id],
        );
        gc_unreferenced_media_file(db, &media_dir, info);
    }
}

// ===== rows =====

struct LayoutRow {
    layout: StoryLayout,
    title_audio_asset_id: Option<String>,
    title_spoken_text: Option<String>,
    question_audio_asset_id: Option<String>,
    question_spoken_text: Option<String>,
    voice_id: Option<String>,
}

pub(crate) struct PromptRow {
    pub(crate) node_id: String,
    pub(crate) audio_asset_id: String,
    pub(crate) spoken_text: String,
    pub(crate) voice_id: String,
}

fn read_layout_row(db: &DbHandle, story_id: &str) -> Result<Option<LayoutRow>, AppError> {
    db.conn()
        .query_row(
            "SELECT layout, title_audio_asset_id, title_spoken_text, question_audio_asset_id, question_spoken_text, voice_id \
             FROM story_layouts WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| {
                Ok(LayoutRow {
                    layout: StoryLayout::parse(&r.get::<_, String>(0)?).unwrap_or_default(),
                    title_audio_asset_id: r.get(1)?,
                    title_spoken_text: r.get(2)?,
                    question_audio_asset_id: r.get(3)?,
                    question_spoken_text: r.get(4)?,
                    voice_id: r.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|e| storage_error(&e, "read_layout"))
}

pub(crate) fn read_prompt_rows(db: &DbHandle, story_id: &str) -> Result<Vec<PromptRow>, AppError> {
    let mut stmt = db
        .conn()
        .prepare("SELECT node_id, audio_asset_id, spoken_text, voice_id FROM story_node_prompts WHERE story_id = ?1")
        .map_err(|e| storage_error(&e, "read_prompts"))?;
    let rows = stmt
        .query_map(rusqlite::params![story_id], |r| {
            Ok(PromptRow {
                node_id: r.get(0)?,
                audio_asset_id: r.get(1)?,
                spoken_text: r.get(2)?,
                voice_id: r.get(3)?,
            })
        })
        .map_err(|e| storage_error(&e, "read_prompts"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_error(&e, "read_prompts"))
}

/// The story's title and parsed structure (`None` when the JSON is not a
/// canonical v3 graph — then nothing lays out).
fn read_story(
    db: &DbHandle,
    story_id: &str,
) -> Result<(String, Option<CanonicalStructure>), AppError> {
    let (title, json): (String, String) = db
        .conn()
        .query_row(
            "SELECT title, structure_json FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => story_not_found(),
            other => storage_error(&other, "read_story"),
        })?;
    Ok((title, serde_json::from_str(&json).ok()))
}

fn ensure_story(db: &DbHandle, story_id: &str) -> Result<(), AppError> {
    read_story(db, story_id).map(|_| ())
}

// ===== errors =====

fn story_not_found() -> AppError {
    AppError::library_inconsistent(
        "Histoire introuvable, recharge la bibliothèque.",
        "Retourne à la bibliothèque et recharge la liste.",
    )
    .with_details(serde_json::json!({ "source": "presentation", "cause": "not_found" }))
}

fn not_linear_error() -> AppError {
    AppError::library_inconsistent(
        "Les annonces ne peuvent pas être générées : cette histoire n'a pas d'épisodes à annoncer.",
        "Donne un audio à chaque épisode et retire les choix, puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "presentation", "cause": "not_linear" }))
}

fn storage_error(_err: &rusqlite::Error, stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu lire ta bibliothèque locale.",
        "Relance l'application ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "presentation", "stage": stage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::node::store_node_media;
    use crate::application::story::{create_story, CreateStoryInput};
    use crate::infrastructure::db::{open_in_memory, run_migrations};
    use crate::infrastructure::filesystem::MediaKind;

    fn fresh_db() -> DbHandle {
        let mut db = open_in_memory().expect("open");
        run_migrations(&mut db).expect("migrate");
        db
    }

    fn story(db: &mut DbHandle, title: &str) -> String {
        create_story(
            db,
            CreateStoryInput {
                title: title.into(),
            },
        )
        .expect("create")
        .id
    }

    fn set_structure(db: &DbHandle, story_id: &str, labels: &[(&str, &str)]) {
        let nodes: Vec<serde_json::Value> = labels
            .iter()
            .map(|(id, label)| {
                serde_json::json!({
                    "id": id, "text": "", "label": label,
                    "imageAssetId": null, "audioAssetId": format!("aud-{id}"), "options": [],
                })
            })
            .collect();
        let structure: CanonicalStructure = serde_json::from_value(
            serde_json::json!({ "schemaVersion": 3, "startNodeId": "n1", "nodes": nodes }),
        )
        .expect("canonical");
        let json = crate::domain::story::canonical_structure_json(&structure);
        let checksum = crate::domain::story::content_checksum(&json);
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = ?1, content_checksum = ?2 WHERE id = ?3",
                rusqlite::params![json, checksum, story_id],
            )
            .expect("structure");
    }

    /// A tiny valid WAV (44-byte header + a few samples) the store sniffs as
    /// audio — stands in for a synthesized clip.
    fn wav_clip(seed: u8) -> Vec<u8> {
        crate::infrastructure::device::audio_transcode::test_support::wav_sine(
            8_000,
            1,
            440.0 + seed as f64,
            0.05,
            0.3,
        )
    }

    #[test]
    fn a_fresh_story_is_sequential_with_every_announcement_missing() {
        let mut db = fresh_db();
        let id = story(&mut db, "Tina et le serpent à plumes");
        set_structure(
            &db,
            &id,
            &[
                ("n1", "Le trésor : épisode 1/2"),
                ("n2", "La flûte : épisode 2/2"),
            ],
        );
        let p = read_presentation(&db, &id, Some("voice-a")).expect("read");
        assert_eq!(p.layout, StoryLayout::Sequential);
        assert!(p.linear);
        assert_eq!(p.title.spoken_text, "Tina et le serpent à plumes.");
        assert_eq!(p.title.status, AnnouncementStatus::Missing);
        assert_eq!(p.question.spoken_text, MENU_QUESTION);
        assert_eq!(p.question.status, AnnouncementStatus::Missing);
        assert_eq!(p.chapters.len(), 2);
        assert_eq!(
            p.chapters[0].announcement.spoken_text,
            "Épisode 1. Le trésor."
        );
        assert_eq!(
            p.chapters[1].announcement.spoken_text,
            "Épisode 2. La flûte."
        );
        assert!(p
            .chapters
            .iter()
            .all(|c| c.announcement.status == AnnouncementStatus::Missing));
    }

    #[test]
    fn the_layout_can_be_set_and_read_back() {
        let mut db = fresh_db();
        let id = story(&mut db, "Série");
        set_layout(&db, &id, StoryLayout::Menu).expect("set");
        assert_eq!(
            read_presentation(&db, &id, None).unwrap().layout,
            StoryLayout::Menu
        );
        set_layout(&db, &id, StoryLayout::Sequential).expect("set");
        assert_eq!(
            read_presentation(&db, &id, None).unwrap().layout,
            StoryLayout::Sequential
        );
        assert!(set_layout(&db, "nope", StoryLayout::Menu).is_err());
    }

    #[test]
    fn the_plan_lists_every_missing_clip_then_only_the_stale_ones() {
        let mut db = fresh_db();
        let app_data = tempfile::tempdir().expect("app data");
        let id = story(&mut db, "Série");
        set_structure(&db, &id, &[("n1", "Un"), ("n2", "")]);
        let plan = plan_announcements(&db, &id, "voice-a", false).expect("plan");
        assert_eq!(plan.len(), 4, "title + question + 2 chapters");
        assert_eq!(plan[0].target, AnnouncementTarget::Title);
        assert_eq!(plan[1].target, AnnouncementTarget::Question);
        assert_eq!(
            plan[3].spoken_text, "Épisode 2.",
            "an empty label speaks its position"
        );

        // Commit every clip.
        for (i, clip) in plan.iter().enumerate() {
            let prepared = store_node_media(app_data.path(), MediaKind::Audio, &wav_clip(i as u8))
                .expect("store");
            commit_announcement(
                &mut db,
                &id,
                &clip.target,
                prepared,
                &clip.spoken_text,
                "voice-a",
            )
            .expect("commit");
        }
        let p = read_presentation(&db, &id, Some("voice-a")).expect("read");
        assert_eq!(p.title.status, AnnouncementStatus::Ready);
        assert_eq!(p.question.status, AnnouncementStatus::Ready);
        assert!(p
            .chapters
            .iter()
            .all(|c| c.announcement.status == AnnouncementStatus::Ready));
        assert_eq!(p.voice_id.as_deref(), Some("voice-a"));
        assert!(plan_announcements(&db, &id, "voice-a", false)
            .unwrap()
            .is_empty());
        let assets: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE story_id = ?1",
                rusqlite::params![&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(assets, 4);

        // Rename a chapter: only its clip is stale, and re-planned.
        set_structure(&db, &id, &[("n1", "Un bis"), ("n2", "")]);
        let p = read_presentation(&db, &id, Some("voice-a")).expect("read");
        assert_eq!(p.chapters[0].announcement.status, AnnouncementStatus::Stale);
        assert_eq!(p.chapters[1].announcement.status, AnnouncementStatus::Ready);
        let plan = plan_announcements(&db, &id, "voice-a", false).expect("plan");
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].target,
            AnnouncementTarget::Chapter {
                node_id: "n1".into()
            }
        );

        // Another voice: everything is stale for it; `force` too.
        assert_eq!(
            plan_announcements(&db, &id, "voice-b", false)
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            plan_announcements(&db, &id, "voice-a", true).unwrap().len(),
            4
        );
    }

    #[test]
    fn committing_a_clip_again_replaces_the_previous_asset_and_its_file() {
        let mut db = fresh_db();
        let app_data = tempfile::tempdir().expect("app data");
        let id = story(&mut db, "Série");
        set_structure(&db, &id, &[("n1", "Un")]);
        let target = AnnouncementTarget::Chapter {
            node_id: "n1".into(),
        };
        let first =
            store_node_media(app_data.path(), MediaKind::Audio, &wav_clip(1)).expect("store");
        let first_file = first.stored.file_name.clone();
        let a1 = commit_announcement(&mut db, &id, &target, first, "Un.", "v").expect("commit");
        let second =
            store_node_media(app_data.path(), MediaKind::Audio, &wav_clip(2)).expect("store");
        let a2 = commit_announcement(&mut db, &id, &target, second, "Un.", "v").expect("commit");
        assert_ne!(a1, a2);
        let assets: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE story_id = ?1",
                rusqlite::params![&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(assets, 1, "the replaced asset row is gone");
        let media_dir = crate::infrastructure::filesystem::resolve_node_media_dir(app_data.path());
        assert!(
            !media_dir.join(first_file).exists(),
            "the replaced file is GC'd"
        );
    }

    #[test]
    fn deleting_a_node_takes_its_prompt_and_asset_along() {
        use crate::application::story::structure::delete_node;
        let mut db = fresh_db();
        let app_data = tempfile::tempdir().expect("app data");
        let id = story(&mut db, "Série");
        set_structure(&db, &id, &[("n1", "Un"), ("n2", "Deux")]);
        let prepared =
            store_node_media(app_data.path(), MediaKind::Audio, &wav_clip(3)).expect("store");
        let file = prepared.stored.file_name.clone();
        commit_announcement(
            &mut db,
            &id,
            &AnnouncementTarget::Chapter {
                node_id: "n2".into(),
            },
            prepared,
            "Deux.",
            "v",
        )
        .expect("commit");
        delete_node(&mut db, app_data.path(), &id, "n2").expect("delete");
        let prompts: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM story_node_prompts WHERE story_id = ?1",
                rusqlite::params![&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(prompts, 0);
        let assets: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE story_id = ?1",
                rusqlite::params![&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(assets, 0);
        let media_dir = crate::infrastructure::filesystem::resolve_node_media_dir(app_data.path());
        assert!(!media_dir.join(file).exists());
    }

    #[test]
    fn orphan_prompts_are_swept() {
        let mut db = fresh_db();
        let app_data = tempfile::tempdir().expect("app data");
        let id = story(&mut db, "Série");
        set_structure(&db, &id, &[("n1", "Un"), ("n2", "Deux")]);
        let prepared =
            store_node_media(app_data.path(), MediaKind::Audio, &wav_clip(4)).expect("store");
        commit_announcement(
            &mut db,
            &id,
            &AnnouncementTarget::Chapter {
                node_id: "n2".into(),
            },
            prepared,
            "Deux.",
            "v",
        )
        .expect("commit");
        // The structure loses n2 behind the prompt's back.
        set_structure(&db, &id, &[("n1", "Un")]);
        sweep_orphan_prompts(&mut db, app_data.path(), &id);
        let prompts: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM story_node_prompts WHERE story_id = ?1",
                rusqlite::params![&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(prompts, 0);
    }

    #[test]
    fn a_story_with_choices_has_no_announcements_to_plan() {
        let mut db = fresh_db();
        let id = story(&mut db, "Aventure");
        let json = serde_json::json!({ "schemaVersion": 3, "startNodeId": "n1", "nodes": [
            { "id": "n1", "text": "", "label": "Un", "imageAssetId": null, "audioAssetId": "a", "options": [{ "label": "x", "target": null }] }
        ] });
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = ?1 WHERE id = ?2",
                rusqlite::params![json.to_string(), &id],
            )
            .unwrap();
        let p = read_presentation(&db, &id, None).expect("read");
        assert!(!p.linear);
        assert!(p.chapters.is_empty());
        let err = plan_announcements(&db, &id, "v", false).expect_err("not linear");
        assert_eq!(
            serde_json::to_value(&err).unwrap()["details"]["cause"],
            "not_linear"
        );
    }
}
