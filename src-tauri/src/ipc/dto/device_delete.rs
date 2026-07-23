use serde::{Deserialize, Serialize};

use crate::application::device::delete::DeletedDeviceStory;

/// Input accepted by the `delete_device_story` Tauri command. Same
/// boundary discipline as the import input: exactly the two identifiers the
/// UI legitimately holds (opaque hashed `deviceIdentifier`, canonical
/// `packUuid`), `deny_unknown_fields` so no path can ever be smuggled in.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteDeviceStoryInputDto {
    pub device_identifier: String,
    pub pack_uuid: String,
}

/// Outcome of a settled `delete_device_story`. Family-neutral (the family
/// stays a diagnostic detail, never on the wire). `wasPresent` is `false`
/// when the pack was already absent — an idempotent no-op, not an error.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDeviceStoryOutcomeDto {
    pub pack_uuid: String,
    pub was_present: bool,
}

impl DeleteDeviceStoryOutcomeDto {
    pub fn from_outcome(outcome: DeletedDeviceStory) -> Self {
        Self {
            pack_uuid: outcome.pack_uuid,
            was_present: outcome.was_present,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_accepts_canonical_camel_case_payload() {
        let dto: DeleteDeviceStoryInputDto = serde_json::from_value(serde_json::json!({
            "deviceIdentifier": "0123456789abcdef0123456789abcdef",
            "packUuid": "abababab-abab-abab-abab-ababfac5562d",
        }))
        .expect("deser");
        assert_eq!(dto.device_identifier, "0123456789abcdef0123456789abcdef");
        assert_eq!(dto.pack_uuid, "abababab-abab-abab-abab-ababfac5562d");
    }

    #[test]
    fn input_rejects_an_unknown_field_so_no_path_crosses_ipc() {
        let err = serde_json::from_value::<DeleteDeviceStoryInputDto>(serde_json::json!({
            "deviceIdentifier": "x",
            "packUuid": "y",
            "mountPath": "/sneaky",
        }))
        .expect_err("must reject unknown field");
        assert!(err.to_string().contains("mountPath"));
    }

    #[test]
    fn outcome_serializes_in_camel_case_family_neutral() {
        let dto = DeleteDeviceStoryOutcomeDto::from_outcome(DeletedDeviceStory {
            pack_uuid: "abababab-abab-abab-abab-ababfac5562d".into(),
            was_present: true,
            family: crate::domain::device::DeviceFamily::Lunii,
            firmware_cohort: crate::domain::device::FirmwareCohort::Lunii(
                crate::domain::device::LuniiFirmwareCohort::V3,
            ),
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["packUuid"], "abababab-abab-abab-abab-ababfac5562d");
        assert_eq!(v["wasPresent"], true);
        assert!(v.get("was_present").is_none(), "must be camelCase");
        // The family/cohort never cross the wire.
        assert!(v.get("family").is_none());
        assert!(v.get("firmwareCohort").is_none());
    }
}
