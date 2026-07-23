use rustory_lib::ipc::dto::{
    ConnectedDeviceDto, FirmwareCohortDto, SupportedFamilyDto, SupportedOperationsDto,
    UnsupportedReasonDto,
};

fn supported_dto() -> ConnectedDeviceDto {
    ConnectedDeviceDto::Supported {
        family: SupportedFamilyDto::Lunii,
        firmware_cohort: FirmwareCohortDto::OrigineV1,
        metadata_format_version: Some(3),
        device_identifier: "0123456789abcdef0123456789abcdef".into(),
        supported_operations: SupportedOperationsDto {
            read_library: true,
            inspect_story: true,
            import_story: true,
            write_story: false,
            delete_story: true,
            send_archive: false,
        },
    }
}

fn supported_flam_dto() -> ConnectedDeviceDto {
    ConnectedDeviceDto::Supported {
        family: SupportedFamilyDto::Flam,
        firmware_cohort: FirmwareCohortDto::FlamGen1,
        metadata_format_version: None,
        device_identifier: "fedcba9876543210fedcba9876543210".into(),
        // The FLAM Gen1 matrix line: read capabilities ✅✅✅, write ❌,
        // delete ❌, archive-send ❌ (unproven on real FLAM hardware).
        supported_operations: SupportedOperationsDto {
            read_library: true,
            inspect_story: true,
            import_story: true,
            write_story: false,
            delete_story: false,
            send_archive: false,
        },
    }
}

#[test]
fn connected_device_none_serializes_with_kind_none() {
    let v = serde_json::to_value(&ConnectedDeviceDto::None).expect("ser");
    assert_eq!(v, serde_json::json!({ "kind": "none" }));
}

#[test]
fn connected_device_supported_origine_v1_round_trip_wire_shape() {
    let v = serde_json::to_value(supported_dto()).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "supported",
            "family": "lunii",
            "firmwareCohort": "origineV1",
            "metadataFormatVersion": 3,
            "deviceIdentifier": "0123456789abcdef0123456789abcdef",
            "supportedOperations": {
                "readLibrary": true,
                "inspectStory": true,
                "importStory": true,
                "writeStory": false,
                "deleteStory": true,
                "sendArchive": false,
            },
        })
    );
}

#[test]
fn connected_device_supported_v3_serializes_with_import_story_false() {
    let dto = ConnectedDeviceDto::Supported {
        family: SupportedFamilyDto::Lunii,
        firmware_cohort: FirmwareCohortDto::V3,
        metadata_format_version: Some(7),
        device_identifier: "fedcba9876543210fedcba9876543210".into(),
        supported_operations: SupportedOperationsDto {
            read_library: true,
            inspect_story: true,
            import_story: false,
            write_story: false,
            delete_story: true,
            send_archive: true,
        },
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["firmwareCohort"], "v3");
    assert_eq!(v["supportedOperations"]["importStory"], false);
    assert_eq!(v["supportedOperations"]["writeStory"], false);
    // V3 CAN delete even while import/write stay closed — deletion is
    // crypto-free (delist + remove opaque bytes).
    assert_eq!(v["supportedOperations"]["deleteStory"], true);
    // And V3 CAN receive a pack archive: the DEDICATED send capability,
    // open while the round-trip `writeStory` stays closed.
    assert_eq!(v["supportedOperations"]["sendArchive"], true);
}

#[test]
fn connected_device_unsupported_metadata_serializes_with_typed_reason() {
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
fn connected_device_unsupported_serializes_null_firmware_hint_when_absent() {
    let dto = ConnectedDeviceDto::Unsupported {
        reason: UnsupportedReasonDto::MetadataCorrupt,
        firmware_hint: None,
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["reason"], "metadataCorrupt");
    assert!(v["firmwareHint"].is_null());
}

#[test]
fn connected_device_ambiguous_with_2_candidates() {
    let dto = ConnectedDeviceDto::Ambiguous { candidate_count: 2 };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "ambiguous");
    assert_eq!(v["candidateCount"], 2);
}

#[test]
fn supported_operations_dto_uses_camel_case_only() {
    let v = serde_json::to_value(supported_dto()).expect("ser");
    let ops = &v["supportedOperations"];
    for camel in [
        "readLibrary",
        "inspectStory",
        "importStory",
        "writeStory",
        "deleteStory",
        "sendArchive",
    ] {
        assert!(ops.get(camel).is_some(), "missing camelCase field: {camel}");
    }
    for snake in [
        "read_library",
        "inspect_story",
        "import_story",
        "write_story",
        "delete_story",
        "send_archive",
    ] {
        assert!(
            ops.get(snake).is_none(),
            "snake_case must not leak: {snake}"
        );
    }
}

