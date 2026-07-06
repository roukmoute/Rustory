use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::{
    AcceptArtifactImportInputDto, AcceptStructuredCreationInputDto, ExportStoryDialogInputDto,
    ExportStoryDialogOutcomeDto, ImportArtifactAnalysisDto, ImportAspectDto, ImportCategoryDto,
    ImportFindingDto, ImportQualityDto, ImportStateDto, ImportableContentDto,
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
    assert_eq!(
        v["findings"].as_array().expect("findings").len(),
        5,
        "exactly one finding per folder aspect"
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
