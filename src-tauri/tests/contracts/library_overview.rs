use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::{LibraryOverviewDto, SendBlockerDto, StoryCardDto};

#[test]
fn library_overview_empty_wire_shape() {
    let dto = LibraryOverviewDto::empty();
    let v = serde_json::to_value(&dto).expect("serialize overview");
    assert_eq!(v, serde_json::json!({ "stories": [] }));
}

#[test]
fn library_overview_with_stories_wire_shape() {
    let dto = LibraryOverviewDto {
        stories: vec![StoryCardDto::native("story-1".into(), "Un titre".into())],
    };
    let v = serde_json::to_value(&dto).expect("serialize overview");
    assert_eq!(
        v,
        serde_json::json!({
            "stories": [{ "id": "story-1", "title": "Un titre" }]
        })
    );
}

#[test]
fn library_overview_with_an_imported_story_carries_import_state() {
    use rustory_lib::ipc::dto::{
        ImportAspectDto, ImportCategoryDto, ImportFindingDto, ImportStateDto,
    };

    let dto = LibraryOverviewDto {
        stories: vec![StoryCardDto {
            id: "story-2".into(),
            title: "Histoire importée".into(),
            import_state: Some(ImportStateDto::NeedsReview),
            import_report: Some(vec![ImportFindingDto {
                aspect: ImportAspectDto::Title,
                category: ImportCategoryDto::Ambiguous,
                message: "Le titre a été normalisé à l'import (espaces ou caractères ajustés)."
                    .into(),
            }]),
            transferable: false,
            sendable: false,
            send_blocker: None,
            cover_asset_id: None,
        }],
    };
    let v = serde_json::to_value(&dto).expect("serialize overview");
    let card = &v["stories"][0];
    assert_eq!(card["id"], "story-2");
    assert_eq!(card["importState"], "needsReview");
    assert_eq!(card["importReport"][0]["aspect"], "title");
    assert_eq!(card["importReport"][0]["category"], "ambiguous");
    // A native card in the same payload stays `{ id, title }`.
    let native = serde_json::to_value(StoryCardDto::native("n".into(), "Native".into()))
        .expect("serialize native");
    assert!(native.get("importState").is_none());
}

#[test]
fn library_overview_send_readiness_wire_shape() {
    // Sendable: the flag alone. Blocked: the reason alone, in camelCase —
    // the closed set the frontend mirrors for its "Envoi indisponible" copy.
    let mut sendable = StoryCardDto::native("web".into(), "Web".into());
    sendable.sendable = true;
    let mut blocked = StoryCardDto::native("mute".into(), "Muette".into());
    blocked.send_blocker = Some(SendBlockerDto::MissingAudio);
    let dto = LibraryOverviewDto {
        stories: vec![
            sendable,
            blocked,
            StoryCardDto::device_pack("pack".into(), "Copiée".into()),
        ],
    };
    let v = serde_json::to_value(&dto).expect("serialize overview");
    assert_eq!(
        v["stories"][0],
        serde_json::json!({ "id": "web", "title": "Web", "sendable": true })
    );
    assert_eq!(
        v["stories"][1],
        serde_json::json!({ "id": "mute", "title": "Muette", "sendBlocker": "missingAudio" })
    );
    assert_eq!(
        v["stories"][2],
        serde_json::json!({
            "id": "pack", "title": "Copiée", "transferable": true, "sendBlocker": "devicePack"
        })
    );
    for (blocker, wire) in [
        (SendBlockerDto::DevicePack, "devicePack"),
        (SendBlockerDto::Empty, "empty"),
        (SendBlockerDto::Malformed, "malformed"),
        (SendBlockerDto::Branching, "branching"),
        (SendBlockerDto::MissingAudio, "missingAudio"),
    ] {
        assert_eq!(serde_json::to_value(blocker).expect("serialize"), wire);
    }
    assert!(
        v.to_string().find("sendable_archive").is_none()
            && v.to_string().find("send_blocker").is_none(),
        "snake_case must never leak across IPC"
    );
}

#[test]
fn app_error_wire_shape_for_local_storage_unavailable() {
    let err = AppError::local_storage_unavailable(
        "Le stockage local est inaccessible.",
        "Vérifie les permissions puis relance.",
    );

    let v = serde_json::to_value(&err).expect("serialize error");
    assert_eq!(v["code"], "LOCAL_STORAGE_UNAVAILABLE");
    assert_eq!(v["message"], "Le stockage local est inaccessible.");
    assert_eq!(v["userAction"], "Vérifie les permissions puis relance.");
    assert!(v["details"].is_null(), "details is null when absent");
    assert!(
        v.get("user_action").is_none(),
        "snake_case must never leak across IPC"
    );
}
