//! The SYSTEM voices — what this computer already speaks with, found at
//! runtime and never assumed from the OS name alone:
//!
//! - **macOS** — the `say` command (always present): voices from `say -v ?`,
//!   synthesis to a WAV file.
//! - **Linux** — `pico2wave` (SVOX Pico, the better voice) or `espeak-ng`,
//!   whichever is on the PATH; none → no system voice.
//! - **Windows** — the speech runtime (WinRT `SpeechSynthesizer`), in
//!   [`super::system_windows`].
//!
//! The Unix engines are plain commands, so their listing/parsing code is
//! compiled and tested everywhere (with fake programs on the PATH); only
//! the OS selection is conditional. Voice ids are `system:<engine>:<name>`.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::process::{find_on_path, run_with_budget, take_wav_output, SYNTHESIS_BUDGET};
use super::{is_announcement_language, SpeechError, SpeechSynthesizer, Voice, VoiceEngine};

/// The system engine of this OS.
pub fn system_synthesizer() -> Box<dyn SpeechSynthesizer> {
    #[cfg(target_os = "windows")]
    {
        Box::new(super::system_windows::WindowsSpeech::default())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(SystemSpeech::MacSay(MacSay::default()))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Box::new(SystemSpeech::Linux(LinuxSpeech::default()))
    }
}

/// The command-based system engines (macOS `say`, Linux tools).
pub enum SystemSpeech {
    MacSay(MacSay),
    Linux(LinuxSpeech),
}

impl SpeechSynthesizer for SystemSpeech {
    fn list_voices(&self) -> Vec<Voice> {
        match self {
            Self::MacSay(engine) => engine.list_voices(),
            Self::Linux(engine) => engine.list_voices(),
        }
    }

    fn synthesize(&self, voice_id: &str, text: &str) -> Result<Vec<u8>, SpeechError> {
        match self {
            Self::MacSay(engine) => engine.synthesize(voice_id, text),
            Self::Linux(engine) => engine.synthesize(voice_id, text),
        }
    }
}

/// A scratch WAV path for one synthesis.
fn scratch_wav(
    stage: &'static str,
) -> Result<(tempfile::TempDir, std::path::PathBuf), SpeechError> {
    let dir = tempfile::tempdir().map_err(|_| SpeechError::EngineFailed(stage))?;
    let path = dir.path().join("speech.wav");
    Ok((dir, path))
}

// ===== macOS: `say` =====

/// The macOS `say` engine. `say -v ?` lists voices as
/// `<name>  <locale>  # <sample>`; synthesis writes a 16-bit 22050 Hz WAV.
#[derive(Debug, Clone)]
pub struct MacSay {
    program: String,
    budget: Duration,
}

impl Default for MacSay {
    fn default() -> Self {
        Self {
            program: "say".into(),
            budget: SYNTHESIS_BUDGET,
        }
    }
}

impl MacSay {
    const ID_PREFIX: &'static str = "system:say:";

    /// Parse the `say -v ?` listing into French voices.
    pub fn parse_voice_list(listing: &str) -> Vec<Voice> {
        listing
            .lines()
            .filter_map(|line| {
                // `Amélie (Enhanced)  fr_CA  # Bonjour…` — the locale is the
                // last token before `#`, the name everything before it.
                let head = line.split('#').next()?.trim_end();
                let locale_start = head.rfind(char::is_whitespace)?;
                let locale = head[locale_start..].trim();
                let name = head[..locale_start].trim();
                if name.is_empty() || !is_announcement_language(locale) {
                    return None;
                }
                Some(Voice {
                    id: format!("{}{name}", Self::ID_PREFIX),
                    name: name.to_string(),
                    language: locale.replace('_', "-"),
                    engine: VoiceEngine::System,
                })
            })
            .collect()
    }
}

impl SpeechSynthesizer for MacSay {
    fn list_voices(&self) -> Vec<Voice> {
        let Ok(output) = Command::new(&self.program).args(["-v", "?"]).output() else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        Self::parse_voice_list(&String::from_utf8_lossy(&output.stdout))
    }

    fn synthesize(&self, voice_id: &str, text: &str) -> Result<Vec<u8>, SpeechError> {
        let name = voice_id
            .strip_prefix(Self::ID_PREFIX)
            .filter(|n| !n.is_empty())
            .ok_or(SpeechError::VoiceUnavailable)?;
        let (_guard, out) = scratch_wav("say_scratch")?;
        let mut cmd = Command::new(&self.program);
        cmd.arg("-v")
            .arg(name)
            .arg("-o")
            .arg(&out)
            .args(["--file-format=WAVE", "--data-format=LEI16@22050"])
            .arg(text);
        run_with_budget(cmd, self.budget, "say")?;
        take_wav_output(&out)
    }
}

