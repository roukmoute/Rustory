use serde::{Deserialize, Serialize};

use crate::application::device::import::ImportedDeviceStory;
use crate::ipc::dto::StoryCardDto;

/// Input accepted by the `import_device_story` Tauri command.
/// `deny_unknown_fields` fails the deserialization if the UI ever adds a
/// field ahead of the Rust contract, so the boundary stays authoritative.
///
/// The frontend supplies exactly the two identifiers it legitimately
/// holds: the opaque hashed `device_identifier` from detection and the
/// canonical `pack_uuid` from the inventory. No path, no short id —
/// Rust re-resolves everything else itself.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportDeviceStoryInputDto {
    pub device_identifier: String,
    pub pack_uuid: String,
}

/// Outcome returned by a successful `import_device_story`. Mirror of
/// `src/shared/ipc-contracts/device-import.ts`; drift is enforced by the
/// contract tests AND the runtime guard `isImportDeviceStoryOutcome`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDeviceStoryOutcomeDto {
    pub story: StoryCardDto,
    pub pack_short_id: String,
    pub imported_at: String,
}

impl ImportDeviceStoryOutcomeDto {
    pub fn from_outcome(outcome: ImportedDeviceStory) -> Self {
        Self {
            story: outcome.story,
            pack_short_id: outcome.pack_short_id,
            imported_at: outcome.imported_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_accepts_canonical_camel_case_payload() {
        let dto: ImportDeviceStoryInputDto = serde_json::from_value(serde_json::json!({
            "deviceIdentifier": "0123456789abcdef0123456789abcdef",
            "packUuid": "abababab-abab-abab-abab-ababfac5562d",
        }))
        .expect("deser");
        assert_eq!(dto.device_identifier, "0123456789abcdef0123456789abcdef");
        assert_eq!(dto.pack_uuid, "abababab-abab-abab-abab-ababfac5562d");
    }

    #[test]
    fn input_rejects_snake_case_field() {
        let err = serde_json::from_value::<ImportDeviceStoryInputDto>(serde_json::json!({
            "device_identifier": "x",
            "packUuid": "y",
        }))
        .expect_err("must reject snake_case");
        let message = err.to_string().to_lowercase();
        assert!(
            message.contains("device_identifier") || message.contains("unknown field"),
            "expected snake_case rejection, got: {message}"
        );
    }

    #[test]
    fn input_rejects_unknown_field() {
        let err = serde_json::from_value::<ImportDeviceStoryInputDto>(serde_json::json!({
            "deviceIdentifier": "x",
            "packUuid": "y",
            "mountPath": "/sneaky",
        }))
        .expect_err("must reject unknown field — no path crosses IPC");
        assert!(err.to_string().contains("mountPath"));
    }

    #[test]
    fn input_rejects_missing_fields() {
        serde_json::from_value::<ImportDeviceStoryInputDto>(serde_json::json!({
            "deviceIdentifier": "x",
        }))
        .expect_err("must reject missing packUuid");
        serde_json::from_value::<ImportDeviceStoryInputDto>(serde_json::json!({
            "packUuid": "y",
        }))
        .expect_err("must reject missing deviceIdentifier");
    }

    #[test]
    fn outcome_serializes_in_camel_case() {
        let dto = ImportDeviceStoryOutcomeDto::from_outcome(ImportedDeviceStory {
            story: StoryCardDto::native(
                "0197a5d0-0000-7000-8000-000000000000".into(),
                "Histoire de ma Lunii (FAC5562D)".into(),
            ),
            pack_short_id: "FAC5562D".into(),
            imported_at: "2026-06-10T00:00:00.000Z".into(),
            pack_file_count: 5,
            pack_total_bytes: 18,
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["story"]["id"], "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(v["story"]["title"], "Histoire de ma Lunii (FAC5562D)");
        assert_eq!(v["packShortId"], "FAC5562D");
        assert_eq!(v["importedAt"], "2026-06-10T00:00:00.000Z");
        for snake in ["pack_short_id", "imported_at"] {
            assert!(v.get(snake).is_none(), "{snake} must be camelCase");
        }
        // The diagnostic-only counts never cross the IPC boundary.
        assert!(v.get("packFileCount").is_none());
        assert!(v.get("packTotalBytes").is_none());
    }
}
