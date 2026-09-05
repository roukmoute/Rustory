//! IPC boundary of a story's PRESENTATION on the device (layout + spoken
//! announcements) and of the announcement VOICES (system voices, the
//! downloadable embedded voice, the user's choice). Rust decides everything
//! (what is said, which engine, what exists); the frontend renders and
//! triggers. Every engine call and every download runs on a blocking
//! worker, never on the UI thread and never under the DB lock.

use std::sync::atomic::Ordering;
use std::time::Instant;

use tauri::ipc::Channel;
use tauri::{async_runtime, AppHandle, Manager, State};

use crate::application::settings::{read_setting, write_setting, ANNOUNCEMENT_VOICE_KEY};
use crate::application::story::announcements::{generate_announcements, speech_error};
use crate::application::story::node::store_node_media;
use crate::application::story::presentation::{self, read_presentation, set_layout};
use crate::commands::shared::{base64_decode, base64_encode, validate_story_id};
use crate::domain::shared::AppError;
use crate::infrastructure::filesystem::MediaKind;
use crate::infrastructure::speech::{
    EmbeddedError, EmbeddedVoiceStatus, HttpArtifactFetcher, SpeechError, SpeechSynthesizer,
    EMBEDDED_VOICE_ID,
};
use crate::ipc::dto::{
    AnnouncementVoiceDto, AnnouncementVoicesDto, AttachRecordedAnnouncementInputDto,
    EmbeddedVoiceStatusDto, GenerateAnnouncementsInputDto, GenerateAnnouncementsOutcomeDto,
    PreviewAnnouncementVoiceInputDto, RemoveAnnouncementInputDto, SetAnnouncementVoiceInputDto,
    SetStoryLayoutInputDto, StoryPresentationDto, VoicePreviewDto,
};
use crate::AppState;

/// The sentence a voice preview speaks.
const PREVIEW_TEXT: &str = "Quelle histoire veux-tu écouter ? Épisode 1, Le trésor de Moctezuma.";

/// Hard bound on a microphone recording (base64 decoded): a spoken title is
/// seconds long; 20 MB of WAV is several minutes at CD quality.
const MAX_RECORDING_BYTES: usize = 20 * 1024 * 1024;

/// Read the presentation of a story (layout, announcements and their state
/// against the voice announcements would be generated with now).
#[tauri::command]
pub async fn read_story_presentation(
    app: AppHandle,
    state: State<'_, AppState>,
    story_id: String,
) -> Result<StoryPresentationDto, AppError> {
    validate_story_id(&story_id)?;
    let _ = app;
    let db = state.db.clone();
    let speech = state.speech.clone();
    async_runtime::spawn_blocking(move || {
        // The voice list is an engine call: OFF the lock, before it.
        let selected = selected_voice_id(&db, speech.as_ref())?;
        let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        presentation_dto(&guard, &story_id, selected.as_deref())
    })
    .await
    .map_err(|_| join_error())?
}

/// Change the layout of a story (sequential ↔ menu). The announcements are
/// untouched: switching back and forth never loses a generated clip.
#[tauri::command]
pub async fn set_story_layout(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SetStoryLayoutInputDto,
) -> Result<StoryPresentationDto, AppError> {
    validate_story_id(&input.story_id)?;
    let _ = app;
    let db = state.db.clone();
    let speech = state.speech.clone();
    async_runtime::spawn_blocking(move || {
        let selected = selected_voice_id(&db, speech.as_ref())?;
        let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        set_layout(&guard, &input.story_id, input.layout.to_domain())?;
        presentation_dto(&guard, &input.story_id, selected.as_deref())
    })
    .await
    .map_err(|_| join_error())?
}

