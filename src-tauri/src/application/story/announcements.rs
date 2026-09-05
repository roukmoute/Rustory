//! Generate a story's spoken announcements with a voice: the orchestration
//! between the presentation read-model (what to say, what is already said),
//! the speech engine (a process or a runtime call — never under the DB
//! lock) and the media store (the clip becomes an ordinary story audio
//! asset). Clip by clip, so a failure mid-way keeps every clip already
//! committed: the next run only plans what is still missing.

use std::path::Path;
use std::sync::Mutex;

use crate::domain::shared::AppError;
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::MediaKind;
use crate::infrastructure::speech::{SpeechError, SpeechSynthesizer};

use super::node::store_node_media;
use super::presentation::{commit_announcement, plan_announcements, sweep_orphan_prompts};

/// What a generation run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationReport {
    /// Clips synthesized and committed by this run.
    pub generated: usize,
    /// Clips the plan wanted (0 = everything was already ready).
    pub planned: usize,
}

/// Generate (or refresh) every announcement of `story_id` with `voice_id`.
/// `force` regenerates the ready ones too. Progress is an integer percent
/// of the planned clips.
pub fn generate_announcements(
    db: &Mutex<DbHandle>,
    app_data_dir: &Path,
    story_id: &str,
    voice_id: &str,
    force: bool,
    speech: &dyn SpeechSynthesizer,
    on_progress: &dyn Fn(u8),
) -> Result<GenerationReport, AppError> {
    // Plan UNDER the lock, without any I/O.
    let clips = {
        let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        sweep_orphan_prompts(&mut guard, app_data_dir, story_id);
        plan_announcements(&guard, story_id, voice_id, force)?
    };
    let planned = clips.len();
    let mut generated = 0usize;
    on_progress(0);
    for (index, clip) in clips.iter().enumerate() {
        // Speak and promote OFF the lock (an engine may take seconds).
        let wav = speech
            .synthesize(voice_id, &clip.spoken_text)
            .map_err(speech_error)?;
        let prepared = store_node_media(app_data_dir, MediaKind::Audio, &wav)?;
        // Commit UNDER the lock.
        {
            let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            commit_announcement(
                &mut guard,
                story_id,
                &clip.target,
                prepared,
                &clip.spoken_text,
                voice_id,
            )?;
        }
        generated += 1;
        let pct = ((index + 1) as f64 / planned.max(1) as f64 * 100.0).round() as u8;
        on_progress(pct.min(100));
    }
    if planned == 0 {
        on_progress(100);
    }
    Ok(GenerationReport { generated, planned })
}

