use serde::{Deserialize, Serialize};

/// Input accepted by the `export_story_with_save_dialog` Tauri command.
/// `deny_unknown_fields` fails the deserialization if the UI ever adds a
/// field ahead of the Rust contract, so the boundary stays authoritative.
///
/// `suggested_filename` is the default text pre-filled in the native save
/// dialog (typically `{sanitizedTitle}.rustory`). The frontend never
/// constructs the actual destination path — the dialog returns it, and
/// Rust validates it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportStoryDialogInputDto {
    pub story_id: String,
    pub suggested_filename: String,
}

/// Tagged outcome returned by `export_story_with_save_dialog`.
///
/// A cancelled dialog is NOT an error — the command resolves with
/// `{ kind: "cancelled" }` so the UI can silently return to idle.
/// Errors (file-system denied, story missing, I/O failure, dialog
/// backend failure) cross the boundary as [`AppError`] rejections.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExportStoryDialogOutcomeDto {
    Exported {
        #[serde(rename = "destinationPath")]
        destination_path: String,
        #[serde(rename = "bytesWritten")]
        bytes_written: u64,
        #[serde(rename = "contentChecksum")]
        content_checksum: String,
    },
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn export_story_dialog_input_rejects_missing_fields() {
        serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
            "storyId": "x",
        }))
        .expect_err("must reject missing suggestedFilename");
        serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
            "suggestedFilename": "y.rustory",
        }))
        .expect_err("must reject missing storyId");
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
        // Only the discriminant is present — no destination, no bytes.
        assert_eq!(v.as_object().expect("object").len(), 1);
    }
}
