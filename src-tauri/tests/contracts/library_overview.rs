use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::{LibraryOverviewDto, StoryCardDto};

#[test]
fn library_overview_empty_wire_shape() {
    let dto = LibraryOverviewDto::empty();
    let v = serde_json::to_value(&dto).expect("serialize overview");
    assert_eq!(v, serde_json::json!({ "stories": [] }));
}

#[test]
fn library_overview_with_stories_wire_shape() {
    let dto = LibraryOverviewDto {
        stories: vec![StoryCardDto {
            id: "story-1".into(),
            title: "Un titre".into(),
        }],
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
