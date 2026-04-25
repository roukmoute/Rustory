use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::{ExportStoryDialogInputDto, ExportStoryDialogOutcomeDto};

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
