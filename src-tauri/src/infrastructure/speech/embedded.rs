//! The EMBEDDED neural voice: the Piper text-to-speech runtime and a French
//! voice model, downloaded on explicit request and run as a separate
//! process. Same quality on every OS, no dependency in the build.
//!
//! Piper (rhasspy/piper, MIT) bundles ONNX Runtime and its phonemizer
//! (espeak-ng, GPL-3.0); it is NEVER linked into Rustory — it is spawned
//! like any command, its output WAV read from a file. The voice
//! (`fr_FR-siwis-medium`, dataset CC-BY 4.0, 22050 Hz) is the model the
//! Piper project publishes for French.
//!
//! Install discipline, fail-closed: every artifact has a PINNED url and
//! SHA-256 ([`EmbeddedManifest::official`]); bytes are verified BEFORE any
//! extraction; the archive is extracted into a staging dir then promoted by
//! rename; a marker file records the installed manifest so the status is a
//! plain read. Nothing is downloaded without the user's gesture, nothing is
//! executed that failed its checksum.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::process::{take_wav_output, wait_with_budget, SYNTHESIS_BUDGET};
use super::{SpeechError, SpeechSynthesizer, Voice, VoiceEngine};

/// The stable id of the embedded voice (one French voice today).
pub const EMBEDDED_VOICE_ID: &str = "embedded:fr_FR-siwis-medium";
/// Its user-facing name.
pub const EMBEDDED_VOICE_NAME: &str = "Voix neuronale française (Siwis)";
/// Sub-directory of the app data dir holding the runtime and the voices.
pub const SPEECH_DIR_NAME: &str = "speech";

const PIPER_VERSION: &str = "2023.11.14-2";
const VOICE_FILE: &str = "fr_FR-siwis-medium.onnx";
const VOICE_CONFIG_FILE: &str = "fr_FR-siwis-medium.onnx.json";
const MARKER_FILE: &str = "installed.json";

/// One downloadable artifact: where, how big, and what its bytes must hash to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// How the runtime archive is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    TarGz,
    Zip,
}

/// Everything an install needs, per platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedManifest {
    pub version: String,
    pub runtime: Artifact,
    pub runtime_kind: ArchiveKind,
    /// Path of the executable inside the extracted archive.
    pub runtime_binary: String,
    pub model: Artifact,
    pub config: Artifact,
}

impl EmbeddedManifest {
    /// The pinned official artifacts for THIS build's platform; `None` on a
    /// platform Piper does not ship for.
    pub fn official() -> Option<Self> {
        let (file, size, sha256, kind, binary) = runtime_for_platform()?;
        Some(Self {
            version: PIPER_VERSION.into(),
            runtime: Artifact {
                url: format!("https://github.com/rhasspy/piper/releases/download/{PIPER_VERSION}/{file}"),
                size,
                sha256: sha256.into(),
            },
            runtime_kind: kind,
            runtime_binary: binary.into(),
            model: Artifact {
                url: format!("https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR/siwis/medium/{VOICE_FILE}"),
                size: 63_201_294,
                sha256: "641d1ab097da2b81128c076810edb052b385decc8be3381814802a64a73baf99".into(),
            },
            config: Artifact {
                url: format!("https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR/siwis/medium/{VOICE_CONFIG_FILE}"),
                size: 4_875,
                sha256: "39479916c2db192b5ac9764daddd0c744d83e023ad890c6976c0633ae4df8959".into(),
            },
        })
    }

    /// Total bytes to download.
    pub fn total_size(&self) -> u64 {
        self.runtime.size + self.model.size + self.config.size
    }
}

/// `(archive file, size, sha256, kind, binary path in archive)` of the Piper
/// release for this platform.
fn runtime_for_platform() -> Option<(&'static str, u64, &'static str, ArchiveKind, &'static str)> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some((
            "piper_linux_x86_64.tar.gz",
            26_460_462,
            "a50cb45f355b7af1f6d758c1b360717877ba0a398cc8cbe6d2a7a3a26e225992",
            ArchiveKind::TarGz,
            "piper/piper",
        ))
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Some((
            "piper_macos_aarch64.tar.gz",
            19_146_957,
            "6b1eb03b3735946cb35216e063e7eebcc33a6bbf5dd96ec0217959bf1cdcb0cc",
            ArchiveKind::TarGz,
            "piper/piper",
        ))
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Some((
            "piper_windows_amd64.zip",
            22_477_236,
            "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea",
            ArchiveKind::Zip,
            "piper/piper.exe",
        ))
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        None
    }
}

