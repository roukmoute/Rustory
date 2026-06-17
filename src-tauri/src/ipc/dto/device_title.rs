use serde::{Deserialize, Serialize};

use crate::domain::device::title::PackTitle;

use super::device_library::PackTitleSourceDto;

/// Input accepted by the `set_device_story_title` Tauri command. The user
/// names a device story that no catalog recognizes (or renames one). The
/// frontend supplies the canonical `pack_uuid` it holds from the inventory
/// and the raw title text; Rust normalizes, validates and persists it.
/// `deny_unknown_fields` keeps the boundary authoritative.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDeviceStoryTitleInputDto {
    pub pack_uuid: String,
    pub title: String,
}

/// Outcome returned by a successful `set_device_story_title`: the stored
/// title and its provenance (always `user`). Mirror of
/// `src/shared/ipc-contracts/device-title.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStoryTitleDto {
    pub title: String,
    pub source: PackTitleSourceDto,
}

impl DeviceStoryTitleDto {
    pub fn from_pack_title(title: PackTitle) -> Self {
        Self {
            title: title.title,
            source: title.source.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::title::PackTitleSource;

    #[test]
    fn input_accepts_canonical_camel_case_payload() {
        let dto: SetDeviceStoryTitleInputDto = serde_json::from_value(serde_json::json!({
            "packUuid": "abababab-abab-abab-abab-ababfac5562d",
            "title": "Mon histoire",
        }))
        .expect("deser");
        assert_eq!(dto.pack_uuid, "abababab-abab-abab-abab-ababfac5562d");
        assert_eq!(dto.title, "Mon histoire");
    }

    #[test]
    fn input_rejects_unknown_field() {
        let err = serde_json::from_value::<SetDeviceStoryTitleInputDto>(serde_json::json!({
            "packUuid": "x",
            "title": "y",
            "source": "official",
        }))
        .expect_err("must reject unknown field — provenance is Rust-owned");
        assert!(err.to_string().contains("source"));
    }

    #[test]
    fn input_rejects_snake_case_field() {
        serde_json::from_value::<SetDeviceStoryTitleInputDto>(serde_json::json!({
            "pack_uuid": "x",
            "title": "y",
        }))
        .expect_err("must reject snake_case");
    }

    #[test]
    fn outcome_serializes_with_camel_case_source_token() {
        let dto = DeviceStoryTitleDto::from_pack_title(PackTitle {
            title: "Mon histoire".into(),
            source: PackTitleSource::User,
            thumbnail: None,
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["title"], "Mon histoire");
        assert_eq!(v["source"], "user");
    }
}