// ===== Linux: pico2wave / espeak-ng =====

/// The Linux engine: SVOX `pico2wave` when present (one French voice,
/// `fr-FR`), and `espeak-ng` voices (`espeak-ng --voices=fr`).
#[derive(Debug, Clone)]
pub struct LinuxSpeech {
    budget: Duration,
}

impl Default for LinuxSpeech {
    fn default() -> Self {
        Self {
            budget: SYNTHESIS_BUDGET,
        }
    }
}

impl LinuxSpeech {
    const PICO_ID: &'static str = "system:pico2wave:fr-FR";
    const ESPEAK_PREFIX: &'static str = "system:espeak-ng:";

    /// Parse `espeak-ng --voices=fr` (columns: `Pty Language Age/Gender
    /// VoiceName File Other Langs`) into voices.
    pub fn parse_espeak_voices(listing: &str) -> Vec<Voice> {
        listing
            .lines()
            .skip(1)
            .filter_map(|line| {
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() < 4 {
                    return None;
                }
                let language = cols[1];
                let name = cols[3];
                if !is_announcement_language(language) {
                    return None;
                }
                Some(Voice {
                    id: format!("{}{language}", Self::ESPEAK_PREFIX),
                    name: format!("{name} (espeak-ng)"),
                    language: language.to_string(),
                    engine: VoiceEngine::System,
                })
            })
            .collect()
    }
}

impl SpeechSynthesizer for LinuxSpeech {
    fn list_voices(&self) -> Vec<Voice> {
        let mut voices = Vec::new();
        if find_on_path("pico2wave").is_some() {
            voices.push(Voice {
                id: Self::PICO_ID.into(),
                name: "Pico (fr-FR)".into(),
                language: "fr-FR".into(),
                engine: VoiceEngine::System,
            });
        }
        if let Some(espeak) = find_on_path("espeak-ng") {
            if let Ok(output) = Command::new(espeak).arg("--voices=fr").output() {
                if output.status.success() {
                    voices.extend(Self::parse_espeak_voices(&String::from_utf8_lossy(
                        &output.stdout,
                    )));
                }
            }
        }
        voices
    }

    fn synthesize(&self, voice_id: &str, text: &str) -> Result<Vec<u8>, SpeechError> {
        if voice_id == Self::PICO_ID {
            let program = find_on_path("pico2wave").ok_or(SpeechError::VoiceUnavailable)?;
            let (_guard, out) = scratch_wav("pico_scratch")?;
            let mut cmd = Command::new(program);
            cmd.args(["-l", "fr-FR", "-w"]).arg(&out).arg(text);
            run_with_budget(cmd, self.budget, "pico2wave")?;
            return take_wav_output(&out);
        }
        if let Some(language) = voice_id.strip_prefix(Self::ESPEAK_PREFIX) {
            if language.is_empty() || !is_announcement_language(language) {
                return Err(SpeechError::VoiceUnavailable);
            }
            let program = find_on_path("espeak-ng").ok_or(SpeechError::VoiceUnavailable)?;
            let (_guard, out) = scratch_wav("espeak_scratch")?;
            let mut cmd = Command::new(program);
            cmd.arg("-v").arg(language).arg("-w").arg(&out).arg(text);
            run_with_budget(cmd, self.budget, "espeak-ng")?;
            return take_wav_output(&out);
        }
        Err(SpeechError::VoiceUnavailable)
    }
}