/// Generate the missing / stale announcements of a story with the selected
/// voice (all of them with `force`). Progress: integer percent of the clips.
#[tauri::command]
pub async fn generate_story_announcements(
    app: AppHandle,
    state: State<'_, AppState>,
    input: GenerateAnnouncementsInputDto,
    on_progress: Channel<u8>,
) -> Result<GenerateAnnouncementsOutcomeDto, AppError> {
    validate_story_id(&input.story_id)?;
    let app_data_dir = app_data_dir(&app)?;
    let db = state.db.clone();
    let speech = state.speech.clone();
    let started = Instant::now();
    let outcome = async_runtime::spawn_blocking(move || {
        let voice_id = selected_voice_id(&db, speech.as_ref())?
            .ok_or_else(|| speech_error(SpeechError::NoEngine))?;
        let last = std::cell::Cell::new(-1i16);
        let forward = |pct: u8| {
            if i16::from(pct) != last.get() {
                last.set(i16::from(pct));
                let _ = on_progress.send(pct);
            }
        };
        let report = generate_announcements(
            &db,
            &app_data_dir,
            &input.story_id,
            &voice_id,
            input.force,
            speech.as_ref(),
            &forward,
        )?;
        let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let presentation = presentation_dto(&guard, &input.story_id, Some(&voice_id))?;
        Ok::<_, AppError>(GenerateAnnouncementsOutcomeDto {
            generated: report.generated as u32,
            planned: report.planned as u32,
            voice_id,
            presentation,
        })
    })
    .await
    .map_err(|_| join_error())?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match &outcome {
        Ok(out) => crate::infrastructure::diagnostics::device_log::Event::AnnouncementsGenerated {
            generated: out.generated,
            planned: out.planned,
            engine: engine_tag(&out.voice_id),
            elapsed_ms,
        },
        Err(err) => {
            crate::infrastructure::diagnostics::device_log::Event::AnnouncementsGenerationFailed {
                source: failure_source(err),
                elapsed_ms,
            }
        }
    };
    let _ = crate::infrastructure::diagnostics::device_log::record_event(&app, event);
    outcome
}

/// Attach a MICROPHONE recording as the announcement of one target: the
/// bytes (WAV, base64) are validated and promoted OFF the lock, then
/// committed under it in place of the previous clip. Never regenerated by
/// a voice afterwards.
#[tauri::command]
pub async fn attach_recorded_announcement(
    app: AppHandle,
    state: State<'_, AppState>,
    input: AttachRecordedAnnouncementInputDto,
) -> Result<StoryPresentationDto, AppError> {
    validate_story_id(&input.story_id)?;
    let app_data_dir = app_data_dir(&app)?;
    let db = state.db.clone();
    let speech = state.speech.clone();
    async_runtime::spawn_blocking(move || {
        let bytes = base64_decode(&input.audio_base64).ok_or_else(recording_invalid_error)?;
        if bytes.is_empty() || bytes.len() > MAX_RECORDING_BYTES {
            return Err(recording_invalid_error());
        }
        // Sniffed + promoted like any node audio (a non-audio payload is
        // refused here, before any row).
        let prepared = store_node_media(&app_data_dir, MediaKind::Audio, &bytes)?;
        let target = input.target.to_domain();
        let selected = selected_voice_id(&db, speech.as_ref())?;
        let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        presentation::attach_recorded_announcement(&mut guard, &input.story_id, &target, prepared)?;
        presentation_dto(&guard, &input.story_id, selected.as_deref())
    })
    .await
    .map_err(|_| join_error())?
}

/// Remove the stored clip of one announcement (recorded or generated): it
/// goes back to missing.
#[tauri::command]
pub async fn remove_story_announcement(
    app: AppHandle,
    state: State<'_, AppState>,
    input: RemoveAnnouncementInputDto,
) -> Result<StoryPresentationDto, AppError> {
    validate_story_id(&input.story_id)?;
    let app_data_dir = app_data_dir(&app)?;
    let db = state.db.clone();
    let speech = state.speech.clone();
    async_runtime::spawn_blocking(move || {
        let target = input.target.to_domain();
        let selected = selected_voice_id(&db, speech.as_ref())?;
        let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        presentation::remove_announcement(&mut guard, &app_data_dir, &input.story_id, &target)?;
        presentation_dto(&guard, &input.story_id, selected.as_deref())
    })
    .await
    .map_err(|_| join_error())?
}

