use rustory_lib::domain::import::{FolderMediaKind, MediaProbe, StructuredFolderAnalysis};
use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::import_export::{
    archive_import_findings_from_summary, folder_import_findings_from_summary,
    import_findings_from_summary, rss_import_findings_from_summary,
};
use rustory_lib::ipc::dto::{
    AcceptArtifactImportInputDto, AcceptStructuredCreationInputDto, ArchiveCreationAnalysisDto,
    DropAnalysisDto, ExportStoryArchiveOutcomeDto, ExportStoryDialogInputDto,
    ExportStoryDialogOutcomeDto, ImportArtifactAnalysisDto, ImportAspectDto, ImportCategoryDto,
    ImportFindingDto, ImportQualityDto, ImportStateDto, ImportableContentDto, OsOpenAnalysisDto,
    StructuredCreationAnalysisDto,
};

#[test]
fn export_story_dialog_input_accepts_canonical_camel_case_payload() {
    let dto: ExportStoryDialogInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "0197a5d0-0000-7000-8000-000000000000",
        "suggestedFilename": "Mon histoire.rustory",
    }))
    .expect("deser");
    assert_eq!(dto.story_id, "0197a5d0-0000-7000-8000-000000000000");
    assert_eq!(dto.suggested_filename, "Mon histoire.rustory");
}

#[test]
fn export_story_dialog_input_rejects_snake_case_story_id() {
    let err = serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
        "story_id": "x",
        "suggestedFilename": "y.rustory",
    }))
    .expect_err("must reject snake_case");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("story_id") || message.contains("unknown field"),
        "expected snake_case or unknown-field rejection, got: {message}"
    );
}

#[test]
fn export_story_dialog_input_rejects_snake_case_suggested_filename() {
    let err = serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
        "storyId": "x",
        "suggested_filename": "y.rustory",
    }))
    .expect_err("must reject snake_case");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("suggested_filename") || message.contains("unknown field"),
        "expected snake_case or unknown-field rejection, got: {message}"
    );
}

#[test]
fn export_story_dialog_input_rejects_unknown_field() {
    let err = serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
        "storyId": "x",
        "suggestedFilename": "y.rustory",
        "extra": "z",
    }))
    .expect_err("must reject unknown field");
    assert!(err.to_string().contains("extra"));
}

#[test]
fn export_story_dialog_input_rejects_missing_story_id() {
    serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
        "suggestedFilename": "y.rustory",
    }))
    .expect_err("must reject missing storyId");
}

#[test]
fn export_story_dialog_input_rejects_missing_suggested_filename() {
    serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
        "storyId": "x",
    }))
    .expect_err("must reject missing suggestedFilename");
}

#[test]
fn exported_outcome_wire_shape_is_tagged_camel_case() {
    let dto = ExportStoryDialogOutcomeDto::Exported {
        destination_path: "/tmp/histoire.rustory".into(),
        bytes_written: 451,
        content_checksum: "a".repeat(64),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["kind"], "exported");
    assert_eq!(v["destinationPath"], "/tmp/histoire.rustory");
    assert_eq!(v["bytesWritten"], 451);
    assert_eq!(v["contentChecksum"].as_str().unwrap().len(), 64);
    for snake in ["destination_path", "bytes_written", "content_checksum"] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn cancelled_outcome_wire_shape_carries_only_kind() {
    let dto = ExportStoryDialogOutcomeDto::Cancelled;
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["kind"], "cancelled");
    assert_eq!(v.as_object().expect("object").len(), 1);
}

// ===== Local artifact import =====

// ===== Story archive export (library story → shareable `.zip` pack) =====

#[test]
fn export_story_archive_takes_the_same_input_as_the_artifact_export() {
    // ONE input shape for both exports of the registry: the story and the
    // file name the save dialog suggests — never a destination path.
    let dto: ExportStoryDialogInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "0197a5d0-0000-7000-8000-000000000000",
        "suggestedFilename": "Mon histoire.zip",
    }))
    .expect("deser");
    assert_eq!(dto.suggested_filename, "Mon histoire.zip");
}

#[test]
fn exported_archive_outcome_wire_shape_is_tagged_camel_case() {
    let dto = ExportStoryArchiveOutcomeDto::Exported {
        destination_path: "/home/moi/Mon histoire.zip".into(),
        bytes_written: 12_345_678,
        image_count: 1,
        audio_count: 2,
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "exported",
            "destinationPath": "/home/moi/Mon histoire.zip",
            "bytesWritten": 12_345_678,
            "imageCount": 1,
            "audioCount": 2,
        })
    );
}

#[test]
fn cancelled_archive_outcome_wire_shape_carries_only_kind() {
    let v = serde_json::to_value(ExportStoryArchiveOutcomeDto::Cancelled).expect("ser");
    assert_eq!(v, serde_json::json!({ "kind": "cancelled" }));
}

#[test]
fn app_error_wire_shape_for_export_failed() {
    let err = AppError::export_failed(
        "Export impossible: un ou plusieurs épisodes de cette histoire n'ont pas d'audio.",
        "Ajoute un audio à chaque épisode, puis réessaie l'export.",
    )
    .with_details(serde_json::json!({
        "source": "story_pack",
        "cause": "missing_audio",
    }));
    let v = serde_json::to_value(&err).expect("serialize");
    assert_eq!(v["code"], "EXPORT_FAILED");
    assert_eq!(
        v["message"],
        "Export impossible: un ou plusieurs épisodes de cette histoire n'ont pas d'audio."
    );
    assert_eq!(
        v["userAction"],
        "Ajoute un audio à chaque épisode, puis réessaie l'export."
    );
    assert_eq!(v["details"]["source"], "story_pack");
    assert_eq!(v["details"]["cause"], "missing_audio");
    assert!(
        v.get("user_action").is_none(),
        "snake_case must never leak across the boundary"
    );
}

