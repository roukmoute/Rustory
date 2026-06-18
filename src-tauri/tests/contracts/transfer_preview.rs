//! Wire-shape contract for the `read_transfer_preview` command DTO. Pure
//! serialization assertions — the cross-stack mirror is
//! `src/shared/ipc-contracts/transfer-preview.ts`, kept symmetric by these
//! tests + the runtime guard `isTransferPreviewDto`.

use rustory_lib::ipc::dto::{
    ReadTransferPreviewInputDto, TransferPreviewDto, TransferPreviewStoryDto, UnsupportedReasonDto,
};

const VALID_ID: &str = "0123456789abcdef0123456789abcdef";

fn ready(on_device: bool, unchanged_count: u32) -> TransferPreviewDto {
    TransferPreviewDto::Ready {
        device_identifier: VALID_ID.into(),
        story: TransferPreviewStoryDto {
            id: "0197a5d0-0000-7000-8000-000000000000".into(),
            title: "Mon histoire".into(),
        },
        on_device,
        unchanged_count,
        transferable: false,
    }
}

#[test]
fn no_device_serializes_with_kind_only() {
    let v = serde_json::to_value(TransferPreviewDto::NoDevice).expect("ser");
    assert_eq!(v, serde_json::json!({ "kind": "noDevice" }));
    assert_eq!(v.as_object().expect("obj").len(), 1);
}

#[test]
fn ready_replace_round_trips_wire_shape() {
    let v = serde_json::to_value(ready(true, 2)).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "ready",
            "deviceIdentifier": "0123456789abcdef0123456789abcdef",
            "story": {
                "id": "0197a5d0-0000-7000-8000-000000000000",
                "title": "Mon histoire",
            },
            "onDevice": true,
            "unchangedCount": 2,
            "transferable": false,
        })
    );
}

#[test]
fn ready_new_serializes_on_device_false() {
    let v = serde_json::to_value(ready(false, 5)).expect("ser");
    assert_eq!(v["kind"], "ready");
    assert_eq!(v["onDevice"], false);
    assert_eq!(v["unchangedCount"], 5);
    assert_eq!(v["transferable"], false);
}

#[test]
fn device_identifier_field_is_string_not_object() {
    let v = serde_json::to_value(ready(false, 0)).expect("ser");
    assert!(v["deviceIdentifier"].is_string());
}

#[test]
fn ready_uses_camel_case_only() {
    let v = serde_json::to_value(ready(true, 1)).expect("ser");
    for camel in [
        "deviceIdentifier",
        "story",
        "onDevice",
        "unchangedCount",
        "transferable",
    ] {
        assert!(v.get(camel).is_some(), "missing camelCase field: {camel}");
    }
    for snake in ["device_identifier", "on_device", "unchanged_count"] {
        assert!(v.get(snake).is_none(), "snake_case must not leak: {snake}");
    }
}

#[test]
fn unsupported_serializes_typed_reason() {
    let dto = TransferPreviewDto::Unsupported {
        reason: UnsupportedReasonDto::MultipleCandidates,
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "unsupported");
    assert_eq!(v["reason"], "multipleCandidates");
    // kind + reason, nothing else.
    assert_eq!(v.as_object().expect("obj").len(), 2);
}

#[test]
fn input_accepts_canonical_camel_case_payload() {
    let dto: ReadTransferPreviewInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "0197a5d0-0000-7000-8000-000000000000",
        "deviceIdentifier": VALID_ID,
    }))
    .expect("deser");
    assert_eq!(dto.story_id, "0197a5d0-0000-7000-8000-000000000000");
    assert_eq!(dto.device_identifier, VALID_ID);
}

#[test]
fn input_rejects_unknown_field_no_path_crosses_ipc() {
    let err = serde_json::from_value::<ReadTransferPreviewInputDto>(serde_json::json!({
        "storyId": "x",
        "deviceIdentifier": "y",
        "mountPath": "/sneaky",
    }))
    .expect_err("must reject unknown field");
    assert!(err.to_string().contains("mountPath"));
}
