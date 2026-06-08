use serde::Serialize;

use crate::application::device::ConnectedLuniiOutcome;
use crate::domain::device::{
    DeviceFamily, DeviceProfile, LuniiFirmwareCohort, SupportedOperations, UnsupportedReason,
};

/// Wire shape returned by the `read_connected_lunii` Tauri command.
///
/// Tagged enum on `kind`: `"none"`, `"supported"`, `"unsupported"`,
/// `"ambiguous"`. All field names are camelCase. The frontend mirror
/// lives at `src/shared/ipc-contracts/device.ts` — drift is enforced by
/// the contract tests in `src-tauri/tests/contracts/device.rs` AND the
/// runtime guard `isConnectedDeviceDto`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConnectedDeviceDto {
    None,
    #[serde(rename_all = "camelCase")]
    Supported {
        family: SupportedFamilyDto,
        firmware_cohort: FirmwareCohortDto,
        metadata_format_version: u8,
        device_identifier: String,
        supported_operations: SupportedOperationsDto,
    },
    #[serde(rename_all = "camelCase")]
    Unsupported {
        reason: UnsupportedReasonDto,
        firmware_hint: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Ambiguous {
        candidate_count: u32,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SupportedFamilyDto {
    Lunii,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FirmwareCohortDto {
    OrigineV1,
    MidGenV2,
    V3,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportedOperationsDto {
    pub read_library: bool,
    pub inspect_story: bool,
    pub import_story: bool,
    pub write_story: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UnsupportedReasonDto {
    FirmwareUnsupported,
    MetadataUnsupported,
    MetadataCorrupt,
    FamilyUnknown,
    OperationNotAuthorized,
    MultipleCandidates,
}

impl ConnectedDeviceDto {
    pub fn from_outcome(outcome: ConnectedLuniiOutcome) -> Self {
        match outcome {
            ConnectedLuniiOutcome::None => Self::None,
            ConnectedLuniiOutcome::Supported(profile) => Self::Supported {
                family: family_dto(profile.family),
                firmware_cohort: cohort_dto(profile.firmware_cohort),
                metadata_format_version: profile.metadata_format_version,
                device_identifier: profile.device_identifier.clone(),
                supported_operations: operations_dto(&profile),
            },
            ConnectedLuniiOutcome::Unsupported {
                reason,
                firmware_hint,
            } => Self::Unsupported {
                reason: reason_dto(reason),
                firmware_hint,
            },
            ConnectedLuniiOutcome::Ambiguous { candidate_count } => {
                Self::Ambiguous { candidate_count }
            }
        }
    }
}

fn family_dto(f: DeviceFamily) -> SupportedFamilyDto {
    match f {
        DeviceFamily::Lunii => SupportedFamilyDto::Lunii,
    }
}

fn cohort_dto(c: LuniiFirmwareCohort) -> FirmwareCohortDto {
    match c {
        LuniiFirmwareCohort::OrigineV1 => FirmwareCohortDto::OrigineV1,
        LuniiFirmwareCohort::MidGenV2 => FirmwareCohortDto::MidGenV2,
        LuniiFirmwareCohort::V3 => FirmwareCohortDto::V3,
    }
}

fn operations_dto(profile: &DeviceProfile) -> SupportedOperationsDto {
    let ops: SupportedOperations = profile.supported_operations;
    SupportedOperationsDto {
        read_library: ops.read_library,
        inspect_story: ops.inspect_story,
        import_story: ops.import_story,
        write_story: ops.write_story,
    }
}

fn reason_dto(r: UnsupportedReason) -> UnsupportedReasonDto {
    match r {
        UnsupportedReason::FirmwareUnsupported => UnsupportedReasonDto::FirmwareUnsupported,
        UnsupportedReason::MetadataUnsupported => UnsupportedReasonDto::MetadataUnsupported,
        UnsupportedReason::MetadataCorrupt => UnsupportedReasonDto::MetadataCorrupt,
        UnsupportedReason::FamilyUnknown => UnsupportedReasonDto::FamilyUnknown,
        UnsupportedReason::OperationNotAuthorized => UnsupportedReasonDto::OperationNotAuthorized,
        UnsupportedReason::MultipleCandidates => UnsupportedReasonDto::MultipleCandidates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn none_variant_serializes_as_kind_none() {
        let v = serde_json::to_value(&ConnectedDeviceDto::None).expect("ser");
        assert_eq!(v, json!({ "kind": "none" }));
    }

    #[test]
    fn supported_variant_round_trips_with_camel_case_fields() {
        let dto = ConnectedDeviceDto::Supported {
            family: SupportedFamilyDto::Lunii,
            firmware_cohort: FirmwareCohortDto::OrigineV1,
            metadata_format_version: 3,
            device_identifier: "abc".into(),
            supported_operations: SupportedOperationsDto {
                read_library: true,
                inspect_story: true,
                import_story: true,
                write_story: false,
            },
        };
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "supported");
        assert_eq!(v["family"], "lunii");
        assert_eq!(v["firmwareCohort"], "origineV1");
        assert_eq!(v["metadataFormatVersion"], 3);
        assert_eq!(v["deviceIdentifier"], "abc");
        assert_eq!(v["supportedOperations"]["readLibrary"], true);
        assert_eq!(v["supportedOperations"]["writeStory"], false);
        assert!(
            v.get("supported_operations").is_none(),
            "snake_case must not leak"
        );
    }

    #[test]
    fn unsupported_variant_serializes_with_typed_reason() {
        let dto = ConnectedDeviceDto::Unsupported {
            reason: UnsupportedReasonDto::MetadataUnsupported,
            firmware_hint: Some("metadata_v99".into()),
        };
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "unsupported");
        assert_eq!(v["reason"], "metadataUnsupported");
        assert_eq!(v["firmwareHint"], "metadata_v99");
    }

    #[test]
    fn ambiguous_variant_serializes_with_count() {
        let dto = ConnectedDeviceDto::Ambiguous { candidate_count: 3 };
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "ambiguous");
        assert_eq!(v["candidateCount"], 3);
    }

    #[test]
    fn from_outcome_maps_supported_origine_v1() {
        let outcome = ConnectedLuniiOutcome::Supported(
            match crate::domain::device::classify_lunii(3, true, true, "id") {
                crate::domain::device::DeviceProfileClassification::Supported(p) => p,
                _ => unreachable!(),
            },
        );
        let dto = ConnectedDeviceDto::from_outcome(outcome);
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "supported");
        assert_eq!(v["firmwareCohort"], "origineV1");
        assert_eq!(v["supportedOperations"]["importStory"], true);
        assert_eq!(v["supportedOperations"]["writeStory"], false);
    }

    #[test]
    fn from_outcome_maps_supported_v3_with_import_disabled() {
        let outcome = ConnectedLuniiOutcome::Supported(
            match crate::domain::device::classify_lunii(7, true, true, "id") {
                crate::domain::device::DeviceProfileClassification::Supported(p) => p,
                _ => unreachable!(),
            },
        );
        let dto = ConnectedDeviceDto::from_outcome(outcome);
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["firmwareCohort"], "v3");
        assert_eq!(v["supportedOperations"]["importStory"], false);
        assert_eq!(v["supportedOperations"]["writeStory"], false);
    }

    #[test]
    fn from_outcome_maps_unsupported_metadata() {
        let outcome = ConnectedLuniiOutcome::Unsupported {
            reason: UnsupportedReason::MetadataUnsupported,
            firmware_hint: Some("metadata_v4".into()),
        };
        let dto = ConnectedDeviceDto::from_outcome(outcome);
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "unsupported");
        assert_eq!(v["reason"], "metadataUnsupported");
        assert_eq!(v["firmwareHint"], "metadata_v4");
    }

    #[test]
    fn from_outcome_maps_ambiguous() {
        let outcome = ConnectedLuniiOutcome::Ambiguous { candidate_count: 2 };
        let dto = ConnectedDeviceDto::from_outcome(outcome);
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "ambiguous");
        assert_eq!(v["candidateCount"], 2);
    }

    #[test]
    fn from_outcome_maps_none() {
        let outcome = ConnectedLuniiOutcome::None;
        let dto = ConnectedDeviceDto::from_outcome(outcome);
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "none");
    }

    #[test]
    fn unsupported_reason_dto_serializes_each_variant_in_camel_case() {
        for (variant, expected) in [
            (
                UnsupportedReasonDto::FirmwareUnsupported,
                "firmwareUnsupported",
            ),
            (
                UnsupportedReasonDto::MetadataUnsupported,
                "metadataUnsupported",
            ),
            (UnsupportedReasonDto::MetadataCorrupt, "metadataCorrupt"),
            (UnsupportedReasonDto::FamilyUnknown, "familyUnknown"),
            (
                UnsupportedReasonDto::OperationNotAuthorized,
                "operationNotAuthorized",
            ),
            (
                UnsupportedReasonDto::MultipleCandidates,
                "multipleCandidates",
            ),
        ] {
            let v = serde_json::to_value(&variant).expect("ser");
            assert_eq!(v, serde_json::Value::String(expected.into()), "{variant:?}");
        }
    }

    #[test]
    fn firmware_cohort_dto_serializes_each_variant_in_camel_case() {
        let v = serde_json::to_value(&FirmwareCohortDto::OrigineV1).expect("ser");
        assert_eq!(v, serde_json::Value::String("origineV1".into()));
        let v = serde_json::to_value(&FirmwareCohortDto::MidGenV2).expect("ser");
        assert_eq!(v, serde_json::Value::String("midGenV2".into()));
        let v = serde_json::to_value(&FirmwareCohortDto::V3).expect("ser");
        assert_eq!(v, serde_json::Value::String("v3".into()));
    }

    #[test]
    fn supported_family_dto_serializes_lunii_in_camel_case() {
        let v = serde_json::to_value(&SupportedFamilyDto::Lunii).expect("ser");
        assert_eq!(v, serde_json::Value::String("lunii".into()));
    }
}