fn importable_content_json() -> serde_json::Value {
    serde_json::json!({
        "title": "Le Soleil",
        "structureJson": "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}",
        "contentChecksum": "a".repeat(64),
        "createdAt": "2026-06-20T10:00:00.000Z",
        "updatedAt": "2026-06-24T14:15:00.000Z",
    })
}

#[test]
fn accept_artifact_import_input_accepts_canonical_camel_case_payload() {
    let dto: AcceptArtifactImportInputDto = serde_json::from_value(serde_json::json!({
        "content": importable_content_json(),
        "sourceName": "histoire.rustory",
        "artifactChecksum": "b".repeat(64),
    }))
    .expect("deser");
    assert_eq!(dto.content.title, "Le Soleil");
    assert_eq!(dto.source_name, "histoire.rustory");
}

#[test]
fn accept_artifact_import_input_rejects_snake_case_source_name() {
    let err = serde_json::from_value::<AcceptArtifactImportInputDto>(serde_json::json!({
        "content": importable_content_json(),
        "source_name": "x.rustory",
        "artifactChecksum": "b".repeat(64),
    }))
    .expect_err("snake_case must be refused");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("source_name")
            || message.contains("unknown field")
            || message.contains("missing field")
    );
}

#[test]
fn accept_artifact_import_input_rejects_unknown_field() {
    let err = serde_json::from_value::<AcceptArtifactImportInputDto>(serde_json::json!({
        "content": importable_content_json(),
        "sourceName": "x.rustory",
        "artifactChecksum": "b".repeat(64),
        "extra": "z",
    }))
    .expect_err("unknown field must be refused");
    assert!(err.to_string().contains("extra"));
}

#[test]
fn importable_content_wire_carries_no_schema_version() {
    // `schemaVersion` is NOT part of the wire content — the accept phase
    // re-proves the canonical version against the embedded `structureJson`.
    let err = serde_json::from_value::<ImportableContentDto>(serde_json::json!({
        "title": "Le Soleil",
        "structureJson": "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}",
        "contentChecksum": "a".repeat(64),
        "createdAt": "2026-06-20T10:00:00.000Z",
        "updatedAt": "2026-06-24T14:15:00.000Z",
        "schemaVersion": 1,
    }))
    .expect_err("schemaVersion must be refused as unknown");
    assert!(err.to_string().contains("schemaVersion") || err.to_string().contains("unknown"));
}