/// The announcement voices available now, the selection, and the embedded
/// voice's install status.
#[tauri::command]
pub async fn read_announcement_voices(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AnnouncementVoicesDto, AppError> {
    let _ = app;
    let db = state.db.clone();
    let speech = state.speech.clone();
    let installing = state.embedded_voice_installing.clone();
    async_runtime::spawn_blocking(move || {
        voices_dto(&db, speech.as_ref(), installing.load(Ordering::SeqCst))
    })
    .await
    .map_err(|_| join_error())?
}

/// Store the announcement voice. The id must be one of the available voices.
#[tauri::command]
pub async fn set_announcement_voice(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SetAnnouncementVoiceInputDto,
) -> Result<AnnouncementVoicesDto, AppError> {
    let _ = app;
    let db = state.db.clone();
    let speech = state.speech.clone();
    let installing = state.embedded_voice_installing.clone();
    async_runtime::spawn_blocking(move || {
        let available = speech.french_voices();
        if !available.iter().any(|v| v.id == input.voice_id) {
            return Err(speech_error(SpeechError::VoiceUnavailable));
        }
        {
            let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            write_setting(&guard, ANNOUNCEMENT_VOICE_KEY, Some(&input.voice_id))?;
        }
        voices_dto(&db, speech.as_ref(), installing.load(Ordering::SeqCst))
    })
    .await
    .map_err(|_| join_error())?
}

/// Speak a fixed sample with a voice, returned as a playable data URL.
#[tauri::command]
pub async fn preview_announcement_voice(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PreviewAnnouncementVoiceInputDto,
) -> Result<VoicePreviewDto, AppError> {
    let _ = app;
    let speech = state.speech.clone();
    async_runtime::spawn_blocking(move || {
        let wav = speech
            .synthesize(&input.voice_id, PREVIEW_TEXT)
            .map_err(speech_error)?;
        let duration_ms = crate::infrastructure::speech::wav::duration_ms(&wav).unwrap_or(0);
        Ok::<_, AppError>(VoicePreviewDto {
            data_url: format!("data:audio/wav;base64,{}", base64_encode(&wav)),
            duration_ms,
            spoken_text: PREVIEW_TEXT.to_string(),
        })
    })
    .await
    .map_err(|_| join_error())?
}

/// Download and install the embedded neural voice (an explicit gesture;
/// ~90 MB, verified against pinned checksums), then select it as the
/// announcement voice. Progress: integer percent of the bytes.
#[tauri::command]
pub async fn install_embedded_voice(
    app: AppHandle,
    state: State<'_, AppState>,
    on_progress: Channel<u8>,
) -> Result<AnnouncementVoicesDto, AppError> {
    let db = state.db.clone();
    let speech = state.speech.clone();
    let installing = state.embedded_voice_installing.clone();
    if installing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(install_busy_error());
    }
    let started = Instant::now();
    let outcome = async_runtime::spawn_blocking(move || {
        let result = (|| {
            let embedded = speech.embedded();
            if matches!(embedded.status(), EmbeddedVoiceStatus::Unsupported) {
                return Err(embedded_error(EmbeddedError::Unsupported));
            }
            let last = std::cell::Cell::new(-1i16);
            embedded
                .install(&HttpArtifactFetcher::default(), &|p| {
                    let pct = if p.bytes_total == 0 {
                        0
                    } else {
                        ((p.bytes_done as f64 / p.bytes_total as f64) * 100.0).round() as u8
                    }
                    .min(100);
                    if i16::from(pct) != last.get() {
                        last.set(i16::from(pct));
                        let _ = on_progress.send(pct);
                    }
                })
                .map_err(embedded_error)?;
            // The freshly installed voice becomes the announcement voice —
            // the gesture's intent — unless the user picks another later.
            {
                let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                write_setting(&guard, ANNOUNCEMENT_VOICE_KEY, Some(EMBEDDED_VOICE_ID))?;
            }
            Ok::<(), AppError>(())
        })();
        installing.store(false, Ordering::SeqCst);
        result?;
        voices_dto(&db, speech.as_ref(), false)
    })
    .await
    .map_err(|_| {
        state
            .embedded_voice_installing
            .store(false, Ordering::SeqCst);
        join_error()
    })?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match &outcome {
        Ok(_) => crate::infrastructure::diagnostics::device_log::Event::EmbeddedVoiceInstalled {
            elapsed_ms,
        },
        Err(err) => {
            crate::infrastructure::diagnostics::device_log::Event::EmbeddedVoiceInstallFailed {
                source: failure_source(err),
                elapsed_ms,
            }
        }
    };
    let _ = crate::infrastructure::diagnostics::device_log::record_event(&app, event);
    outcome
}