/// Whether the embedded voice is usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedVoiceStatus {
    /// Piper ships no build for this platform.
    Unsupported,
    /// Not downloaded yet (or a previous install did not complete).
    NotInstalled,
    /// Runtime + voice present; `version` is the installed Piper release.
    Installed { version: String },
}

/// Download / install progress, in bytes over the manifest total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedInstallProgress {
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// Why an install failed. Closed, PII-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedError {
    Unsupported,
    /// The download failed (network, HTTP status, size bound).
    Download(&'static str),
    /// The bytes do not match the pinned checksum — nothing is kept.
    Checksum(&'static str),
    /// The archive could not be extracted or promoted.
    Install(&'static str),
}

impl EmbeddedError {
    pub const fn diagnostic_tag(&self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Download(_) => "download",
            Self::Checksum(_) => "checksum",
            Self::Install(_) => "install",
        }
    }

    pub const fn stage(&self) -> &'static str {
        match self {
            Self::Unsupported => "platform",
            Self::Download(s) | Self::Checksum(s) | Self::Install(s) => s,
        }
    }
}

/// Fetches an artifact's bytes, reporting progress. The production
/// implementation is HTTP; tests serve local bytes.
pub trait ArtifactFetcher {
    fn fetch(&self, url: &str, on_progress: &mut dyn FnMut(u64)) -> Result<Vec<u8>, EmbeddedError>;
}

/// The embedded voice at `<app_data_dir>/speech`.
pub struct EmbeddedVoice {
    root: PathBuf,
    manifest: Option<EmbeddedManifest>,
    budget: Duration,
}

impl EmbeddedVoice {
    pub fn new(app_data_dir: &Path) -> Self {
        Self::with_manifest(app_data_dir, EmbeddedManifest::official())
    }

    /// An embedded voice over an explicit manifest (tests, other voices).
    pub fn with_manifest(app_data_dir: &Path, manifest: Option<EmbeddedManifest>) -> Self {
        Self {
            root: app_data_dir.join(SPEECH_DIR_NAME),
            manifest,
            budget: SYNTHESIS_BUDGET,
        }
    }

    pub fn manifest(&self) -> Option<&EmbeddedManifest> {
        self.manifest.as_ref()
    }

    fn runtime_dir(&self) -> PathBuf {
        self.root.join("runtime")
    }

    fn voices_dir(&self) -> PathBuf {
        self.root.join("voices")
    }

    fn binary_path(&self) -> Option<PathBuf> {
        self.manifest
            .as_ref()
            .map(|m| self.runtime_dir().join(&m.runtime_binary))
    }

    fn model_path(&self) -> PathBuf {
        self.voices_dir().join(VOICE_FILE)
    }

    fn config_path(&self) -> PathBuf {
        self.voices_dir().join(VOICE_CONFIG_FILE)
    }

