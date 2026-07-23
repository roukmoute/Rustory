use serde::{Deserialize, Serialize};

use crate::application::device::send::SentToDevice;

/// Input accepted by the `send_pack_to_device` Tauri command. EXACTLY the one
/// identifier the UI legitimately holds (opaque hashed `deviceIdentifier`) —
/// the source archive is picked in a NATIVE dialog owned by Rust, so no path
/// ever crosses the IPC boundary in either direction; `deny_unknown_fields`
/// refuses any smuggled one.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendPackToDeviceInputDto {
    pub device_identifier: String,
}

/// Outcome of a settled `send_pack_to_device`. Tagged on `kind`: a dismissed
/// native dialog is `cancelled` (a non-event, never an error — the
/// catalog-import pattern); a completed write is `sent` with the pack facts
/// the UI echoes. Family-neutral: the family/cohort stay diagnostic details.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SendPackToDeviceOutcomeDto {
    Cancelled,
    #[serde(rename_all = "camelCase")]
    Sent {
        pack_uuid: String,
        image_count: u32,
        audio_count: u32,
    },
}

impl SendPackToDeviceOutcomeDto {
    pub fn from_outcome(outcome: SentToDevice) -> Self {
        Self::Sent {
            pack_uuid: outcome.pack_uuid,
            // Bounded by the archive entry cap, far under u32.
            image_count: outcome.image_count as u32,
            audio_count: outcome.audio_count as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_accepts_canonical_camel_case_payload() {
        let dto: SendPackToDeviceInputDto = serde_json::from_value(serde_json::json!({
            "deviceIdentifier": "0123456789abcdef0123456789abcdef",
        }))
        .expect("deser");
        assert_eq!(dto.device_identifier, "0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn input_rejects_an_unknown_field_so_no_path_crosses_ipc() {
        let err = serde_json::from_value::<SendPackToDeviceInputDto>(serde_json::json!({
            "deviceIdentifier": "x",
            "archivePath": "/sneaky.zip",
        }))
        .expect_err("must reject unknown field");
        assert!(err.to_string().contains("archivePath"));
    }

    #[test]
    fn cancelled_outcome_serializes_with_kind_only() {
        let v = serde_json::to_value(SendPackToDeviceOutcomeDto::Cancelled).expect("ser");
        assert_eq!(v, serde_json::json!({ "kind": "cancelled" }));
    }

    #[test]
    fn sent_outcome_serializes_in_camel_case_family_neutral() {
        let dto = SendPackToDeviceOutcomeDto::from_outcome(SentToDevice {
            pack_uuid: "abababab-abab-abab-abab-ababfac5562d".into(),
            short_id: "FAC5562D".into(),
            image_count: 117,
            audio_count: 223,
            family: crate::domain::device::DeviceFamily::Lunii,
            firmware_cohort: crate::domain::device::FirmwareCohort::Lunii(
                crate::domain::device::LuniiFirmwareCohort::V3,
            ),
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(
            v,
            serde_json::json!({
                "kind": "sent",
                "packUuid": "abababab-abab-abab-abab-ababfac5562d",
                "imageCount": 117,
                "audioCount": 223,
            })
        );
        // The family/cohort/short id never cross the wire.
        assert!(v.get("family").is_none());
        assert!(v.get("firmwareCohort").is_none());
        assert!(v.get("shortId").is_none());
    }
}