#[test]
fn analyzed_verdict_round_trips_the_documented_wire_shape() {
    let dto = ImportArtifactAnalysisDto::Analyzed {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::NeedsReview,
        findings: vec![ImportFindingDto {
            aspect: ImportAspectDto::Title,
            category: ImportCategoryDto::Ambiguous,
            message: "Le titre a été normalisé à l'import (espaces ou caractères ajustés).".into(),
        }],
        importable_content: Some(ImportableContentDto {
            title: "  Le Soleil  ".into(),
            structure_json: "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}".into(),
            content_checksum: "a".repeat(64),
            created_at: "2026-06-20T10:00:00.000Z".into(),
            updated_at: "2026-06-24T14:15:00.000Z".into(),
        }),
        source_name: "histoire.rustory".into(),
        artifact_checksum: "b".repeat(64),
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "analyzed");
    assert_eq!(v["quality"], "partial");
    assert_eq!(v["state"], "needsReview");
    assert_eq!(v["findings"][0]["aspect"], "title");
    assert_eq!(v["importableContent"]["title"], "  Le Soleil  ");
    assert_eq!(v["sourceName"], "histoire.rustory");
    for snake in ["source_name", "artifact_checksum", "importable_content"] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn analysis_cancelled_wire_shape_carries_only_kind() {
    let v = serde_json::to_value(ImportArtifactAnalysisDto::Cancelled).expect("ser");
    assert_eq!(v, serde_json::json!({ "kind": "cancelled" }));
}

#[test]
fn app_error_wire_shape_for_import_failed_file_read() {
    let err = AppError::import_failed(
        "Import impossible: fichier illisible.",
        "Vérifie que le fichier existe puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "file_read", "stage": "read" }));
    let v = serde_json::to_value(&err).expect("serialize");
    assert_eq!(v["code"], "IMPORT_FAILED");
    assert_eq!(v["details"]["source"], "file_read");
    assert_eq!(v["details"]["stage"], "read");
    assert!(
        v.get("user_action").is_none(),
        "snake_case must never leak across the boundary"
    );
}

#[test]
fn app_error_wire_shape_for_export_destination_unavailable() {
    let err = AppError::export_destination_unavailable(
        "Écriture refusée par le système pour ce dossier.",
        "Choisis un dossier où tu as les droits en écriture.",
    )
    .with_details(serde_json::json!({
        "source": "temp_create",
        "kind": "permission_denied",
    }));
    let v = serde_json::to_value(&err).expect("serialize");
    assert_eq!(v["code"], "EXPORT_DESTINATION_UNAVAILABLE");
    assert_eq!(
        v["message"],
        "Écriture refusée par le système pour ce dossier."
    );
    assert_eq!(
        v["userAction"],
        "Choisis un dossier où tu as les droits en écriture."
    );
    assert_eq!(v["details"]["source"], "temp_create");
    assert_eq!(v["details"]["kind"], "permission_denied");
    assert!(
        v.get("user_action").is_none(),
        "snake_case must never leak across the boundary"
    );
}

// ===== Structured-folder creation (folder → new canonical story) =====

/// Analyze a manifest through the REAL domain pipeline so the contract test
/// exercises the exact wire the command emits.
fn folder_analysis(manifest: &str) -> rustory_lib::domain::import::StructuredFolderAnalysis {
    rustory_lib::domain::import::analyze_structured_folder_components(
        manifest.as_bytes(),
        &std::collections::BTreeMap::new(),
    )
}

#[test]
fn structured_creation_analyzed_wire_shape_is_tagged_camel_case() {
    let analysis = folder_analysis(
        r#"{ "formatVersion": 1, "title": "Le voyage", "nodes": [ { "id": "n1" } ] }"#,
    );
    let dto = StructuredCreationAnalysisDto::analyzed(
        &analysis,
        "mon-dossier".into(),
        "/home/user/mon-dossier".into(),
    );
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["kind"], "analyzed");
    assert_eq!(v["quality"], "clean");
    assert_eq!(v["state"], "recognized");
    assert_eq!(v["folderName"], "mon-dossier");
    // The folderPath is REQUIRED on an analyzed verdict (the accept phase
    // needs it back) — and it is the ONLY place the absolute path exists.
    assert_eq!(v["folderPath"], "/home/user/mon-dossier");
    assert_eq!(v["creatableSummary"]["title"], "Le voyage");
    assert_eq!(v["creatableSummary"]["nodeCount"], 1);
    assert!(v["creatableSummary"]["retainedMedia"]
        .as_array()
        .expect("array")
        .is_empty());
    let aspects: Vec<&str> = v["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|finding| finding["aspect"].as_str().expect("aspect"))
        .collect();
    assert_eq!(
        aspects,
        [
            "envelope",
            "formatVersion",
            "title",
            "structure",
            "media",
            "mediaSize"
        ],
        "exactly one finding per folder aspect, in this order"
    );
    for snake in [
        "folder_name",
        "folder_path",
        "creatable_summary",
        "node_count",
        "retained_media",
        "discarded_media",
    ] {
        assert!(
            v.get(snake).is_none() && v["creatableSummary"].get(snake).is_none(),
            "{snake} must be camelCase"
        );
    }
}

#[test]
fn structured_creation_blocked_verdict_omits_the_creatable_summary() {
    let analysis =
        folder_analysis(r#"{ "formatVersion": 2, "title": "Futur", "nodes": [ { "id": "n1" } ] }"#);
    let dto = StructuredCreationAnalysisDto::analyzed(
        &analysis,
        "dossier-futur".into(),
        "/tmp/dossier-futur".into(),
    );
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["quality"], "unusable");
    assert_eq!(v["state"], "blocked");
    assert!(
        v.get("creatableSummary").is_none(),
        "a blocked verdict carries nothing to create"
    );
}

#[test]
fn structured_creation_media_findings_use_the_folder_copy() {
    // A media pair on the wire: the message is the FOLDER copy (manifest /
    // creation wording), and the `media` aspect serializes camelCase.
    let manifest = r#"{ "formatVersion": 1, "title": "Média", "nodes": [ { "id": "n1", "image": "absente.png" } ] }"#;
    let analysis = rustory_lib::domain::import::analyze_structured_folder_components(
        manifest.as_bytes(),
        &[(
            "absente.png".to_string(),
            rustory_lib::domain::import::MediaProbe::Absent,
        )]
        .into_iter()
        .collect(),
    );
    let dto = StructuredCreationAnalysisDto::analyzed(&analysis, "d".into(), "/tmp/d".into());
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["state"], "partial");
    let findings = v["findings"].as_array().expect("findings");
    let media = findings
        .iter()
        .find(|f| f["aspect"] == "media")
        .expect("a media finding");
    assert_eq!(media["category"], "missing");
    assert!(media["message"]
        .as_str()
        .expect("message")
        .contains("introuvables"));
    assert_eq!(
        v["creatableSummary"]["discardedMedia"],
        serde_json::json!(["absente.png"])
    );
}

#[test]
fn structured_creation_cancelled_wire_shape_carries_only_kind() {
    let v = serde_json::to_value(StructuredCreationAnalysisDto::Cancelled).expect("ser");
    assert_eq!(v, serde_json::json!({ "kind": "cancelled" }));
}

#[test]
fn accept_structured_creation_input_accepts_canonical_camel_case_payload() {
    let dto: AcceptStructuredCreationInputDto = serde_json::from_value(serde_json::json!({
        "folderPath": "/home/user/mon-dossier",
    }))
    .expect("deser");
    assert_eq!(dto.folder_path, "/home/user/mon-dossier");
}

#[test]
fn accept_structured_creation_input_rejects_snake_case_and_unknown_field() {
    let snake = serde_json::from_value::<AcceptStructuredCreationInputDto>(serde_json::json!({
        "folder_path": "/tmp/d",
    }));
    assert!(snake.is_err(), "snake_case folder_path must be refused");

    let unknown = serde_json::from_value::<AcceptStructuredCreationInputDto>(serde_json::json!({
        "folderPath": "/tmp/d",
        "extra": true,
    }));
    assert!(unknown.is_err(), "unknown field must be refused");

    let missing = serde_json::from_value::<AcceptStructuredCreationInputDto>(serde_json::json!({}));
    assert!(missing.is_err(), "missing folderPath must be refused");
}

// ===== Media size (structured folder + structured archive) =====
//
// The size of the media is an aspect of its own on the wire: `mediaSize`.
// Its message is Rust-authoritative and names the bound of the flow that
// emits it, in decimal units.

/// Analyze a folder manifest against the given probes, through the REAL
/// domain pipeline.
fn folder_analysis_with(manifest: &str, probes: &[(&str, MediaProbe)]) -> StructuredFolderAnalysis {
    rustory_lib::domain::import::analyze_structured_folder_components(
        manifest.as_bytes(),
        &probes
            .iter()
            .map(|(name, probe)| (name.to_string(), *probe))
            .collect(),
    )
}

/// Analyze a pack descriptor against the given probes, through the REAL
/// domain pipeline.
fn archive_analysis_with(
    descriptor: &str,
    probes: &[(&str, MediaProbe)],
) -> StructuredFolderAnalysis {
    rustory_lib::domain::import::analyze_structured_archive_components(
        Some(descriptor.as_bytes()),
        &probes
            .iter()
            .map(|(name, probe)| (name.to_string(), *probe))
            .collect(),
        None,
    )
}

const ONE_SOUND_MANIFEST: &str = r#"{ "formatVersion": 1, "title": "Un son", "nodes": [ { "id": "n1", "audio": "long.wav" } ] }"#;

const ONE_SOUND_PACK: &str = r#"{
    "title": "Un son",
    "stageNodes": [
        { "uuid": "s1", "squareOne": true, "image": null, "audio": "long.mp3",
          "okTransition": null, "controlSettings": {} }
    ],
    "actionNodes": []
}"#;

