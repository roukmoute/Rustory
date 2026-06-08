use rustory_lib::ipc::dto::{DeviceLibraryDto, DeviceStoryDto, UnsupportedReasonDto};

fn story(short_id: &str, hidden: bool, content_present: bool) -> DeviceStoryDto {
    DeviceStoryDto {
        uuid: format!("00000000-0000-0000-0000-0000{short_id}"),
        short_id: short_id.into(),
        hidden,
        content_present,
    }
}

fn readable_dto() -> DeviceLibraryDto {
    DeviceLibraryDto::Readable {
        device_identifier: "0123456789abcdef0123456789abcdef".into(),
        stories: vec![
            story("0000ABCD", false, true),
            story("0000BEEF", true, false),
        ],
    }
}

#[test]
fn device_library_none_serializes_with_kind_none() {
    let v = serde_json::to_value(DeviceLibraryDto::None).expect("ser");
    assert_eq!(v, serde_json::json!({ "kind": "none" }));
}

#[test]
fn device_library_readable_round_trip_wire_shape() {
    let v = serde_json::to_value(readable_dto()).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "readable",
            "deviceIdentifier": "0123456789abcdef0123456789abcdef",
            "stories": [
                {
                    "uuid": "00000000-0000-0000-0000-00000000ABCD",
                    "shortId": "0000ABCD",
                    "hidden": false,
                    "contentPresent": true,
                },
                {
                    "uuid": "00000000-0000-0000-0000-00000000BEEF",
                    "shortId": "0000BEEF",
                    "hidden": true,
                    "contentPresent": false,
                },
            ],
        })
    );
}

#[test]
fn device_library_readable_with_no_packs_serializes_empty_stories_array() {
    let dto = DeviceLibraryDto::Readable {
        device_identifier: "ffffffffffffffffffffffffffffffff".into(),
        stories: vec![],
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "readable");
    assert_eq!(v["stories"], serde_json::json!([]));
}

#[test]
fn device_library_unsupported_serializes_with_typed_reason() {
    let dto = DeviceLibraryDto::Unsupported {
        reason: UnsupportedReasonDto::MultipleCandidates,
        firmware_hint: Some("count_2".into()),
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "unsupported");
    assert_eq!(v["reason"], "multipleCandidates");
    assert_eq!(v["firmwareHint"], "count_2");
}

#[test]
fn device_identifier_field_is_string_not_object() {
    let v = serde_json::to_value(readable_dto()).expect("ser");
    assert!(v["deviceIdentifier"].is_string());
}

#[test]
fn device_story_uses_camel_case_only() {
    let v = serde_json::to_value(readable_dto()).expect("ser");
    let first = &v["stories"][0];
    for camel in ["uuid", "shortId", "hidden", "contentPresent"] {
        assert!(
            first.get(camel).is_some(),
            "missing camelCase field: {camel}"
        );
    }
    for snake in ["short_id", "content_present"] {
        assert!(
            first.get(snake).is_none(),
            "snake_case must not leak: {snake}"
        );
    }
}

#[test]
fn none_variant_does_not_emit_extra_fields() {
    let v = serde_json::to_value(DeviceLibraryDto::None).expect("ser");
    let obj = v.as_object().expect("object");
    assert_eq!(obj.len(), 1);
    assert!(obj.contains_key("kind"));
}