// ===== helpers =====

fn presentation_dto(
    db: &crate::infrastructure::db::DbHandle,
    story_id: &str,
    selected_voice_id: Option<&str>,
) -> Result<StoryPresentationDto, AppError> {
    let presentation = read_presentation(db, story_id, selected_voice_id)?;
    let archive_retained: bool = db
        .conn()
        .query_row(
            "SELECT COALESCE(source_archive_retained, 0) FROM story_local_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| r.get::<_, bool>(0),
        )
        .unwrap_or(false);
    Ok(StoryPresentationDto::from_domain(
        &presentation,
        archive_retained,
    ))
}

/// The voice announcements are generated with: the stored choice when it is
/// still available, else the first available French voice. `None` when no
/// voice exists. Engine listing OFF the lock; the setting read under it.
fn selected_voice_id(
    db: &std::sync::Mutex<crate::infrastructure::db::DbHandle>,
    speech: &dyn SpeechSynthesizerExt,
) -> Result<Option<String>, AppError> {
    let available = speech.french_voice_ids();
    let stored = {
        let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        read_setting(&guard, ANNOUNCEMENT_VOICE_KEY)?
    };
    Ok(stored
        .filter(|id| available.iter().any(|v| v == id))
        .or_else(|| available.first().cloned()))
}

fn voices_dto(
    db: &std::sync::Mutex<crate::infrastructure::db::DbHandle>,
    speech: &crate::infrastructure::speech::CompositeSpeech,
    installing: bool,
) -> Result<AnnouncementVoicesDto, AppError> {
    let voices = speech.french_voices();
    let stored = {
        let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        read_setting(&guard, ANNOUNCEMENT_VOICE_KEY)?
    };
    let stored_available = stored
        .as_deref()
        .filter(|id| voices.iter().any(|v| v.id == *id))
        .map(str::to_string);
    let selected_is_stored = stored_available.is_some();
    let selected_voice_id = stored_available.or_else(|| voices.first().map(|v| v.id.clone()));
    let embedded = speech.embedded();
    let download_bytes = embedded.manifest().map(|m| m.total_size()).unwrap_or(0);
    Ok(AnnouncementVoicesDto {
        voices: voices
            .iter()
            .map(AnnouncementVoiceDto::from_domain)
            .collect(),
        selected_voice_id,
        selected_is_stored,
        embedded: EmbeddedVoiceStatusDto::from_domain(
            &embedded.status(),
            installing,
            download_bytes,
        ),
    })
}

/// The one engine capability the selection needs, so `selected_voice_id`
/// stays testable with any engine.
trait SpeechSynthesizerExt {
    fn french_voice_ids(&self) -> Vec<String>;
}

impl SpeechSynthesizerExt for crate::infrastructure::speech::CompositeSpeech {
    fn french_voice_ids(&self) -> Vec<String> {
        self.french_voices().into_iter().map(|v| v.id).collect()
    }
}

