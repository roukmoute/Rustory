//! Speech synthesis — the voices that read a story's announcements (series
//! title, menu question, episode titles) so a child can pick an episode on
//! the Lunii wheel. Two engines, one contract:
//!
//! - the SYSTEM voices already installed on this computer, found per OS at
//!   runtime: Windows through its speech runtime (WinRT), macOS through the
//!   `say` command, Linux through `pico2wave` or `espeak-ng` when present.
//!   Nothing to download; quality follows the OS.
//! - the EMBEDDED neural voice: the Piper runtime plus a French voice model,
//!   DOWNLOADED on explicit request into the app data dir and run as a
//!   separate process (never linked — Piper's phonemizer is GPL). Same
//!   quality everywhere.
//!
//! Every engine produces a WAV; the caller stores it as an ordinary story
//! audio asset (the device path transcodes it to the Lunii MP3 format at
//! send time). Voice ids are stable, prefixed by their engine
//! (`system:…` / `embedded:…`), so a setting survives restarts and a
//! missing engine is reported, never guessed.

pub mod embedded;
pub mod process;
pub mod system;
pub mod wav;

#[cfg(target_os = "windows")]
pub mod system_windows;

use std::path::Path;

pub use embedded::{
    ArtifactFetcher, EmbeddedError, EmbeddedInstallProgress, EmbeddedManifest, EmbeddedVoice,
    EmbeddedVoiceStatus, HttpArtifactFetcher, EMBEDDED_VOICE_ID, EMBEDDED_VOICE_NAME,
};
pub use system::{system_synthesizer, SystemSpeech};

/// The language every announcement is spoken in (BCP-47 primary tag).
pub const ANNOUNCEMENT_LANGUAGE: &str = "fr";

/// Which engine a voice belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceEngine {
    System,
    Embedded,
}

impl VoiceEngine {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Embedded => "embedded",
        }
    }
}

/// A voice the user can pick. `id` is stable across launches and prefixed
/// by its engine; `language` is a BCP-47 tag as the engine reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub language: String,
    pub engine: VoiceEngine,
}

/// Why a synthesis could not happen. Closed, PII-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechError {
    /// No engine is available on this computer (no system voice, no
    /// embedded voice installed).
    NoEngine,
    /// The requested voice id belongs to no available voice.
    VoiceUnavailable,
    /// The engine ran but failed (non-zero exit, missing output).
    EngineFailed(&'static str),
    /// The engine did not answer within its budget.
    Timeout,
    /// The engine produced something that is not a WAV.
    InvalidOutput,
}

impl SpeechError {
    pub const fn diagnostic_tag(&self) -> &'static str {
        match self {
            Self::NoEngine => "no_engine",
            Self::VoiceUnavailable => "voice_unavailable",
            Self::EngineFailed(_) => "engine_failed",
            Self::Timeout => "timeout",
            Self::InvalidOutput => "invalid_output",
        }
    }
}

/// A speech engine: lists its voices and speaks a text with one of them.
pub trait SpeechSynthesizer: Send + Sync {
    /// The voices this engine offers, in a stable order.
    fn list_voices(&self) -> Vec<Voice>;
    /// Speak `text` with the voice `voice_id`, as WAV bytes.
    fn synthesize(&self, voice_id: &str, text: &str) -> Result<Vec<u8>, SpeechError>;
}

/// The two engines behind one contract: the system voices first, then the
/// embedded voice when it is installed. Routes a synthesis by the id prefix.
pub struct CompositeSpeech {
    system: Box<dyn SpeechSynthesizer>,
    embedded: EmbeddedVoice,
}

impl CompositeSpeech {
    /// The production composition for the app data dir `app_data_dir`.
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            system: system_synthesizer(),
            embedded: EmbeddedVoice::new(app_data_dir),
        }
    }

    /// A composition over an explicit system engine (tests).
    pub fn with_system(system: Box<dyn SpeechSynthesizer>, app_data_dir: &Path) -> Self {
        Self {
            system,
            embedded: EmbeddedVoice::new(app_data_dir),
        }
    }

    pub fn embedded(&self) -> &EmbeddedVoice {
        &self.embedded
    }

    /// Only the French voices (the announcement language), system first.
    pub fn french_voices(&self) -> Vec<Voice> {
        self.list_voices()
            .into_iter()
            .filter(|v| is_announcement_language(&v.language))
            .collect()
    }
}

impl SpeechSynthesizer for CompositeSpeech {
    fn list_voices(&self) -> Vec<Voice> {
        let mut voices = self.system.list_voices();
        voices.extend(self.embedded.list_voices());
        voices
    }

    fn synthesize(&self, voice_id: &str, text: &str) -> Result<Vec<u8>, SpeechError> {
        if voice_id.starts_with("embedded:") {
            self.embedded.synthesize(voice_id, text)
        } else if voice_id.starts_with("system:") {
            self.system.synthesize(voice_id, text)
        } else {
            Err(SpeechError::VoiceUnavailable)
        }
    }
}

/// True for a French locale tag (`fr`, `fr-FR`, `fr_CA`, `fr-fr`…).
pub fn is_announcement_language(tag: &str) -> bool {
    let primary = tag
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    primary == ANNOUNCEMENT_LANGUAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcement_language_matches_any_french_locale_tag() {
        for tag in ["fr", "fr-FR", "fr_FR", "fr-CA", "FR-BE", "fr_fr"] {
            assert!(is_announcement_language(tag), "{tag}");
        }
        for tag in ["en-US", "de", "", "frx", "fre"] {
            assert!(!is_announcement_language(tag), "{tag}");
        }
    }

    #[test]
    fn the_composite_routes_by_engine_prefix_and_refuses_unknown_ids() {
        struct Echo;
        impl SpeechSynthesizer for Echo {
            fn list_voices(&self) -> Vec<Voice> {
                vec![Voice {
                    id: "system:x".into(),
                    name: "X".into(),
                    language: "fr-FR".into(),
                    engine: VoiceEngine::System,
                }]
            }
            fn synthesize(&self, voice_id: &str, text: &str) -> Result<Vec<u8>, SpeechError> {
                Ok(format!("{voice_id}|{text}").into_bytes())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let composite = CompositeSpeech::with_system(Box::new(Echo), dir.path());
        assert_eq!(
            composite.french_voices().len(),
            1,
            "no embedded voice installed"
        );
        assert_eq!(
            composite.synthesize("system:x", "Bonjour").unwrap(),
            b"system:x|Bonjour".to_vec()
        );
        assert_eq!(
            composite.synthesize("embedded:nope", "x"),
            Err(SpeechError::VoiceUnavailable)
        );
        assert_eq!(
            composite.synthesize("x", "x"),
            Err(SpeechError::VoiceUnavailable)
        );
    }
}