    /// A plain read: the marker written by a completed install, the binary
    /// and the voice files all present.
    pub fn status(&self) -> EmbeddedVoiceStatus {
        let Some(manifest) = self.manifest.as_ref() else {
            return EmbeddedVoiceStatus::Unsupported;
        };
        let marker = std::fs::read_to_string(self.root.join(MARKER_FILE)).ok();
        let version_ok = marker
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v["version"].as_str().map(|s| s == manifest.version))
            .unwrap_or(false);
        let files_ok = self.binary_path().is_some_and(|p| p.is_file())
            && self.model_path().is_file()
            && self.config_path().is_file();
        if version_ok && files_ok {
            EmbeddedVoiceStatus::Installed {
                version: manifest.version.clone(),
            }
        } else {
            EmbeddedVoiceStatus::NotInstalled
        }
    }

    /// Download, verify and install the runtime and the voice. Progress is
    /// reported in bytes over the manifest total.
    pub fn install(
        &self,
        fetcher: &dyn ArtifactFetcher,
        on_progress: &dyn Fn(EmbeddedInstallProgress),
    ) -> Result<(), EmbeddedError> {
        let manifest = self.manifest.as_ref().ok_or(EmbeddedError::Unsupported)?;
        let total = manifest.total_size();
        let mut base = 0u64;
        let mut fetch_verified = |artifact: &Artifact, stage: &'static str| {
            let mut report = |done: u64| {
                on_progress(EmbeddedInstallProgress {
                    bytes_done: (base + done).min(total),
                    bytes_total: total,
                });
            };
            let bytes = fetcher.fetch(&artifact.url, &mut report)?;
            if bytes.len() as u64 > artifact.size.saturating_mul(2).max(64 * 1024 * 1024) {
                return Err(EmbeddedError::Download(stage));
            }
            if sha256_hex(&bytes) != artifact.sha256 {
                return Err(EmbeddedError::Checksum(stage));
            }
            base += artifact.size;
            Ok::<Vec<u8>, EmbeddedError>(bytes)
        };
        let runtime_bytes = fetch_verified(&manifest.runtime, "runtime")?;
        let model_bytes = fetch_verified(&manifest.model, "model")?;
        let config_bytes = fetch_verified(&manifest.config, "config")?;

        // Stage everything, then promote by rename — a crash mid-way leaves
        // the previous install (or nothing) intact.
        std::fs::create_dir_all(&self.root).map_err(|_| EmbeddedError::Install("root"))?;
        let staging = self.root.join(".staging");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|_| EmbeddedError::Install("staging"))?;
        let staged_runtime = staging.join("runtime");
        extract_archive(&runtime_bytes, manifest.runtime_kind, &staged_runtime)?;
        if !staged_runtime.join(&manifest.runtime_binary).is_file() {
            return Err(EmbeddedError::Install("binary_missing"));
        }
        let staged_voices = staging.join("voices");
        std::fs::create_dir_all(&staged_voices).map_err(|_| EmbeddedError::Install("voices"))?;
        std::fs::write(staged_voices.join(VOICE_FILE), &model_bytes)
            .map_err(|_| EmbeddedError::Install("model_write"))?;
        std::fs::write(staged_voices.join(VOICE_CONFIG_FILE), &config_bytes)
            .map_err(|_| EmbeddedError::Install("config_write"))?;

        let _ = std::fs::remove_file(self.root.join(MARKER_FILE));
        let _ = std::fs::remove_dir_all(self.runtime_dir());
        let _ = std::fs::remove_dir_all(self.voices_dir());
        std::fs::rename(&staged_runtime, self.runtime_dir())
            .map_err(|_| EmbeddedError::Install("promote_runtime"))?;
        std::fs::rename(&staged_voices, self.voices_dir())
            .map_err(|_| EmbeddedError::Install("promote_voices"))?;
        let _ = std::fs::remove_dir_all(&staging);
        let marker = serde_json::json!({
            "version": manifest.version,
            "voice": EMBEDDED_VOICE_ID,
            "runtimeSha256": manifest.runtime.sha256,
            "modelSha256": manifest.model.sha256,
        });
        std::fs::write(self.root.join(MARKER_FILE), marker.to_string())
            .map_err(|_| EmbeddedError::Install("marker"))?;
        on_progress(EmbeddedInstallProgress {
            bytes_done: total,
            bytes_total: total,
        });
        Ok(())
    }

    /// Remove the installed runtime and voice.
    pub fn uninstall(&self) -> Result<(), EmbeddedError> {
        let _ = std::fs::remove_file(self.root.join(MARKER_FILE));
        let _ = std::fs::remove_dir_all(self.runtime_dir());
        let _ = std::fs::remove_dir_all(self.voices_dir());
        Ok(())
    }
}

impl SpeechSynthesizer for EmbeddedVoice {
    fn list_voices(&self) -> Vec<Voice> {
        match self.status() {
            EmbeddedVoiceStatus::Installed { .. } => vec![Voice {
                id: EMBEDDED_VOICE_ID.into(),
                name: EMBEDDED_VOICE_NAME.into(),
                language: "fr-FR".into(),
                engine: VoiceEngine::Embedded,
            }],
            _ => Vec::new(),
        }
    }