const OVERSIZE_SOUND: MediaProbe = MediaProbe::Oversize {
    kind: Some(FolderMediaKind::Audio),
};

fn finding_of<'a>(wire: &'a serde_json::Value, aspect: &str) -> &'a serde_json::Value {
    wire["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["aspect"] == aspect)
        .unwrap_or_else(|| panic!("a `{aspect}` finding"))
}

#[test]
fn archive_creation_analyzed_wire_shape_carries_one_finding_per_archive_aspect() {
    let analysis = archive_analysis_with(ONE_SOUND_PACK, &[]);
    let dto = ArchiveCreationAnalysisDto::analyzed(
        &analysis,
        "Un son.zip".into(),
        "/home/user/Un son.zip".into(),
    );
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["kind"], "analyzed");
    assert_eq!(v["archiveName"], "Un son.zip");
    assert_eq!(v["archivePath"], "/home/user/Un son.zip");
    let aspects: Vec<&str> = v["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|finding| finding["aspect"].as_str().expect("aspect"))
        .collect();
    assert_eq!(
        aspects,
        ["envelope", "title", "structure", "media", "mediaSize"],
        "the pack format declares no format version — never asserted"
    );
    for snake in [
        "archive_name",
        "archive_path",
        "creatable_summary",
        "media_size",
    ] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn an_archive_media_over_the_per_media_bound_is_named_for_its_size_on_the_wire() {
    let analysis = archive_analysis_with(ONE_SOUND_PACK, &[("long.mp3", OVERSIZE_SOUND)]);
    let dto = ArchiveCreationAnalysisDto::analyzed(&analysis, "p.zip".into(), "/tmp/p.zip".into());
    let v = serde_json::to_value(&dto).expect("serialize");
    // Left out, never blocking: the story is created without it.
    assert_eq!(v["quality"], "partial");
    assert_eq!(v["state"], "needsReview");
    assert_eq!(
        finding_of(&v, "mediaSize"),
        &serde_json::json!({
            "aspect": "mediaSize",
            "category": "ambiguous",
            "message": "Certains fichiers audio ou image référencés dépassent la taille autorisée (134 Mo par fichier). L'histoire sera créée sans eux ; pour les garder, remplace-les dans l'archive par des fichiers plus légers puis relance l'analyse.",
        })
    );
    // The media itself is neither missing nor of an unknown format.
    assert_eq!(finding_of(&v, "media")["category"], "recognized");
    assert_eq!(
        v["creatableSummary"]["discardedMedia"],
        serde_json::json!(["long.mp3"])
    );
}

#[test]
fn archive_media_over_the_aggregate_bound_block_on_their_size_on_the_wire() {
    // Sounds within the per-media bound whose sum crosses the 32 GiB bound.
    let per_media: u64 = 128 * 1024 * 1024;
    let count = 257;
    let stages: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            serde_json::json!({
                "uuid": format!("s{i}"), "squareOne": i == 0, "image": null,
                "audio": format!("son-{i}.mp3"), "okTransition": null, "controlSettings": {}
            })
        })
        .collect();
    let descriptor =
        serde_json::json!({ "title": "Collection", "stageNodes": stages, "actionNodes": [] })
            .to_string();
    let names: Vec<String> = (0..count).map(|i| format!("son-{i}.mp3")).collect();
    let probes: Vec<(&str, MediaProbe)> = names
        .iter()
        .map(|name| {
            (
                name.as_str(),
                MediaProbe::Usable {
                    kind: FolderMediaKind::Audio,
                    byte_size: per_media,
                },
            )
        })
        .collect();
    let analysis = archive_analysis_with(&descriptor, &probes);
    let dto = ArchiveCreationAnalysisDto::analyzed(&analysis, "c.zip".into(), "/tmp/c.zip".into());
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["quality"], "unusable");
    assert_eq!(v["state"], "blocked");
    assert!(v.get("creatableSummary").is_none());
    assert_eq!(
        finding_of(&v, "mediaSize"),
        &serde_json::json!({
            "aspect": "mediaSize",
            "category": "blocking",
            "message": "Les fichiers audio et image référencés dépassent ensemble la taille autorisée (34,4 Go). Allège l'archive (moins de médias ou des fichiers plus légers) puis relance l'analyse.",
        })
    );
    // The size is the stated reason — the structure is not blamed for it.
    assert_eq!(finding_of(&v, "structure")["category"], "recognized");
}

