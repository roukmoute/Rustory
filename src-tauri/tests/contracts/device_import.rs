//! Wire-contract tests for the `import_device_story` command. Mirror of
//! `src/shared/ipc-contracts/device-import.ts` — both sides assert the
//! exact same shapes so a drift fails loudly in CI on either stack.

use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::{ImportDeviceStoryInputDto, ImportDeviceStoryOutcomeDto, StoryCardDto};

#[test]
fn input_accepts_the_canonical_camel_case_payload() {
    let dto: ImportDeviceStoryInputDto = serde_json::from_value(serde_json::json!({
        "deviceIdentifier": "0123456789abcdef0123456789abcdef",
        "packUuid": "abababab-abab-abab-abab-ababfac5562d",
    }))
    .expect("canonical payload must deserialize");
    assert_eq!(dto.device_identifier, "0123456789abcdef0123456789abcdef");
    assert_eq!(dto.pack_uuid, "abababab-abab-abab-abab-ababfac5562d");
}

#[test]
fn input_rejects_unknown_fields_so_no_path_can_sneak_through() {
    serde_json::from_value::<ImportDeviceStoryInputDto>(serde_json::json!({
        "deviceIdentifier": "0123456789abcdef0123456789abcdef",
        "packUuid": "abababab-abab-abab-abab-ababfac5562d",
        "mountPath": "/media/lunii",
    }))
    .expect_err("an extra field must be refused at the boundary");
}

#[test]
fn input_rejects_snake_case_fields() {
    serde_json::from_value::<ImportDeviceStoryInputDto>(serde_json::json!({
        "device_identifier": "0123456789abcdef0123456789abcdef",
        "pack_uuid": "abababab-abab-abab-abab-ababfac5562d",
    }))
    .expect_err("snake_case must be refused");
}

#[test]
fn outcome_round_trips_the_documented_wire_shape() {
    let dto = ImportDeviceStoryOutcomeDto {
        story: StoryCardDto {
            id: "0197a5d0-0000-7000-8000-000000000000".into(),
            title: "Histoire de ma Lunii (FAC5562D)".into(),
        },
        pack_short_id: "FAC5562D".into(),
        imported_at: "2026-06-10T12:00:00.000Z".into(),
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "story": {
                "id": "0197a5d0-0000-7000-8000-000000000000",
                "title": "Histoire de ma Lunii (FAC5562D)",
            },
            "packShortId": "FAC5562D",
            "importedAt": "2026-06-10T12:00:00.000Z",
        })
    );
}

#[test]
fn import_failed_error_carries_stable_code_and_closed_source() {
    let err = AppError::import_failed(
        "Copie impossible: l'appareil connecté a changé.",
        "Rebranche la Lunii souhaitée puis réessaie la copie.",
    )
    .with_details(serde_json::json!({
        "source": "device_changed",
    }));
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "IMPORT_FAILED");
    assert_eq!(v["details"]["source"], "device_changed");
    assert!(v.get("userAction").is_some());
    assert!(v.get("user_action").is_none(), "snake_case must not leak");
}

#[test]
fn import_failed_sources_form_the_documented_closed_set() {
    // One serialized sample per documented `details.source` token —
    // ui-states.md#Device Story Import Contract is the public list.
    for source in [
        "already_imported",
        "pack_missing",
        "pack_invalid",
        "pack_oversize",
        "device_changed",
        "fs_read",
        "staging_write",
        "promote",
        "db_commit",
        "read_timeout",
        "spawn_blocking_join",
        "other",
    ] {
        let err = AppError::import_failed("msg", "action")
            .with_details(serde_json::json!({ "source": source }));
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], source);
    }
}