    fn synthesize(&self, voice_id: &str, text: &str) -> Result<Vec<u8>, SpeechError> {
        if voice_id != EMBEDDED_VOICE_ID {
            return Err(SpeechError::VoiceUnavailable);
        }
        if !matches!(self.status(), EmbeddedVoiceStatus::Installed { .. }) {
            return Err(SpeechError::VoiceUnavailable);
        }
        let binary = self.binary_path().ok_or(SpeechError::VoiceUnavailable)?;
        let scratch =
            tempfile::tempdir().map_err(|_| SpeechError::EngineFailed("piper_scratch"))?;
        let out = scratch.path().join("speech.wav");
        let mut command = Command::new(&binary);
        command
            .arg("--model")
            .arg(self.model_path())
            .arg("--config")
            .arg(self.config_path())
            .arg("--output_file")
            .arg(&out)
            .current_dir(binary.parent().unwrap_or(&self.root))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|_| SpeechError::EngineFailed("piper_spawn"))?;
        {
            use std::io::Write;
            let mut stdin = child
                .stdin
                .take()
                .ok_or(SpeechError::EngineFailed("piper_stdin"))?;
            // One line = one utterance; a trailing newline ends the input.
            let line = text.replace(['\r', '\n'], " ");
            stdin
                .write_all(format!("{line}\n").as_bytes())
                .map_err(|_| SpeechError::EngineFailed("piper_stdin"))?;
        }
        wait_with_budget(child, self.budget, "piper")?;
        take_wav_output(&out)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Extract `bytes` (tar.gz or zip) under `dest`, refusing entries that
/// would escape it, and restoring the executable bit of tar entries.
fn extract_archive(bytes: &[u8], kind: ArchiveKind, dest: &Path) -> Result<(), EmbeddedError> {
    std::fs::create_dir_all(dest).map_err(|_| EmbeddedError::Install("extract_dir"))?;
    match kind {
        ArchiveKind::TarGz => {
            let gz = flate2::read::GzDecoder::new(bytes);
            let mut archive = tar::Archive::new(gz);
            archive.set_preserve_permissions(true);
            for entry in archive
                .entries()
                .map_err(|_| EmbeddedError::Install("tar_read"))?
            {
                let mut entry = entry.map_err(|_| EmbeddedError::Install("tar_entry"))?;
                let path = entry
                    .path()
                    .map_err(|_| EmbeddedError::Install("tar_path"))?
                    .into_owned();
                if !is_safe_relative(&path) {
                    return Err(EmbeddedError::Install("tar_escape"));
                }
                entry
                    .unpack_in(dest)
                    .map_err(|_| EmbeddedError::Install("tar_unpack"))?;
            }
            Ok(())
        }
        ArchiveKind::Zip => {
            let cursor = std::io::Cursor::new(bytes);
            let mut archive =
                zip::ZipArchive::new(cursor).map_err(|_| EmbeddedError::Install("zip_read"))?;
            for i in 0..archive.len() {
                let mut file = archive
                    .by_index(i)
                    .map_err(|_| EmbeddedError::Install("zip_entry"))?;
                let Some(rel) = file.enclosed_name() else {
                    return Err(EmbeddedError::Install("zip_escape"));
                };
                let target = dest.join(rel);
                if file.is_dir() {
                    std::fs::create_dir_all(&target)
                        .map_err(|_| EmbeddedError::Install("zip_dir"))?;
                    continue;
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|_| EmbeddedError::Install("zip_dir"))?;
                }
                let mut out = std::fs::File::create(&target)
                    .map_err(|_| EmbeddedError::Install("zip_file"))?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)
                    .map_err(|_| EmbeddedError::Install("zip_file"))?;
                std::io::Write::write_all(&mut out, &buf)
                    .map_err(|_| EmbeddedError::Install("zip_file"))?;
            }
            Ok(())
        }
    }
}

