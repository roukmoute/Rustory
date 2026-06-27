use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::{
    AcceptArtifactImportInputDto, ExportStoryDialogInputDto, ExportStoryDialogOutcomeDto,
    ImportArtifactAnalysisDto, ImportAspectDto, ImportCategoryDto, ImportFindingDto,
    ImportQualityDto, ImportStateDto, ImportableContentDto,
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
        "structureJson": "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}",
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
        "structureJson": "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}",
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
            structure_json: "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}".into(),
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