#[allow(dead_code)]
fn _unused(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_say_listing_keeping_only_french_voices() {
        let listing = "Alex                en_US    # Most people recognize me by my voice.\n\
                       Amélie (Enhanced)   fr_CA    # Bonjour, je m'appelle Amélie.\n\
                       Thomas              fr_FR    # Bonjour, je m'appelle Thomas.\n\
                       Daniel              en_GB    # Hello, my name is Daniel.\n";
        let voices = MacSay::parse_voice_list(listing);
        assert_eq!(voices.len(), 2);
        assert_eq!(voices[0].id, "system:say:Amélie (Enhanced)");
        assert_eq!(voices[0].name, "Amélie (Enhanced)");
        assert_eq!(voices[0].language, "fr-CA");
        assert_eq!(voices[1].id, "system:say:Thomas");
        assert_eq!(voices[1].engine, VoiceEngine::System);
    }

    #[test]
    fn parses_the_espeak_listing_keeping_only_french_voices() {
        let listing = "Pty Language       Age/Gender VoiceName          File                 Other Languages\n\
                        5  fr              --/M      French             gmw/fr               (fr-fr 5)\n\
                        5  fr-BE           --/M      French_(Belgium)   roa/fr-BE            (fr 8)\n\
                        5  de              --/M      German             gmw/de\n";
        let voices = LinuxSpeech::parse_espeak_voices(listing);
        assert_eq!(voices.len(), 2);
        assert_eq!(voices[0].id, "system:espeak-ng:fr");
        assert_eq!(voices[0].name, "French (espeak-ng)");
        assert_eq!(voices[1].language, "fr-BE");
    }

    #[test]
    fn unknown_voice_ids_are_refused_without_running_anything() {
        let mac = MacSay::default();
        assert_eq!(
            mac.synthesize("system:say:", "x"),
            Err(SpeechError::VoiceUnavailable)
        );
        assert_eq!(
            mac.synthesize("embedded:x", "x"),
            Err(SpeechError::VoiceUnavailable)
        );
        let linux = LinuxSpeech::default();
        assert_eq!(
            linux.synthesize("system:espeak-ng:de", "x"),
            Err(SpeechError::VoiceUnavailable)
        );
        assert_eq!(
            linux.synthesize("system:other", "x"),
            Err(SpeechError::VoiceUnavailable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_linux_engine_finds_fake_tools_on_the_path_and_reads_their_wav() {
        use super::super::process::test_support::{fake_program, path_with};
        use crate::infrastructure::device::audio_transcode::test_support::wav_sine;

        let dir = tempfile::tempdir().unwrap();
        let wav = wav_sine(16_000, 1, 300.0, 0.2, 0.2);
        let sample = dir.path().join("sample.wav");
        std::fs::write(&sample, &wav).unwrap();
        // pico2wave -l fr-FR -w <out> <text> ; espeak-ng --voices=fr | -v fr -w <out> <text>
        fake_program(
            dir.path(),
            "pico2wave",
            &format!("cp {} \"$4\"", sample.display()),
        );
        fake_program(
            dir.path(),
            "espeak-ng",
            &format!(
                "if [ \"$1\" = \"--voices=fr\" ]; then printf 'Pty Language Age/Gender VoiceName File\\n 5  fr  --/M  French  gmw/fr\\n'; else cp {} \"$4\"; fi",
                sample.display()
            ),
        );
        // The PATH is process-wide: serialize with the other PATH-touching test.
        let _guard = PATH_LOCK.lock().unwrap();
        let previous = std::env::var_os("PATH");
        std::env::set_var("PATH", path_with(dir.path()));
        let engine = LinuxSpeech::default();
        let voices = engine.list_voices();
        let out = engine.synthesize("system:pico2wave:fr-FR", "Bonjour");
        let out2 = engine.synthesize("system:espeak-ng:fr", "Bonjour");
        match previous {
            Some(p) => std::env::set_var("PATH", p),
            None => std::env::remove_var("PATH"),
        }
        assert_eq!(voices.len(), 2, "{voices:?}");
        assert_eq!(voices[0].id, "system:pico2wave:fr-FR");
        assert_eq!(voices[1].id, "system:espeak-ng:fr");
        assert_eq!(out.unwrap(), wav);
        assert_eq!(out2.unwrap(), wav);
    }

    #[cfg(unix)]
    #[test]
    fn the_say_engine_runs_the_command_and_reads_the_wav_it_wrote() {
        use super::super::process::test_support::fake_program;
        use crate::infrastructure::device::audio_transcode::test_support::wav_sine;

        let dir = tempfile::tempdir().unwrap();
        let wav = wav_sine(22_050, 1, 500.0, 0.2, 0.2);
        let sample = dir.path().join("sample.wav");
        std::fs::write(&sample, &wav).unwrap();
        // say -v <name> -o <out> --file-format=WAVE --data-format=… <text>
        let say = fake_program(
            dir.path(),
            "say",
            &format!(
                "if [ \"$1\" = \"-v\" ] && [ \"$2\" = \"?\" ]; then printf 'Thomas  fr_FR  # Bonjour.\\n'; else cp {} \"$4\"; fi",
                sample.display()
            ),
        );
        let engine = MacSay {
            program: say.to_string_lossy().into_owned(),
            budget: Duration::from_secs(5),
        };
        let voices = engine.list_voices();
        assert_eq!(voices.len(), 1);
        assert_eq!(
            engine.synthesize("system:say:Thomas", "Bonjour").unwrap(),
            wav
        );
    }

    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
