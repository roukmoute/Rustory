//! Wire contract of a story's presentation (layout + announcements) and of
//! the announcement voices — mirrored by `src/ipc/contract-tests/presentation.test.ts`.

use rustory_lib::ipc::dto::{
    AnnouncementDto, AnnouncementSourceDto, AnnouncementStatusDto, AnnouncementTargetDto,
    AnnouncementVoiceDto, AnnouncementVoicesDto, AttachRecordedAnnouncementInputDto,
    ChapterAnnouncementDto, EmbeddedVoiceStateDto, EmbeddedVoiceStatusDto,
    GenerateAnnouncementsInputDto, LinearBlockerDto, SendBlockerDto, SetAnnouncementVoiceInputDto,
    SetStoryLayoutInputDto, StoryLayoutDto, StoryPresentationDto, VoiceEngineDto, VoicePreviewDto,
};

#[test]
fn story_presentation_wire_shape() {
    let dto = StoryPresentationDto {
        layout: StoryLayoutDto::Menu,
        voice_id: Some("system:say:Thomas".into()),
        archive_retained: false,
        linear: true,
        linear_blocker: None,
        title: AnnouncementDto {
            spoken_text: "Tina et le serpent à plumes.".into(),
            status: AnnouncementStatusDto::Ready,
            asset_id: Some("a-title".into()),
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
            label: "Le trésor : épisode 1/10".into(),
            spoken_text: "Épisode 1. Le trésor.".into(),
            status: AnnouncementStatusDto::Stale,
            asset_id: Some("a-1".into()),
            source: Some(AnnouncementSourceDto::Recorded),
        }],
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(
        v,
        serde_json::json!({
            "layout": "menu",
            "voiceId": "system:say:Thomas",
            "archiveRetained": false,
            "linear": true,
            "title": { "spokenText": "Tina et le serpent à plumes.", "status": "ready", "assetId": "a-title", "source": "voice" },
            "question": { "spokenText": "Quelle histoire veux-tu écouter ?", "status": "missing" },
            "chapters": [{
                "nodeId": "n1",
                "label": "Le trésor : épisode 1/10",
                "spokenText": "Épisode 1. Le trésor.",
                "status": "stale",
                "assetId": "a-1",
                "source": "recorded"
            }]
        })
    );
}

#[test]
fn a_non_linear_presentation_names_the_node_to_fix() {
    let dto = StoryPresentationDto {
        layout: StoryLayoutDto::Sequential,
        voice_id: None,
        archive_retained: false,
        linear: false,
        linear_blocker: Some(LinearBlockerDto {
            reason: SendBlockerDto::MissingAudio,
            node_id: Some("n2".into()),
            label: Some("Deux".into()),
        }),
        title: AnnouncementDto {
            spoken_text: "Série.".into(),
            status: AnnouncementStatusDto::Missing,
            asset_id: None,
            source: None,
        },
        question: AnnouncementDto {
            spoken_text: "Quelle histoire veux-tu écouter ?".into(),
            status: AnnouncementStatusDto::Missing,
            asset_id: None,
            source: None,
        },
        chapters: Vec::new(),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["linear"], false);
    assert_eq!(
        v["linearBlocker"],
        serde_json::json!({ "reason": "missingAudio", "nodeId": "n2", "label": "Deux" })
    );
}

#[test]
fn recording_inputs_parse_their_target_kind() {
    let input: AttachRecordedAnnouncementInputDto = serde_json::from_str(
        r#"{"storyId":"s1","target":{"kind":"chapter","nodeId":"n2"},"audioBase64":"UklGRg=="}"#,
    )
    .expect("parse");
    assert_eq!(
        input.target,
        AnnouncementTargetDto::Chapter {
            node_id: "n2".into()
        }
    );
    assert!(serde_json::from_str::<AnnouncementTargetDto>(r#"{"kind":"chapter"}"#).is_err());
}

#[test]
fn presentation_inputs_parse_from_camel_case_and_refuse_unknown_layouts() {
    let input: SetStoryLayoutInputDto =
        serde_json::from_str(r#"{"storyId":"s1","layout":"sequential"}"#).expect("parse");
    assert_eq!(input.layout, StoryLayoutDto::Sequential);
    assert!(
        serde_json::from_str::<SetStoryLayoutInputDto>(r#"{"storyId":"s1","layout":"grid"}"#)
            .is_err()
    );
    let gen: GenerateAnnouncementsInputDto =
        serde_json::from_str(r#"{"storyId":"s1","force":true}"#).expect("parse");
    assert!(gen.force);
    let voice: SetAnnouncementVoiceInputDto =
        serde_json::from_str(r#"{"voiceId":"embedded:fr_FR-siwis-medium"}"#).expect("parse");
    assert_eq!(voice.voice_id, "embedded:fr_FR-siwis-medium");
}

#[test]
fn announcement_voices_wire_shape() {
    let dto = AnnouncementVoicesDto {
        voices: vec![
            AnnouncementVoiceDto {
                id: "system:say:Thomas".into(),
                name: "Thomas".into(),
                language: "fr-FR".into(),
                engine: VoiceEngineDto::System,
            },
            AnnouncementVoiceDto {
                id: "embedded:fr_FR-siwis-medium".into(),
                name: "Voix neuronale française (Siwis)".into(),
                language: "fr-FR".into(),
                engine: VoiceEngineDto::Embedded,
            },
        ],
        selected_voice_id: Some("system:say:Thomas".into()),
        selected_is_stored: false,
        embedded: EmbeddedVoiceStatusDto {
            state: EmbeddedVoiceStateDto::Installed,
            version: Some("2023.11.14-2".into()),
            download_bytes: 89_667_631,
            voice_id: "embedded:fr_FR-siwis-medium".into(),
            voice_name: "Voix neuronale française (Siwis)".into(),
        },
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(
        v,
        serde_json::json!({
            "voices": [
                { "id": "system:say:Thomas", "name": "Thomas", "language": "fr-FR", "engine": "system" },
                { "id": "embedded:fr_FR-siwis-medium", "name": "Voix neuronale française (Siwis)", "language": "fr-FR", "engine": "embedded" }
            ],
            "selectedVoiceId": "system:say:Thomas",
            "selectedIsStored": false,
            "embedded": {
                "state": "installed",
                "version": "2023.11.14-2",
                "downloadBytes": 89_667_631,
                "voiceId": "embedded:fr_FR-siwis-medium",
                "voiceName": "Voix neuronale française (Siwis)"
            }
        })
    );
    for (state, wire) in [
        (EmbeddedVoiceStateDto::Unsupported, "unsupported"),
        (EmbeddedVoiceStateDto::NotInstalled, "notInstalled"),
        (EmbeddedVoiceStateDto::Installing, "installing"),
        (EmbeddedVoiceStateDto::Installed, "installed"),
    ] {
        assert_eq!(serde_json::to_value(state).expect("serialize"), wire);
    }
    let preview = VoicePreviewDto {
        data_url: "data:audio/wav;base64,UklGRg==".into(),
        duration_ms: 2_954,
        spoken_text: "Quelle histoire…".into(),
    };
    let v = serde_json::to_value(&preview).expect("serialize");
    assert_eq!(v["dataUrl"], "data:audio/wav;base64,UklGRg==");
    assert_eq!(v["durationMs"], 2_954);
}