/// A speech failure as the user sees it: the voice is missing, or the
/// engine failed — never a half-generated menu.
pub fn speech_error(err: SpeechError) -> AppError {
    let (message, action) = match err {
        SpeechError::NoEngine => (
            "Aucune voix n'est disponible sur cet ordinateur.",
            "Installe une voix française dans les réglages du système, ou télécharge la voix neuronale depuis les réglages de Rustory.",
        ),
        SpeechError::VoiceUnavailable => (
            "La voix choisie n'est plus disponible.",
            "Choisis une autre voix dans les réglages de Rustory, puis réessaie.",
        ),
        SpeechError::EngineFailed(_) | SpeechError::InvalidOutput => (
            "La voix n'a pas pu produire l'annonce.",
            "Réessaie ; si le problème persiste, choisis une autre voix dans les réglages.",
        ),
        SpeechError::Timeout => (
            "La voix a mis trop de temps à répondre.",
            "Réessaie ; si le problème persiste, choisis une autre voix dans les réglages.",
        ),
    };
    AppError::media_processing_failed(message, action).with_details(serde_json::json!({
        "source": "speech",
        "cause": err.diagnostic_tag(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::presentation::{read_presentation, AnnouncementStatus};
    use crate::application::story::{create_story, CreateStoryInput};
    use crate::domain::device::StoryLayout;
    use crate::infrastructure::db::{open_in_memory, run_migrations};
    use crate::infrastructure::device::audio_transcode::test_support::wav_sine;
    use crate::infrastructure::speech::{Voice, VoiceEngine};

    /// Speaks every text as a short WAV, remembering what it was asked.
    struct FakeVoice {
        spoken: Mutex<Vec<String>>,
        fail_on: Option<&'static str>,
    }

    impl SpeechSynthesizer for FakeVoice {
        fn list_voices(&self) -> Vec<Voice> {
            vec![Voice {
                id: "system:fake".into(),
                name: "Fake".into(),
                language: "fr-FR".into(),
                engine: VoiceEngine::System,
            }]
        }
        fn synthesize(&self, voice_id: &str, text: &str) -> Result<Vec<u8>, SpeechError> {
            if voice_id != "system:fake" {
                return Err(SpeechError::VoiceUnavailable);
            }
            if self.fail_on.is_some_and(|needle| text.contains(needle)) {
                return Err(SpeechError::EngineFailed("fake"));
            }
            self.spoken.lock().unwrap().push(text.to_string());
            Ok(wav_sine(8_000, 1, 300.0 + text.len() as f64, 0.05, 0.3))
        }
    }

    fn setup(labels: &[(&str, &str)]) -> (Mutex<DbHandle>, tempfile::TempDir, String) {
        let mut db = open_in_memory().unwrap();
        run_migrations(&mut db).unwrap();
        let id = create_story(
            &mut db,
            CreateStoryInput {
                title: "Série".into(),
            },
        )
        .unwrap()
        .id;
        let nodes: Vec<serde_json::Value> = labels
            .iter()
            .map(|(id, label)| {
                serde_json::json!({
                    "id": id, "text": "", "label": label,
                    "imageAssetId": null, "audioAssetId": format!("aud-{id}"), "options": [],
                })
            })
            .collect();
        let structure: crate::domain::story::CanonicalStructure = serde_json::from_value(
            serde_json::json!({ "schemaVersion": 3, "startNodeId": "n1", "nodes": nodes }),
        )
        .unwrap();
        let json = crate::domain::story::canonical_structure_json(&structure);
        let checksum = crate::domain::story::content_checksum(&json);
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = ?1, content_checksum = ?2 WHERE id = ?3",
                rusqlite::params![json, checksum, &id],
            )
            .unwrap();
        (Mutex::new(db), tempfile::tempdir().unwrap(), id)
    }

    #[test]
    fn generates_every_announcement_then_nothing_more() {
        let (db, app_data, id) = setup(&[("n1", "Un : épisode 1/2"), ("n2", "Deux")]);
        let voice = FakeVoice {
            spoken: Mutex::new(Vec::new()),
            fail_on: None,
        };
        let progress = Mutex::new(Vec::new());
        let report = generate_announcements(
            &db,
            app_data.path(),
            &id,
            "system:fake",
            false,
            &voice,
            &|p| progress.lock().unwrap().push(p),
        )
        .expect("generate");
        assert_eq!(
            report,
            GenerationReport {
                generated: 4,
                planned: 4
            }
        );
        let spoken = voice.spoken.lock().unwrap().clone();
        assert_eq!(
            spoken,
            vec![
                "Série.".to_string(),
                crate::domain::speech::MENU_QUESTION.to_string(),
                "Épisode 1. Un.".to_string(),
                "Deux.".to_string(),
            ]
        );
        assert_eq!(*progress.lock().unwrap().last().unwrap(), 100);
        let guard = db.lock().unwrap();
        let p = read_presentation(&guard, &id, Some("system:fake")).unwrap();
        assert_eq!(
            p.layout,
            StoryLayout::Sequential,
            "generation never changes the layout"
        );
        assert!(p
            .chapters
            .iter()
            .all(|c| c.announcement.status == AnnouncementStatus::Ready));
        drop(guard);
        let report = generate_announcements(
            &db,
            app_data.path(),
            &id,
            "system:fake",
            false,
            &voice,
            &|_| {},
        )
        .unwrap();
        assert_eq!(
            report,
            GenerationReport {
                generated: 0,
                planned: 0
            }
        );
    }

    #[test]
    fn a_failing_clip_keeps_the_ones_already_committed() {
        let (db, app_data, id) = setup(&[("n1", "Un"), ("n2", "Boum")]);
        let voice = FakeVoice {
            spoken: Mutex::new(Vec::new()),
            fail_on: Some("Boum"),
        };
        let err = generate_announcements(
            &db,
            app_data.path(),
            &id,
            "system:fake",
            false,
            &voice,
            &|_| {},
        )
        .expect_err("the failing clip refuses");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "MEDIA_PROCESSING_FAILED");
        assert_eq!(v["details"]["source"], "speech");
        let guard = db.lock().unwrap();
        let p = read_presentation(&guard, &id, Some("system:fake")).unwrap();
        assert_eq!(p.title.status, AnnouncementStatus::Ready);
        assert_eq!(p.question.status, AnnouncementStatus::Ready);
        assert_eq!(p.chapters[0].announcement.status, AnnouncementStatus::Ready);
        assert_eq!(
            p.chapters[1].announcement.status,
            AnnouncementStatus::Missing
        );
    }

    #[test]
    fn an_unknown_voice_is_refused_with_an_actionable_message() {
        let (db, app_data, id) = setup(&[("n1", "Un")]);
        let voice = FakeVoice {
            spoken: Mutex::new(Vec::new()),
            fail_on: None,
        };
        let err = generate_announcements(
            &db,
            app_data.path(),
            &id,
            "system:gone",
            false,
            &voice,
            &|_| {},
        )
        .expect_err("unknown voice");
        assert_eq!(
            serde_json::to_value(&err).unwrap()["details"]["cause"],
            "voice_unavailable"
        );
        assert!(!err.user_action.as_deref().unwrap_or("").is_empty());
    }
}