#[test]
fn a_folder_media_over_its_bounds_is_named_for_its_size_on_the_wire() {
    // Per media: 32 MiB, spelled in decimal units.
    let analysis = folder_analysis_with(ONE_SOUND_MANIFEST, &[("long.wav", OVERSIZE_SOUND)]);
    let dto = StructuredCreationAnalysisDto::analyzed(&analysis, "d".into(), "/tmp/d".into());
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["state"], "needsReview");
    assert_eq!(
        finding_of(&v, "mediaSize"),
        &serde_json::json!({
            "aspect": "mediaSize",
            "category": "ambiguous",
            "message": "Certains fichiers audio ou image référencés dépassent la taille autorisée (34 Mo par fichier). L'histoire sera créée sans eux ; tu pourras ajouter des fichiers plus légers dans l'éditeur.",
        })
    );

    // Together: 256 MiB — nine sounds of exactly the per-media bound cross it.
    let names: Vec<String> = (0..9).map(|i| format!("son-{i}.mp3")).collect();
    let nodes: Vec<serde_json::Value> = names
        .iter()
        .enumerate()
        .map(|(i, name)| serde_json::json!({ "id": format!("n{i}"), "audio": name }))
        .collect();
    let manifest =
        serde_json::json!({ "formatVersion": 1, "title": "Total", "nodes": nodes }).to_string();
    let probes: Vec<(&str, MediaProbe)> = names
        .iter()
        .map(|name| {
            (
                name.as_str(),
                MediaProbe::Usable {
                    kind: FolderMediaKind::Audio,
                    byte_size: 32 * 1024 * 1024,
                },
            )
        })
        .collect();
    let analysis = folder_analysis_with(&manifest, &probes);
    let dto = StructuredCreationAnalysisDto::analyzed(&analysis, "d".into(), "/tmp/d".into());
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["state"], "blocked");
    assert_eq!(
        finding_of(&v, "mediaSize"),
        &serde_json::json!({
            "aspect": "mediaSize",
            "category": "blocking",
            "message": "Les fichiers audio et image référencés dépassent ensemble la taille autorisée (268 Mo). Allège le dossier (moins de médias ou des fichiers plus légers) puis relance l'analyse.",
        })
    );
}

