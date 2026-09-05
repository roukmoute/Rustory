//! Wire shapes of a story's PRESENTATION on the device (its layout and its
//! spoken announcements) and of the announcement VOICES. Mirrored in
//! `src/shared/ipc-contracts/presentation.ts`; contract tests on both sides.

use serde::{Deserialize, Serialize};

use crate::application::story::presentation::{
    Announcement, AnnouncementSource, AnnouncementStatus, AnnouncementTarget, ChapterAnnouncement,
    LinearBlocker, StoryPresentation,
};
use crate::domain::device::StoryLayout;
use crate::infrastructure::speech::{EmbeddedVoiceStatus, Voice, VoiceEngine};
use crate::ipc::dto::SendBlockerDto;

/// `sequential` | `menu` — see `StoryLayout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoryLayoutDto {
    Sequential,
    Menu,
}

impl StoryLayoutDto {
    pub const fn from_domain(layout: StoryLayout) -> Self {
        match layout {
            StoryLayout::Sequential => Self::Sequential,
            StoryLayout::Menu => Self::Menu,
        }
    }

    pub const fn to_domain(self) -> StoryLayout {
        match self {
            Self::Sequential => StoryLayout::Sequential,
            Self::Menu => StoryLayout::Menu,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnouncementStatusDto {
    Ready,
    Stale,
    Missing,
}

impl AnnouncementStatusDto {
    pub const fn from_domain(status: AnnouncementStatus) -> Self {
        match status {
            AnnouncementStatus::Ready => Self::Ready,
            AnnouncementStatus::Stale => Self::Stale,
            AnnouncementStatus::Missing => Self::Missing,
        }
    }
}

/// `voice` (synthesized) | `recorded` (the user's microphone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnouncementSourceDto {
    Voice,
    Recorded,
}

impl AnnouncementSourceDto {
    pub const fn from_domain(source: AnnouncementSource) -> Self {
        match source {
            AnnouncementSource::Voice => Self::Voice,
            AnnouncementSource::Recorded => Self::Recorded,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnouncementDto {
    pub spoken_text: String,
    pub status: AnnouncementStatusDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    /// Where the stored clip came from; absent while missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AnnouncementSourceDto>,
}

impl AnnouncementDto {
    fn from_domain(announcement: &Announcement) -> Self {
        Self {
            spoken_text: announcement.spoken_text.clone(),
            status: AnnouncementStatusDto::from_domain(announcement.status),
            asset_id: announcement.asset_id.clone(),
            source: announcement.source.map(AnnouncementSourceDto::from_domain),
        }
    }
}

/// Which announcement an attach / remove targets. Tagged on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AnnouncementTargetDto {
    Title,
    Question,
    #[serde(rename_all = "camelCase")]
    Chapter {
        node_id: String,
    },
}

impl AnnouncementTargetDto {
    pub fn to_domain(&self) -> AnnouncementTarget {
        match self {
            Self::Title => AnnouncementTarget::Title,
            Self::Question => AnnouncementTarget::Question,
            Self::Chapter { node_id } => AnnouncementTarget::Chapter {
                node_id: node_id.clone(),
            },
        }
    }
}

/// A microphone recording for one announcement: WAV bytes, base64.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachRecordedAnnouncementInputDto {
    pub story_id: String,
    pub target: AnnouncementTargetDto,
    pub audio_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveAnnouncementInputDto {
    pub story_id: String,
    pub target: AnnouncementTargetDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterAnnouncementDto {
    pub node_id: String,
    pub label: String,
    pub spoken_text: String,
    pub status: AnnouncementStatusDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AnnouncementSourceDto>,
}

impl ChapterAnnouncementDto {
    fn from_domain(chapter: &ChapterAnnouncement) -> Self {
        Self {
            node_id: chapter.node_id.clone(),
            label: chapter.label.clone(),
            spoken_text: chapter.announcement.spoken_text.clone(),
            status: AnnouncementStatusDto::from_domain(chapter.announcement.status),
            asset_id: chapter.announcement.asset_id.clone(),
            source: chapter
                .announcement
                .source
                .map(AnnouncementSourceDto::from_domain),
        }
    }
}

/// Read-model returned by `read_story_presentation` / `set_story_layout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryPresentationDto {
    pub layout: StoryLayoutDto,
    /// The voice the stored announcements were generated with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    /// `true` iff the story is sent from its retained source archive — the
    /// layout then does not apply to the send.
    pub archive_retained: bool,
    /// `true` iff the structure lays out as episodes (announcements make
    /// sense); `false` for a story with choices or without audio.
    pub linear: bool,
    /// When `linear` is false: the reason, and the first node to fix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linear_blocker: Option<LinearBlockerDto>,
    pub title: AnnouncementDto,
    pub question: AnnouncementDto,
    pub chapters: Vec<ChapterAnnouncementDto>,
}

/// Why the structure does not lay out as episodes, naming the node to fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearBlockerDto {
    pub reason: SendBlockerDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl LinearBlockerDto {
    fn from_domain(blocker: &LinearBlocker) -> Self {
        Self {
            reason: SendBlockerDto::from_domain(blocker.blocker),
            node_id: blocker.node_id.clone(),
            label: blocker.label.clone(),
        }
    }
}

impl StoryPresentationDto {
    pub fn from_domain(presentation: &StoryPresentation, archive_retained: bool) -> Self {
        Self {
            layout: StoryLayoutDto::from_domain(presentation.layout),
            voice_id: presentation.voice_id.clone(),
            archive_retained,
            linear: presentation.linear,
            linear_blocker: presentation
                .linear_blocker
                .as_ref()
                .map(LinearBlockerDto::from_domain),
            title: AnnouncementDto::from_domain(&presentation.title),
            question: AnnouncementDto::from_domain(&presentation.question),
            chapters: presentation
                .chapters
                .iter()
                .map(ChapterAnnouncementDto::from_domain)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetStoryLayoutInputDto {
    pub story_id: String,
    pub layout: StoryLayoutDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAnnouncementsInputDto {
    pub story_id: String,
    /// Regenerate the ready clips too (a voice change is detected on its
    /// own; `force` is for "I want them again").
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAnnouncementsOutcomeDto {
    pub generated: u32,
    pub planned: u32,
    pub voice_id: String,
    pub presentation: StoryPresentationDto,
}

// ===== voices =====

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceEngineDto {
    System,
    Embedded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnouncementVoiceDto {
    pub id: String,
    pub name: String,
    pub language: String,
    pub engine: VoiceEngineDto,
}

impl AnnouncementVoiceDto {
    pub fn from_domain(voice: &Voice) -> Self {
        Self {
            id: voice.id.clone(),
            name: voice.name.clone(),
            language: voice.language.clone(),
            engine: match voice.engine {
                VoiceEngine::System => VoiceEngineDto::System,
                VoiceEngine::Embedded => VoiceEngineDto::Embedded,
            },
        }
    }
}

/// `unsupported` | `notInstalled` | `installing` | `installed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EmbeddedVoiceStateDto {
    Unsupported,
    NotInstalled,
    Installing,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedVoiceStatusDto {
    pub state: EmbeddedVoiceStateDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Bytes to download for an install (0 when unsupported).
    pub download_bytes: u64,
    pub voice_id: String,
    pub voice_name: String,
}

impl EmbeddedVoiceStatusDto {
    pub fn from_domain(
        status: &EmbeddedVoiceStatus,
        installing: bool,
        download_bytes: u64,
    ) -> Self {
        let (state, version) = match status {
            EmbeddedVoiceStatus::Unsupported => (EmbeddedVoiceStateDto::Unsupported, None),
            EmbeddedVoiceStatus::NotInstalled if installing => {
                (EmbeddedVoiceStateDto::Installing, None)
            }
            EmbeddedVoiceStatus::NotInstalled => (EmbeddedVoiceStateDto::NotInstalled, None),
            EmbeddedVoiceStatus::Installed { version } => {
                (EmbeddedVoiceStateDto::Installed, Some(version.clone()))
            }
        };
        Self {
            state,
            version,
            download_bytes,
            voice_id: crate::infrastructure::speech::EMBEDDED_VOICE_ID.to_string(),
            voice_name: crate::infrastructure::speech::EMBEDDED_VOICE_NAME.to_string(),
        }
    }
}

/// Read-model returned by `read_announcement_voices` / `set_announcement_voice`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnouncementVoicesDto {
    /// The French voices available now, system voices first.
    pub voices: Vec<AnnouncementVoiceDto>,
    /// The voice announcements are generated with: the stored choice when
    /// it is still available, else the first available voice. Absent when
    /// no voice exists at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_voice_id: Option<String>,
    /// `true` iff `selected_voice_id` comes from the stored setting.
    pub selected_is_stored: bool,
    pub embedded: EmbeddedVoiceStatusDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAnnouncementVoiceInputDto {
    pub voice_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAnnouncementVoiceInputDto {
    pub voice_id: String,
}

/// A spoken sample, as a `data:audio/wav;base64,…` URL the webview plays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoicePreviewDto {
    pub data_url: String,
    pub duration_ms: u64,
    pub spoken_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_presentation_serializes_in_camel_case_with_closed_tags() {
        let dto = StoryPresentationDto {
            layout: StoryLayoutDto::Menu,
            voice_id: Some("system:say:Thomas".into()),
            archive_retained: false,
            linear: true,
            linear_blocker: None,
            title: AnnouncementDto {
                spoken_text: "Série.".into(),
                status: AnnouncementStatusDto::Ready,
                asset_id: Some("a1".into()),
                source: Some(AnnouncementSourceDto::Voice),
            },
            question: AnnouncementDto {
                spoken_text: "Quelle histoire veux-tu écouter ?".into(),
                status: AnnouncementStatusDto::Missing,
                asset_id: None,
                source: None,
            },
            chapters: vec![ChapterAnnouncementDto {
                node_id: "n1".into(),
                label: "Un".into(),
                spoken_text: "Un.".into(),
                status: AnnouncementStatusDto::Stale,
                asset_id: Some("a2".into()),
                source: Some(AnnouncementSourceDto::Recorded),
            }],
        };
        let v = serde_json::to_value(&dto).unwrap();
        assert_eq!(v["layout"], "menu");
        assert_eq!(v["voiceId"], "system:say:Thomas");
        assert_eq!(v["archiveRetained"], false);
        assert_eq!(v["title"]["status"], "ready");
        assert_eq!(v["title"]["source"], "voice");
        assert_eq!(v["question"]["status"], "missing");
        assert!(v["question"].get("source").is_none());
        assert_eq!(v["chapters"][0]["source"], "recorded");
        assert!(v["question"].get("assetId").is_none());
        assert_eq!(v["chapters"][0]["nodeId"], "n1");
        assert_eq!(v["chapters"][0]["status"], "stale");
        assert!(
            v.to_string().find("spoken_text").is_none(),
            "no snake_case on the wire"
        );
        assert!(v.get("linearBlocker").is_none(), "absent when linear");
        let blocked = StoryPresentationDto {
            linear: false,
            linear_blocker: Some(LinearBlockerDto::from_domain(&LinearBlocker {
                blocker: crate::domain::device::StoryPackBlocker::MissingAudio,
                node_id: Some("n2".into()),
                label: Some("Deux".into()),
            })),
            chapters: Vec::new(),
            ..dto
        };
        let v = serde_json::to_value(&blocked).unwrap();
        assert_eq!(
            v["linearBlocker"],
            serde_json::json!({ "reason": "missingAudio", "nodeId": "n2", "label": "Deux" })
        );
    }

    #[test]
    fn inputs_deserialize_from_camel_case_and_layout_round_trips() {
        let input: SetStoryLayoutInputDto =
            serde_json::from_str(r#"{"storyId":"s1","layout":"menu"}"#).unwrap();
        assert_eq!(input.layout, StoryLayoutDto::Menu);
        assert_eq!(input.layout.to_domain(), StoryLayout::Menu);
        assert_eq!(
            StoryLayoutDto::from_domain(StoryLayout::Sequential),
            StoryLayoutDto::Sequential
        );
        let gen: GenerateAnnouncementsInputDto =
            serde_json::from_str(r#"{"storyId":"s1"}"#).unwrap();
        assert!(!gen.force);
        assert!(serde_json::from_str::<SetStoryLayoutInputDto>(
            r#"{"storyId":"s1","layout":"carousel"}"#
        )
        .is_err());
    }

    #[test]
    fn announcement_targets_parse_from_their_kind_tag() {
        let title: AnnouncementTargetDto = serde_json::from_str(r#"{"kind":"title"}"#).unwrap();
        assert_eq!(title.to_domain(), AnnouncementTarget::Title);
        let chapter: AnnouncementTargetDto =
            serde_json::from_str(r#"{"kind":"chapter","nodeId":"n3"}"#).unwrap();
        assert_eq!(
            chapter.to_domain(),
            AnnouncementTarget::Chapter {
                node_id: "n3".into()
            }
        );
        assert!(serde_json::from_str::<AnnouncementTargetDto>(r#"{"kind":"cover"}"#).is_err());
        let input: AttachRecordedAnnouncementInputDto = serde_json::from_str(
            r#"{"storyId":"s1","target":{"kind":"question"},"audioBase64":"UklGRg=="}"#,
        )
        .unwrap();
        assert_eq!(input.target, AnnouncementTargetDto::Question);
    }

    #[test]
    fn voices_and_embedded_status_serialize_with_closed_tags() {
        let dto = AnnouncementVoicesDto {
            voices: vec![AnnouncementVoiceDto::from_domain(&Voice {
                id: "embedded:fr_FR-siwis-medium".into(),
                name: "Siwis".into(),
                language: "fr-FR".into(),
                engine: VoiceEngine::Embedded,
            })],
            selected_voice_id: Some("embedded:fr_FR-siwis-medium".into()),
            selected_is_stored: true,
            embedded: EmbeddedVoiceStatusDto::from_domain(
                &EmbeddedVoiceStatus::Installed {
                    version: "2023.11.14-2".into(),
                },
                false,
                90_000_000,
            ),
        };
        let v = serde_json::to_value(&dto).unwrap();
        assert_eq!(v["voices"][0]["engine"], "embedded");
        assert_eq!(v["selectedIsStored"], true);
        assert_eq!(v["embedded"]["state"], "installed");
        assert_eq!(v["embedded"]["version"], "2023.11.14-2");
        assert_eq!(v["embedded"]["downloadBytes"], 90_000_000);
        let installing =
            EmbeddedVoiceStatusDto::from_domain(&EmbeddedVoiceStatus::NotInstalled, true, 1);
        assert_eq!(
            serde_json::to_value(installing).unwrap()["state"],
            "installing"
        );
        let unsupported =
            EmbeddedVoiceStatusDto::from_domain(&EmbeddedVoiceStatus::Unsupported, false, 0);
        assert_eq!(
            serde_json::to_value(unsupported).unwrap()["state"],
            "unsupported"
        );
    }
}
