//! The Windows SYSTEM voices, through the Windows speech runtime (WinRT
//! `Windows.Media.SpeechSynthesis`): every voice installed on the machine
//! (the classic ones and the natural voices added through the system
//! settings), listed live, spoken to an in-memory WAV stream — no file, no
//! external program.
//!
//! WinRT needs an initialized apartment on the calling thread; the calls run
//! on a blocking worker, so the thread is initialized here (multithreaded)
//! and an "already initialized in another mode" answer is fine.

use windows::core::HSTRING;
use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceInformation};
use windows::Storage::Streams::DataReader;

use super::{SpeechError, SpeechSynthesizer as SpeechEngine, Voice, VoiceEngine};

const ID_PREFIX: &str = "system:winrt:";

/// The Windows speech runtime.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsSpeech;

fn init_apartment() {
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    // A thread already initialized (either mode) answers with an error that
    // changes nothing for the calls below.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
}

fn all_voices() -> windows::core::Result<Vec<VoiceInformation>> {
    let voices = SpeechSynthesizer::AllVoices()?;
    let mut out = Vec::new();
    for index in 0..voices.Size()? {
        out.push(voices.GetAt(index)?);
    }
    Ok(out)
}

fn voice_id(voice: &VoiceInformation) -> windows::core::Result<String> {
    Ok(format!("{ID_PREFIX}{}", voice.Id()?))
}

impl SpeechEngine for WindowsSpeech {
    fn list_voices(&self) -> Vec<Voice> {
        init_apartment();
        let Ok(voices) = all_voices() else {
            return Vec::new();
        };
        voices
            .iter()
            .filter_map(|voice| {
                Some(Voice {
                    id: voice_id(voice).ok()?,
                    name: voice.DisplayName().ok()?.to_string(),
                    language: voice.Language().ok()?.to_string(),
                    engine: VoiceEngine::System,
                })
            })
            .collect()
    }

    fn synthesize(&self, voice_id_wanted: &str, text: &str) -> Result<Vec<u8>, SpeechError> {
        init_apartment();
        let wanted = voice_id_wanted
            .strip_prefix(ID_PREFIX)
            .filter(|id| !id.is_empty())
            .ok_or(SpeechError::VoiceUnavailable)?;
        let voices = all_voices().map_err(|_| SpeechError::EngineFailed("winrt_voices"))?;
        let voice = voices
            .into_iter()
            .find(|v| v.Id().map(|id| id.to_string() == wanted).unwrap_or(false))
            .ok_or(SpeechError::VoiceUnavailable)?;
        let synthesizer =
            SpeechSynthesizer::new().map_err(|_| SpeechError::EngineFailed("winrt_new"))?;
        synthesizer
            .SetVoice(&voice)
            .map_err(|_| SpeechError::EngineFailed("winrt_voice"))?;
        let stream = synthesizer
            .SynthesizeTextToStreamAsync(&HSTRING::from(text))
            .map_err(|_| SpeechError::EngineFailed("winrt_synthesize"))?
            .get()
            .map_err(|_| SpeechError::EngineFailed("winrt_synthesize"))?;
        let size = stream
            .Size()
            .map_err(|_| SpeechError::EngineFailed("winrt_stream"))?;
        let input = stream
            .GetInputStreamAt(0)
            .map_err(|_| SpeechError::EngineFailed("winrt_stream"))?;
        let reader = DataReader::CreateDataReader(&input)
            .map_err(|_| SpeechError::EngineFailed("winrt_reader"))?;
        let size = u32::try_from(size).map_err(|_| SpeechError::InvalidOutput)?;
        reader
            .LoadAsync(size)
            .map_err(|_| SpeechError::EngineFailed("winrt_reader"))?
            .get()
            .map_err(|_| SpeechError::EngineFailed("winrt_reader"))?;
        let mut bytes = vec![0u8; size as usize];
        reader
            .ReadBytes(&mut bytes)
            .map_err(|_| SpeechError::EngineFailed("winrt_reader"))?;
        if !super::wav::is_wav(&bytes) {
            return Err(SpeechError::InvalidOutput);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_outside_the_runtime_prefix_is_refused_without_touching_winrt() {
        assert_eq!(
            WindowsSpeech.synthesize("system:say:Thomas", "x"),
            Err(SpeechError::VoiceUnavailable)
        );
        assert_eq!(
            WindowsSpeech.synthesize("system:winrt:", "x"),
            Err(SpeechError::VoiceUnavailable)
        );
    }

    #[test]
    fn listing_the_voices_never_fails_on_a_windows_runner() {
        // The runner has voices or not; either way the call is a plain list.
        let voices = WindowsSpeech.list_voices();
        for voice in &voices {
            assert!(voice.id.starts_with(ID_PREFIX));
            assert!(!voice.name.is_empty());
        }
    }
}