#[test]
fn firmware_cohort_serializes_as_camel_case_string_for_each_variant() {
    for (variant, expected) in [
        (FirmwareCohortDto::OrigineV1, "origineV1"),
        (FirmwareCohortDto::MidGenV2, "midGenV2"),
        (FirmwareCohortDto::V3, "v3"),
    ] {
        let v = serde_json::to_value(&variant).expect("ser");
        assert_eq!(v, serde_json::Value::String(expected.into()));
    }
}

#[test]
fn unsupported_reason_serializes_as_camel_case_string_for_each_variant() {
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
        assert_eq!(v, serde_json::Value::String(expected.into()));
    }
}

#[test]
fn none_variant_does_not_emit_extra_fields() {
    let v = serde_json::to_value(&ConnectedDeviceDto::None).expect("ser");
    let obj = v.as_object().expect("object");
    // Exactly one key: "kind".
    assert_eq!(obj.len(), 1);
    assert!(obj.contains_key("kind"));
}

#[test]
fn device_identifier_field_is_string_not_object() {
    let v = serde_json::to_value(supported_dto()).expect("ser");
    assert!(v["deviceIdentifier"].is_string());
}

#[test]
fn connected_device_supported_does_not_emit_snake_case_aliases() {
    let v = serde_json::to_value(supported_dto()).expect("ser");
    for snake in [
        "firmware_cohort",
        "metadata_format_version",
        "device_identifier",
        "supported_operations",
    ] {
        assert!(v.get(snake).is_none(), "snake_case must not leak: {snake}");
    }
}

/// Byte-for-byte INVARIANCE of the Lunii supported wire: the exact JSON
/// string frozen as a literal. `deleteStory` then `sendArchive` were each
/// appended as deliberate wire extensions (device-delete, then archive-send
/// capability) AFTER `writeStory`, keeping every prior field's position and
/// value; the `Option<u8>` migration of `metadataFormatVersion` still must
/// not deform the wire — key present, plain integer, same order.
#[test]
fn connected_device_supported_lunii_wire_string_is_byte_for_byte_unchanged() {
    let s = serde_json::to_string(&supported_dto()).expect("ser");
    assert_eq!(
        s,
        "{\"kind\":\"supported\",\"family\":\"lunii\",\"firmwareCohort\":\"origineV1\",\
         \"metadataFormatVersion\":3,\
         \"deviceIdentifier\":\"0123456789abcdef0123456789abcdef\",\
         \"supportedOperations\":{\"readLibrary\":true,\"inspectStory\":true,\
         \"importStory\":true,\"writeStory\":false,\"deleteStory\":true,\
         \"sendArchive\":false}}"
    );
}

/// Twin of the invariance test for the FLAM wire (re-scoped with the
/// activated read capabilities, never deleted): `family`/`firmwareCohort`
/// discriminate, `readLibrary`/`inspectStory`/`importStory` are `true`,
/// `writeStory` stays `false`, and the `metadataFormatVersion` key is
/// ABSENT from the string (never `null`).
#[test]
fn connected_device_supported_flam_wire_string_omits_version_key() {
    let s = serde_json::to_string(&supported_flam_dto()).expect("ser");
    assert_eq!(
        s,
        "{\"kind\":\"supported\",\"family\":\"flam\",\"firmwareCohort\":\"flamGen1\",\
         \"deviceIdentifier\":\"fedcba9876543210fedcba9876543210\",\
         \"supportedOperations\":{\"readLibrary\":true,\"inspectStory\":true,\
         \"importStory\":true,\"writeStory\":false,\"deleteStory\":false,\
         \"sendArchive\":false}}"
    );
    assert!(!s.contains("metadataFormatVersion"));
    assert!(!s.contains("null"));
}

#[test]
fn firmware_cohort_flam_gen1_serializes_as_camel_case_string() {
    let v = serde_json::to_value(FirmwareCohortDto::FlamGen1).expect("ser");
    assert_eq!(v, serde_json::Value::String("flamGen1".into()));
}

#[test]
fn supported_family_flam_serializes_as_camel_case_string() {
    let v = serde_json::to_value(SupportedFamilyDto::Flam).expect("ser");
    assert_eq!(v, serde_json::Value::String("flam".into()));
}