#[test]
fn the_drop_and_os_open_channels_carry_the_media_size_finding_unchanged() {
    // The two channels re-tag the picker verdicts: same findings, same copy.
    let analysis = archive_analysis_with(ONE_SOUND_PACK, &[("long.mp3", OVERSIZE_SOUND)]);
    let picker = serde_json::to_value(ArchiveCreationAnalysisDto::analyzed(
        &analysis,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    let dropped = serde_json::to_value(DropAnalysisDto::archive(
        &analysis,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    let opened = serde_json::to_value(OsOpenAnalysisDto::archive(
        &analysis,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    assert_eq!(dropped["kind"], "archive");
    assert_eq!(opened["kind"], "archive");
    assert_eq!(dropped["findings"], picker["findings"]);
    assert_eq!(opened["findings"], picker["findings"]);

    let folder = folder_analysis_with(ONE_SOUND_MANIFEST, &[("long.wav", OVERSIZE_SOUND)]);
    let picker = serde_json::to_value(StructuredCreationAnalysisDto::analyzed(
        &folder,
        "d".into(),
        "/tmp/d".into(),
    ))
    .expect("serialize");
    let dropped = serde_json::to_value(DropAnalysisDto::folder(
        &folder,
        "d".into(),
        "/tmp/d".into(),
    ))
    .expect("serialize");
    assert_eq!(dropped["kind"], "folder");
    assert_eq!(dropped["findings"], picker["findings"]);
}

#[test]
fn the_media_size_aspect_never_reaches_the_flows_that_judge_no_media() {
    // A `.rustory` artifact embeds no media: its verdict never carries the
    // aspect (its wire is untouched by it).
    let dto = ImportArtifactAnalysisDto::Analyzed {
        quality: ImportQualityDto::Clean,
        state: ImportStateDto::Recognized,
        findings: vec![ImportFindingDto {
            aspect: ImportAspectDto::Envelope,
            category: ImportCategoryDto::Recognized,
            message: "L'enveloppe de l'artefact est valide.".into(),
        }],
        importable_content: None,
        source_name: "histoire.rustory".into(),
        artifact_checksum: "b".repeat(64),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert!(!v.to_string().contains("mediaSize"));
    // The wire tag itself is camelCase, like every aspect.
    assert_eq!(
        serde_json::to_value(ImportAspectDto::MediaSize).expect("serialize"),
        "mediaSize"
    );
}

// ===== Media presence (structured archive) =====
//
// An archive is not a folder: the `media` findings of its review, of the
// card returned at creation and of its durable report speak of a pack and
// of its archive — one Rust-authoritative copy, on every wire.

const ARCHIVE_MEDIA_RECOGNIZED: &str = "Tous les fichiers audio et image référencés par le pack sont présents dans l'archive et reconnus.";

const ARCHIVE_MEDIA_MISSING: &str = "Certains fichiers audio ou image référencés par le pack sont introuvables dans l'archive. L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur.";

#[test]
fn structured_archive_media_findings_use_the_archive_copy() {
    // Every referenced media is in the archive…
    let present = archive_analysis_with(
        ONE_SOUND_PACK,
        &[(
            "long.mp3",
            MediaProbe::Usable {
                kind: FolderMediaKind::Audio,
                byte_size: 4096,
            },
        )],
    );
    let clean = serde_json::to_value(ArchiveCreationAnalysisDto::analyzed(
        &present,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    assert_eq!(clean["quality"], "clean");
    assert_eq!(
        finding_of(&clean, "media"),
        &serde_json::json!({
            "aspect": "media",
            "category": "recognized",
            "message": ARCHIVE_MEDIA_RECOGNIZED,
        })
    );

    // …or one is not: the story is created without it.
    let absent = archive_analysis_with(ONE_SOUND_PACK, &[("long.mp3", MediaProbe::Absent)]);
    let picker = serde_json::to_value(ArchiveCreationAnalysisDto::analyzed(
        &absent,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    assert_eq!(picker["quality"], "partial");
    assert_eq!(picker["state"], "partial");
    assert_eq!(
        finding_of(&picker, "media"),
        &serde_json::json!({
            "aspect": "media",
            "category": "missing",
            "message": ARCHIVE_MEDIA_MISSING,
        })
    );
    assert_eq!(
        picker["creatableSummary"]["discardedMedia"],
        serde_json::json!(["long.mp3"])
    );

    // The drop and OS-open channels re-tag the picker verdict: same copy.
    let dropped = serde_json::to_value(DropAnalysisDto::archive(
        &absent,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    let opened = serde_json::to_value(OsOpenAnalysisDto::archive(
        &absent,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    assert_eq!(dropped["findings"], picker["findings"]);
    assert_eq!(opened["findings"], picker["findings"]);

    // No line of an archive review speaks of a folder.
    for wire in [&clean, &picker] {
        for finding in wire["findings"].as_array().expect("findings") {
            let message = finding["message"].as_str().expect("message");
            assert!(!message.contains("dossier"), "{message}");
        }
    }
}

#[test]
fn an_archive_story_report_words_its_media_with_the_archive_copy() {
    // The durable card report re-words the stored codes with the copy of the
    // story's own flow: an archive story never reads the folder's.
    let summary = r#"[{"aspect":"envelope","category":"recognized"},{"aspect":"title","category":"recognized"},{"aspect":"structure","category":"recognized"},{"aspect":"media","category":"missing"},{"aspect":"mediaSize","category":"recognized"}]"#;
    let media_of = |report: Vec<ImportFindingDto>| {
        report
            .into_iter()
            .find(|finding| finding.aspect == ImportAspectDto::Media)
            .expect("the media finding")
            .message
    };
    assert_eq!(
        media_of(archive_import_findings_from_summary(summary)),
        ARCHIVE_MEDIA_MISSING
    );
    assert_eq!(
        media_of(folder_import_findings_from_summary(summary)),
        "Certains fichiers audio ou image référencés par le dossier sont introuvables. L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur."
    );
}

// ===== Media count (structured folder + structured archive) =====
//
// How many media a source references is an aspect of its own on the wire:
// `mediaCount`. It only ever BLOCKS — within the bound it has no line —, its
// message is Rust-authoritative and names the bound of the flow that emits
// it, and it never stands next to `media` or `mediaSize`: beyond the bound
// no media is read.

/// A folder manifest of `count` nodes, each with its OWN picture, declaring
/// `format_version`.
fn manifest_of_pictures_in_format(format_version: u64, count: usize) -> String {
    let nodes: Vec<serde_json::Value> = (0..count)
        .map(|i| serde_json::json!({ "id": format!("n{i}"), "image": format!("image-{i}.png") }))
        .collect();
    serde_json::json!({
        "formatVersion": format_version,
        "title": "Longue histoire",
        "nodes": nodes,
    })
    .to_string()
}

/// The same manifest in the LISTED format.
fn manifest_of_pictures(count: usize) -> String {
    manifest_of_pictures_in_format(1, count)
}

/// A pack descriptor of `count` stages, each showing its OWN picture.
fn pack_of_pictures(count: usize) -> String {
    let stages: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            serde_json::json!({
                "uuid": format!("s{i}"), "squareOne": i == 0,
                "image": format!("image-{i}.png"), "audio": null,
                "okTransition": null, "controlSettings": {}
            })
        })
        .collect();
    serde_json::json!({ "title": "Pack sans fin", "stageNodes": stages, "actionNodes": [] })
        .to_string()
}

fn aspects_of(wire: &serde_json::Value) -> Vec<&str> {
    wire["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|finding| finding["aspect"].as_str().expect("aspect"))
        .collect()
}

const FOLDER_COUNT_NAMED: &str = "Les fichiers audio et image référencés dépassent le nombre autorisé (64 fichiers différents). Réduis le nombre de médias référencés par le manifest puis relance l'analyse ; tu pourras ajouter les autres dans l'éditeur.";

const ARCHIVE_COUNT_NAMED: &str = "Les fichiers audio et image référencés dépassent le nombre autorisé (8192 fichiers différents). Réduis le nombre de médias de l'archive (par exemple en répartissant le pack sur plusieurs archives) puis relance l'analyse.";

#[test]
fn a_folder_referencing_65_media_blocks_on_their_number_and_names_the_limit_of_64_on_the_wire() {
    let analysis = folder_analysis(&manifest_of_pictures(65));
    let dto = StructuredCreationAnalysisDto::analyzed(
        &analysis,
        "longue-histoire".into(),
        "/home/user/longue-histoire".into(),
    );
    let v = serde_json::to_value(&dto).expect("serialize");
    // The WHOLE wire verdict — the frontend contract test feeds this very
    // payload to its guard. No media was read: neither `media` nor
    // `mediaSize` is on the wire; the structure is not blamed for a count
    // reason; a count block creates nothing (no `creatableSummary`).
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "analyzed",
            "quality": "unusable",
            "state": "blocked",
            "findings": [
                {
                    "aspect": "envelope",
                    "category": "recognized",
                    "message": "Le manifest histoire.json est présent et lisible.",
                },
                {
                    "aspect": "formatVersion",
                    "category": "recognized",
                    "message": "La version de format du manifest est prise en charge.",
                },
                {
                    "aspect": "title",
                    "category": "recognized",
                    "message": "Le titre de l'histoire est valide.",
                },
                {
                    "aspect": "structure",
                    "category": "recognized",
                    "message": "La structure de l'histoire est reconnue.",
                },
                {
                    "aspect": "mediaCount",
                    "category": "blocking",
                    "message": FOLDER_COUNT_NAMED,
                },
            ],
            "folderName": "longue-histoire",
            "folderPath": "/home/user/longue-histoire",
        })
    );
    assert!(
        !v.to_string().contains("media_count"),
        "the wire tag is camelCase"
    );
}

#[test]
fn a_folder_referencing_64_media_carries_no_count_line_on_the_wire() {
    // The bound is inclusive, and a bound that is respected is not a line:
    // the creatable verdict keeps exactly its six aspects.
    let names: Vec<String> = (0..64).map(|i| format!("image-{i}.png")).collect();
    let probes: Vec<(&str, MediaProbe)> = names
        .iter()
        .map(|name| {
            (
                name.as_str(),
                MediaProbe::Usable {
                    kind: FolderMediaKind::Image,
                    byte_size: 1024,
                },
            )
        })
        .collect();
    let analysis = folder_analysis_with(&manifest_of_pictures(64), &probes);
    let dto = StructuredCreationAnalysisDto::analyzed(&analysis, "d".into(), "/tmp/d".into());
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["quality"], "clean");
    assert_eq!(
        aspects_of(&v),
        [
            "envelope",
            "formatVersion",
            "title",
            "structure",
            "media",
            "mediaSize"
        ]
    );
    assert_eq!(
        v["creatableSummary"]["retainedMedia"]
            .as_array()
            .expect("retained")
            .len(),
        64
    );
}

#[test]
fn an_archive_referencing_8193_media_blocks_on_their_number_and_names_the_limit_of_8192() {
    let analysis = archive_analysis_with(&pack_of_pictures(8193), &[]);
    let dto = ArchiveCreationAnalysisDto::analyzed(
        &analysis,
        "pack-sans-fin.zip".into(),
        "/home/user/pack-sans-fin.zip".into(),
    );
    let v = serde_json::to_value(&dto).expect("serialize");
    // The WHOLE wire verdict — the frontend contract test feeds this very
    // payload to its guard. No entry was read (neither `media` nor
    // `mediaSize`), the pack format declares no format version, and the
    // structure is not blamed for a count reason.
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "analyzed",
            "quality": "unusable",
            "state": "blocked",
            "findings": [
                {
                    "aspect": "envelope",
                    "category": "recognized",
                    "message": "Le descripteur story.json est présent et lisible.",
                },
                {
                    "aspect": "title",
                    "category": "recognized",
                    "message": "Le titre de l'histoire est valide.",
                },
                {
                    "aspect": "structure",
                    "category": "recognized",
                    "message": "La structure du pack est reconnue et convertie en histoire.",
                },
                {
                    "aspect": "mediaCount",
                    "category": "blocking",
                    "message": ARCHIVE_COUNT_NAMED,
                },
            ],
            "archiveName": "pack-sans-fin.zip",
            "archivePath": "/home/user/pack-sans-fin.zip",
        })
    );

    // One media fewer: the bound is inclusive, the count has no line.
    let analysis = archive_analysis_with(&pack_of_pictures(8192), &[]);
    let dto = ArchiveCreationAnalysisDto::analyzed(&analysis, "p.zip".into(), "/tmp/p.zip".into());
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(
        aspects_of(&v),
        ["envelope", "title", "structure", "media", "mediaSize"]
    );
}

#[test]
fn the_drop_and_os_open_channels_carry_the_media_count_finding_unchanged() {
    // The two channels re-tag the picker verdicts: same findings, same copy.
    let archive = archive_analysis_with(&pack_of_pictures(8193), &[]);
    let picker = serde_json::to_value(ArchiveCreationAnalysisDto::analyzed(
        &archive,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    let dropped = serde_json::to_value(DropAnalysisDto::archive(
        &archive,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    let opened = serde_json::to_value(OsOpenAnalysisDto::archive(
        &archive,
        "p.zip".into(),
        "/tmp/p.zip".into(),
    ))
    .expect("serialize");
    assert_eq!(dropped["kind"], "archive");
    assert_eq!(opened["kind"], "archive");
    assert_eq!(dropped["findings"], picker["findings"]);
    assert_eq!(opened["findings"], picker["findings"]);
    assert_eq!(
        finding_of(&dropped, "mediaCount")["message"],
        ARCHIVE_COUNT_NAMED
    );

    let folder = folder_analysis(&manifest_of_pictures(65));
    let picker = serde_json::to_value(StructuredCreationAnalysisDto::analyzed(
        &folder,
        "d".into(),
        "/tmp/d".into(),
    ))
    .expect("serialize");
    let dropped = serde_json::to_value(DropAnalysisDto::folder(
        &folder,
        "d".into(),
        "/tmp/d".into(),
    ))
    .expect("serialize");
    assert_eq!(dropped["kind"], "folder");
    assert_eq!(dropped["findings"], picker["findings"]);
    assert_eq!(
        finding_of(&dropped, "mediaCount")["message"],
        FOLDER_COUNT_NAMED
    );
}

#[test]
fn the_media_count_aspect_never_reaches_the_flows_that_judge_no_media() {
    // A folder whose format is not the listed one: its references are never
    // evaluated — not even counted.
    let analysis = folder_analysis(&manifest_of_pictures_in_format(2, 65));
    let dto = StructuredCreationAnalysisDto::analyzed(&analysis, "d".into(), "/tmp/d".into());
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(finding_of(&v, "formatVersion")["category"], "blocking");
    assert_eq!(
        aspects_of(&v),
        ["envelope", "formatVersion", "title", "structure"]
    );
    assert!(!v.to_string().contains("mediaCount"));
    // The wire tag itself is camelCase, like every aspect.
    assert_eq!(
        serde_json::to_value(ImportAspectDto::MediaCount).expect("serialize"),
        "mediaCount"
    );
}

// ===== Persisted findings summaries =====

#[test]
fn summaries_stored_before_the_media_count_aspect_render_unchanged() {
    // A blocked verdict is never persisted, so no stored summary carries the
    // aspect: what a creation stored yesterday is exactly what one stores
    // today — the six folder pairs, the five archive pairs — and it renders
    // pair for pair, with nothing invented for the count.
    let folder = r#"[{"aspect":"envelope","category":"recognized"},{"aspect":"formatVersion","category":"recognized"},{"aspect":"title","category":"recognized"},{"aspect":"structure","category":"recognized"},{"aspect":"media","category":"missing"},{"aspect":"mediaSize","category":"ambiguous"}]"#;
    let archive = r#"[{"aspect":"envelope","category":"recognized"},{"aspect":"title","category":"ambiguous"},{"aspect":"structure","category":"recognized"},{"aspect":"media","category":"recognized"},{"aspect":"mediaSize","category":"recognized"}]"#;
    for (report, stored) in [
        (folder_import_findings_from_summary(folder), folder),
        (archive_import_findings_from_summary(archive), archive),
    ] {
        let pairs: Vec<serde_json::Value> = serde_json::from_str(stored).expect("stored pairs");
        assert_eq!(report.len(), pairs.len(), "every stored pair still renders");
        for (finding, pair) in report.iter().zip(&pairs) {
            let wire = serde_json::to_value(finding).expect("serialize");
            assert_eq!(wire["aspect"], pair["aspect"]);
            assert_eq!(wire["category"], pair["category"]);
            assert!(!finding.message.is_empty());
        }
        assert!(report
            .iter()
            .all(|finding| finding.aspect != ImportAspectDto::MediaCount));
    }
    assert_eq!(
        folder_import_findings_from_summary(folder)[5].message,
        "Certains fichiers audio ou image référencés dépassent la taille autorisée (34 Mo par fichier). L'histoire sera créée sans eux ; tu pourras ajouter des fichiers plus légers dans l'éditeur."
    );
    assert_eq!(
        folder_import_findings_from_summary(folder)[3].message,
        "La structure de l'histoire est reconnue."
    );
}

#[test]
fn summaries_persisted_before_the_media_size_aspect_stay_readable() {
    // What a structured-folder creation stored before the size had its own
    // aspect: five `(aspect, category)` pairs, the media ambiguity possibly
    // standing for a media left out for its size.
    let folder = r#"[{"aspect":"envelope","category":"recognized"},{"aspect":"formatVersion","category":"recognized"},{"aspect":"title","category":"recognized"},{"aspect":"structure","category":"recognized"},{"aspect":"media","category":"ambiguous"}]"#;
    // Same for a structured-archive creation: four pairs.
    let archive = r#"[{"aspect":"envelope","category":"recognized"},{"aspect":"title","category":"recognized"},{"aspect":"structure","category":"recognized"},{"aspect":"media","category":"ambiguous"}]"#;
    const AS_WRITTEN: &str = "Certains fichiers audio ou image référencés ne sont pas utilisables (format non reconnu, fichier trop volumineux ou nom invalide). L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur.";

    for (report, pairs) in [
        (folder_import_findings_from_summary(folder), 5),
        (archive_import_findings_from_summary(archive), 4),
    ] {
        assert_eq!(report.len(), pairs, "every stored pair still renders");
        let media = report
            .iter()
            .find(|finding| finding.aspect == ImportAspectDto::Media)
            .expect("the media finding");
        assert_eq!(
            media.message, AS_WRITTEN,
            "the copy it was written under — the size may have been its cause"
        );
        assert!(report
            .iter()
            .all(|finding| finding.aspect != ImportAspectDto::MediaSize));
    }

    // The summaries of the other flows are untouched by the new aspect.
    let rustory = r#"[{"aspect":"envelope","category":"recognized"},{"aspect":"title","category":"ambiguous"}]"#;
    assert_eq!(import_findings_from_summary(rustory).len(), 2);
    let rss = r#"[{"aspect":"envelope","category":"recognized"},{"aspect":"source","category":"ambiguous"},{"aspect":"media","category":"missing"}]"#;
    assert_eq!(rss_import_findings_from_summary(rss).len(), 3);
}

#[test]
fn a_summary_carrying_the_media_size_aspect_names_the_bound_of_its_flow() {
    let summary = r#"[{"aspect":"media","category":"recognized"},{"aspect":"mediaSize","category":"ambiguous"}]"#;
    let size_of = |report: Vec<ImportFindingDto>| {
        report
            .into_iter()
            .find(|finding| finding.aspect == ImportAspectDto::MediaSize)
            .expect("the size finding")
            .message
    };
    assert!(size_of(archive_import_findings_from_summary(summary)).contains("(134 Mo par fichier)"));
    assert!(size_of(folder_import_findings_from_summary(summary)).contains("(34 Mo par fichier)"));
    // A flow with no bound of its own names none.
    assert_eq!(
        size_of(import_findings_from_summary(summary)),
        "Certains fichiers audio ou image référencés dépassent la taille autorisée. L'histoire sera créée sans eux."
    );
}