fn is_safe_relative(path: &Path) -> bool {
    use std::path::Component;
    !path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// The production fetcher: HTTP(S) with a bounded, streamed download.
pub struct HttpArtifactFetcher {
    client: reqwest::blocking::Client,
}

impl Default for HttpArtifactFetcher {
    fn default() -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("Rustory/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(600))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client }
    }
}

/// Hard bound on one artifact download (the largest is ~63 MB).
const MAX_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;

impl ArtifactFetcher for HttpArtifactFetcher {
    fn fetch(&self, url: &str, on_progress: &mut dyn FnMut(u64)) -> Result<Vec<u8>, EmbeddedError> {
        let mut response = self
            .client
            .get(url)
            .send()
            .map_err(|_| EmbeddedError::Download("request"))?;
        if !response.status().is_success() {
            return Err(EmbeddedError::Download("status"));
        }
        let mut bytes = Vec::new();
        let mut chunk = vec![0u8; 256 * 1024];
        loop {
            let read = response
                .read(&mut chunk)
                .map_err(|_| EmbeddedError::Download("read"))?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
            if bytes.len() > MAX_ARTIFACT_BYTES {
                return Err(EmbeddedError::Download("oversize"));
            }
            on_progress(bytes.len() as u64);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Serves artifacts from memory, counting requests.
    struct LocalFetcher {
        files: HashMap<String, Vec<u8>>,
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl ArtifactFetcher for LocalFetcher {
        fn fetch(
            &self,
            url: &str,
            on_progress: &mut dyn FnMut(u64),
        ) -> Result<Vec<u8>, EmbeddedError> {
            self.calls.borrow_mut().push(url.to_string());
            let bytes = self
                .files
                .get(url)
                .cloned()
                .ok_or(EmbeddedError::Download("missing"))?;
            on_progress(bytes.len() as u64 / 2);
            on_progress(bytes.len() as u64);
            Ok(bytes)
        }
    }

    /// A tar.gz holding `piper/piper` (a shell script that copies `sample`
    /// to the requested `--output_file`) — the shape of the real release.
    #[cfg(unix)]
    fn fake_runtime_targz(sample: &Path) -> Vec<u8> {
        let script = format!(
            "#!/bin/sh\nout=\"\"\nwhile [ $# -gt 0 ]; do if [ \"$1\" = \"--output_file\" ]; then out=\"$2\"; shift; fi; shift; done\ncat >/dev/null\ncp {} \"$out\"\n",
            sample.display()
        );
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.set_size(script.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "piper/piper", script.as_bytes())
                .unwrap();
            builder.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        gz.finish().unwrap()
    }

    fn artifact(url: &str, bytes: &[u8]) -> Artifact {
        Artifact {
            url: url.into(),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        }
    }

    #[cfg(unix)]
    fn manifest_and_fetcher(sample: &Path) -> (EmbeddedManifest, LocalFetcher) {
        let runtime = fake_runtime_targz(sample);
        let model = b"fake onnx model".to_vec();
        let config = b"{\"audio\":{\"sample_rate\":22050}}".to_vec();
        let manifest = EmbeddedManifest {
            version: "test-1".into(),
            runtime: artifact("local://runtime", &runtime),
            runtime_kind: ArchiveKind::TarGz,
            runtime_binary: "piper/piper".into(),
            model: artifact("local://model", &model),
            config: artifact("local://config", &config),
        };
        let mut files = HashMap::new();
        files.insert("local://runtime".to_string(), runtime);
        files.insert("local://model".to_string(), model);
        files.insert("local://config".to_string(), config);
        (
            manifest,
            LocalFetcher {
                files,
                calls: Default::default(),
            },
        )
    }

    #[test]
    fn a_fresh_app_data_dir_has_no_embedded_voice() {
        let dir = tempfile::tempdir().unwrap();
        let voice = EmbeddedVoice::with_manifest(
            dir.path(),
            Some(EmbeddedManifest {
                version: "x".into(),
                runtime: artifact("u", b""),
                runtime_kind: ArchiveKind::TarGz,
                runtime_binary: "piper/piper".into(),
                model: artifact("m", b""),
                config: artifact("c", b""),
            }),
        );
        assert_eq!(voice.status(), EmbeddedVoiceStatus::NotInstalled);
        assert!(voice.list_voices().is_empty());
        assert_eq!(
            voice.synthesize(EMBEDDED_VOICE_ID, "x"),
            Err(SpeechError::VoiceUnavailable)
        );
        let unsupported = EmbeddedVoice::with_manifest(dir.path(), None);
        assert_eq!(unsupported.status(), EmbeddedVoiceStatus::Unsupported);
    }

    #[cfg(unix)]
    #[test]
    fn installs_verifies_promotes_then_lists_and_speaks_with_the_runtime() {
        use crate::infrastructure::device::audio_transcode::test_support::wav_sine;

        let dir = tempfile::tempdir().unwrap();
        let sample = dir.path().join("sample.wav");
        let wav = wav_sine(22_050, 1, 440.0, 0.3, 0.3);
        std::fs::write(&sample, &wav).unwrap();
        let (manifest, fetcher) = manifest_and_fetcher(&sample);
        let app_data = dir.path().join("app");
        let voice = EmbeddedVoice::with_manifest(&app_data, Some(manifest.clone()));

        let progress = std::cell::RefCell::new(Vec::new());
        voice
            .install(&fetcher, &|p| progress.borrow_mut().push(p))
            .expect("install");
        let progress = progress.borrow();
        assert!(progress
            .windows(2)
            .all(|w| w[0].bytes_done <= w[1].bytes_done));
        assert_eq!(progress.last().unwrap().bytes_done, manifest.total_size());
        assert_eq!(
            voice.status(),
            EmbeddedVoiceStatus::Installed {
                version: "test-1".into()
            }
        );
        assert_eq!(fetcher.calls.borrow().len(), 3);
        assert!(!app_data.join(SPEECH_DIR_NAME).join(".staging").exists());

        let voices = voice.list_voices();
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].id, EMBEDDED_VOICE_ID);
        assert_eq!(voices[0].engine, VoiceEngine::Embedded);
        assert_eq!(voice.synthesize(EMBEDDED_VOICE_ID, "Bonjour").unwrap(), wav);

        voice.uninstall().unwrap();
        assert_eq!(voice.status(), EmbeddedVoiceStatus::NotInstalled);
    }

    #[cfg(unix)]
    #[test]
    fn a_checksum_mismatch_installs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let sample = dir.path().join("sample.wav");
        std::fs::write(&sample, b"RIFF").unwrap();
        let (mut manifest, fetcher) = manifest_and_fetcher(&sample);
        manifest.model.sha256 = "0".repeat(64);
        let app_data = dir.path().join("app");
        let voice = EmbeddedVoice::with_manifest(&app_data, Some(manifest));
        assert_eq!(
            voice.install(&fetcher, &|_| {}),
            Err(EmbeddedError::Checksum("model"))
        );
        assert_eq!(voice.status(), EmbeddedVoiceStatus::NotInstalled);
        assert!(!app_data.join(SPEECH_DIR_NAME).join("runtime").exists());
    }

    #[test]
    fn archive_entries_that_escape_the_destination_are_refused() {
        let mut tar_bytes = Vec::new();
        {
            // The builder refuses `..` in a path, so the header is forged
            // the way a hostile archive would be: raw name bytes.
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.as_mut_bytes()[..7].copy_from_slice(b"../evil");
            header.set_size(2);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, &b"hi"[..]).unwrap();
            builder.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        let bytes = gz.finish().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let err = extract_archive(&bytes, ArchiveKind::TarGz, &dir.path().join("out")).unwrap_err();
        assert!(matches!(err, EmbeddedError::Install(_)), "{err:?}");
        assert!(!dir.path().join("evil").exists());
    }

    #[test]
    fn the_official_manifest_is_pinned_for_this_platform() {
        match EmbeddedManifest::official() {
            Some(m) => {
                assert!(m.runtime.url.contains(PIPER_VERSION));
                assert_eq!(m.runtime.sha256.len(), 64);
                assert_eq!(m.model.sha256.len(), 64);
                assert!(m.total_size() > 80_000_000);
            }
            None => {
                // An unsupported platform reports so, never a partial install.
                let dir = tempfile::tempdir().unwrap();
                assert_eq!(
                    EmbeddedVoice::new(dir.path()).status(),
                    EmbeddedVoiceStatus::Unsupported
                );
            }
        }
    }

    /// Real download of the official artifacts (~90 MB) into a temp dir,
    /// then a real synthesis — network-gated, run explicitly:
    /// `RUSTORY_TEST_EMBEDDED_VOICE=1 cargo test … -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_the_official_voice_and_speaks_a_title() {
        if std::env::var("RUSTORY_TEST_EMBEDDED_VOICE").is_err() {
            panic!("set RUSTORY_TEST_EMBEDDED_VOICE=1 to run this network test");
        }
        let dir = tempfile::tempdir().unwrap();
        let voice = EmbeddedVoice::new(dir.path());
        let last = std::cell::Cell::new(0u64);
        voice
            .install(&HttpArtifactFetcher::default(), &|p| last.set(p.bytes_done))
            .expect("install the official voice");
        assert!(matches!(
            voice.status(),
            EmbeddedVoiceStatus::Installed { .. }
        ));
        let wav = voice
            .synthesize(EMBEDDED_VOICE_ID, "Épisode 1. Le trésor de Moctezuma.")
            .expect("speak");
        let ms = super::super::wav::duration_ms(&wav).expect("wav duration");
        eprintln!("embedded voice: {} bytes, {ms} ms", wav.len());
        assert!((1_500..10_000).contains(&ms), "{ms} ms");
    }
}