fn engine_tag(voice_id: &str) -> &'static str {
    if voice_id.starts_with("embedded:") {
        "embedded"
    } else {
        "system"
    }
}

/// Map an AppError to the closed diagnostic `source` set.
fn failure_source(err: &AppError) -> &'static str {
    err.details
        .as_ref()
        .and_then(|d| d.get("source").and_then(|s| s.as_str()))
        .map(|s| match s {
            "speech" => "speech",
            "presentation" => "presentation",
            "embedded_voice" => "embedded_voice",
            "settings" => "settings",
            _ => "other",
        })
        .unwrap_or("other")
}

fn embedded_error(err: EmbeddedError) -> AppError {
    let (message, action) = match err {
        EmbeddedError::Unsupported => (
            "La voix neuronale n'est pas disponible pour cet ordinateur.",
            "Utilise une voix installée sur le système.",
        ),
        EmbeddedError::Download(_) => (
            "Le téléchargement de la voix neuronale a échoué.",
            "Vérifie ta connexion internet puis réessaie.",
        ),
        EmbeddedError::Checksum(_) => (
            "Le fichier téléchargé ne correspond pas à la voix attendue : rien n'a été installé.",
            "Réessaie plus tard ; si le problème persiste, signale-le.",
        ),
        EmbeddedError::Install(_) => (
            "La voix neuronale n'a pas pu être installée.",
            "Vérifie l'espace disponible et les permissions de ton dossier utilisateur, puis réessaie.",
        ),
    };
    AppError::media_processing_failed(message, action).with_details(serde_json::json!({
        "source": "embedded_voice",
        "cause": err.diagnostic_tag(),
        "stage": err.stage(),
    }))
}

fn recording_invalid_error() -> AppError {
    AppError::media_invalid(
        "L'enregistrement n'a pas pu être lu.",
        "Réessaie l'enregistrement ; s'il échoue encore, vérifie le micro dans les réglages du système.",
    )
    .with_details(serde_json::json!({ "source": "recording", "cause": "invalid_payload" }))
}

fn install_busy_error() -> AppError {
    AppError::media_processing_failed(
        "Un téléchargement de la voix neuronale est déjà en cours.",
        "Patiente jusqu'à la fin du téléchargement.",
    )
    .with_details(serde_json::json!({ "source": "embedded_voice", "cause": "busy" }))
}

fn app_data_dir(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    app.path().app_data_dir().map_err(|_| {
        AppError::local_storage_unavailable(
            "Rustory n'a pas pu localiser son dossier de données.",
            "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
        )
        .with_details(serde_json::json!({ "source": "app_data_unavailable" }))
    })
}

fn join_error() -> AppError {
    AppError::local_storage_unavailable(
        "Tâche interrompue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({ "source": "spawn_blocking_join" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_carry_a_cause_and_a_next_gesture() {
        for err in [
            embedded_error(EmbeddedError::Unsupported),
            embedded_error(EmbeddedError::Download("request")),
            embedded_error(EmbeddedError::Checksum("model")),
            embedded_error(EmbeddedError::Install("promote_runtime")),
            install_busy_error(),
            recording_invalid_error(),
            speech_error(SpeechError::NoEngine),
            speech_error(SpeechError::Timeout),
        ] {
            assert!(!err.message.is_empty());
            assert!(
                !err.user_action.as_deref().unwrap_or("").is_empty(),
                "{err:?}"
            );
            let v = serde_json::to_value(&err).unwrap();
            assert!(v["details"]["source"].is_string());
        }
        assert_eq!(
            failure_source(&speech_error(SpeechError::NoEngine)),
            "speech"
        );
        assert_eq!(
            failure_source(&embedded_error(EmbeddedError::Unsupported)),
            "embedded_voice"
        );
        assert_eq!(engine_tag("embedded:x"), "embedded");
        assert_eq!(engine_tag("system:say:Thomas"), "system");
    }
}
