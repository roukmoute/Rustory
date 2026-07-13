use rustory_lib::ipc::dto::{
    DeviceLibraryDto, DeviceStoryDto, PackTitleSourceDto, UnsupportedReasonDto,
};

fn story(short_id: &str, hidden: bool, content_present: bool) -> DeviceStoryDto {
    DeviceStoryDto {
        uuid: format!("00000000-0000-0000-0000-0000{short_id}"),
        short_id: short_id.into(),
        hidden,
        content_present,
        already_imported: false,
        title: None,
        title_source: None,
        thumbnail: None,
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
                    "alreadyImported": false,
                    "title": null,
                    "titleSource": null,
                    "thumbnail": null,
                },
                {
                    "uuid": "00000000-0000-0000-0000-00000000BEEF",
                    "shortId": "0000BEEF",
                    "hidden": true,
                    "contentPresent": false,
                    "alreadyImported": false,
                    "title": null,
                    "titleSource": null,
                    "thumbnail": null,
                },
            ],
        })
    );
}

#[test]
fn device_library_readable_flam_round_trips_the_same_neutral_wire_shape() {
    // The library DTO is family-NEUTRAL: a readable FLAM inventory rides
    // the exact same shape as a Lunii one — a real FLAM story UUID, its
    // uppercase 8-hex tail as shortId, the same flags and recognition
    // fields. Fixture = real wire (what the FLAM reader actually emits).
    let dto = DeviceLibraryDto::Readable {
        device_identifier: "fedcba9876543210fedcba9876543210".into(),
        stories: vec![DeviceStoryDto {
            uuid: "12345678-9abc-def0-1122-334455667788".into(),
            short_id: "55667788".into(),
            hidden: true,
            content_present: true,
            already_imported: false,
            title: None,
            title_source: None,
            thumbnail: None,
        }],
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "readable",
            "deviceIdentifier": "fedcba9876543210fedcba9876543210",
            "stories": [
                {
                    "uuid": "12345678-9abc-def0-1122-334455667788",
                    "shortId": "55667788",
                    "hidden": true,
                    "contentPresent": true,
                    "alreadyImported": false,
                    "title": null,
                    "titleSource": null,
                    "thumbnail": null,
                },
            ],
        })
    );
    // No family field exists on this wire — the capability matrix alone
    // decides what lights up.
    let raw = serde_json::to_string(&dto).expect("ser");
    assert!(!raw.contains("family"));
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
    for camel in [
        "uuid",
        "shortId",
        "hidden",
        "contentPresent",
        "alreadyImported",
        "title",
        "titleSource",
        "thumbnail",
    ] {
        assert!(
            first.get(camel).is_some(),
            "missing camelCase field: {camel}"
        );
    }
    for snake in [
        "short_id",
        "content_present",
        "already_imported",
        "title_source",
    ] {
        assert!(
            first.get(snake).is_none(),
            "snake_case must not leak: {snake}"
        );
    }
}

#[test]
fn device_story_already_imported_serializes_true_when_stamped() {
    let dto = DeviceLibraryDto::Readable {
        device_identifier: "0123456789abcdef0123456789abcdef".into(),
        stories: vec![DeviceStoryDto {
            uuid: "00000000-0000-0000-0000-00000000abcd".into(),
            short_id: "0000ABCD".into(),
            hidden: false,
            content_present: true,
            already_imported: true,
            title: None,
            title_source: None,
            thumbnail: None,
        }],
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["stories"][0]["alreadyImported"], true);
}

#[test]
fn device_story_recognized_title_serializes_with_provenance_and_cover() {
    let dto = DeviceLibraryDto::Readable {
        device_identifier: "0123456789abcdef0123456789abcdef".into(),
        stories: vec![DeviceStoryDto {
            uuid: "00000000-0000-0000-0000-00000000abcd".into(),
            short_id: "0000ABCD".into(),
            hidden: false,
            content_present: true,
            already_imported: false,
            title: Some("Le Loup".into()),
            title_source: Some(PackTitleSourceDto::Official),
            thumbnail: Some("cover.png".into()),
        }],
    };
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["stories"][0]["title"], "Le Loup");
    assert_eq!(v["stories"][0]["titleSource"], "official");
    assert_eq!(v["stories"][0]["thumbnail"], "cover.png");
}

#[test]
fn pack_title_source_tokens_are_stable_lowercase() {
    assert_eq!(
        serde_json::to_value(PackTitleSourceDto::User).expect("ser"),
        serde_json::json!("user")
    );
    assert_eq!(
        serde_json::to_value(PackTitleSourceDto::Official).expect("ser"),
        serde_json::json!("official")
    );
    assert_eq!(
        serde_json::to_value(PackTitleSourceDto::Unofficial).expect("ser"),
        serde_json::json!("unofficial")
    );
}

#[test]
fn none_variant_does_not_emit_extra_fields() {
    let v = serde_json::to_value(DeviceLibraryDto::None).expect("ser");
    let obj = v.as_object().expect("object");
    assert_eq!(obj.len(), 1);
    assert!(obj.contains_key("kind"));
}
