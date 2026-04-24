use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::{
    CreateStoryInputDto, StoryDetailDto, UpdateStoryInputDto, UpdateStoryOutputDto,
};

#[test]
fn create_story_input_accepts_canonical_camel_case_payload() {
    let dto: CreateStoryInputDto =
        serde_json::from_value(serde_json::json!({ "title": "Un titre valide" })).expect("deser");
    assert_eq!(dto.title, "Un titre valide");
}

#[test]
fn create_story_input_rejects_unknown_fields() {
    let err = serde_json::from_value::<CreateStoryInputDto>(
        serde_json::json!({ "title": "x", "description": "hidden" }),
    )
    .expect_err("must reject unknown field");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("description"),
        "expected unknown-field hint, got: {message}"
    );
}

#[test]
fn create_story_input_rejects_missing_title() {
    serde_json::from_value::<CreateStoryInputDto>(serde_json::json!({}))
        .expect_err("must reject missing title");
}

#[test]
fn app_error_wire_shape_for_invalid_story_title() {
    let err = AppError::invalid_story_title(
        "Création impossible: titre requis",
        "Saisis un titre non vide pour créer l'histoire.",
    );
    let v = serde_json::to_value(&err).expect("serialize");
    assert_eq!(v["code"], "INVALID_STORY_TITLE");
    assert_eq!(v["message"], "Création impossible: titre requis");
    assert_eq!(
        v["userAction"],
        "Saisis un titre non vide pour créer l'histoire."
    );
    assert!(
        v.get("user_action").is_none(),
        "snake_case must never leak across the boundary"
    );
    assert!(v["details"].is_null());
}

#[test]
fn update_story_input_accepts_canonical_camel_case_payload() {
    let dto: UpdateStoryInputDto = serde_json::from_value(serde_json::json!({
        "id": "0197a5d0-0000-7000-8000-000000000000",
        "title": "Titre modifié",
    }))
    .expect("deser");
    assert_eq!(dto.id, "0197a5d0-0000-7000-8000-000000000000");
    assert_eq!(dto.title, "Titre modifié");
}

#[test]
fn update_story_input_rejects_unknown_fields() {
    let err = serde_json::from_value::<UpdateStoryInputDto>(serde_json::json!({
        "id": "x",
        "title": "y",
        "extra": "z",
    }))
    .expect_err("must reject unknown field");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("extra"),
        "expected unknown-field hint, got: {message}"
    );
}

#[test]
fn update_story_input_rejects_snake_case_id_field() {
    let err = serde_json::from_value::<UpdateStoryInputDto>(serde_json::json!({
        "story_id": "x",
        "title": "y",
    }))
    .expect_err("must reject snake_case id");
    let message = err.to_string().to_lowercase();
    assert!(
        message.contains("story_id") || message.contains("missing field"),
        "expected snake_case rejection, got: {message}"
    );
}

#[test]
fn update_story_output_wire_shape_is_camel_case() {
    let dto = UpdateStoryOutputDto {
        id: "sid".into(),
        title: "Titre".into(),
        updated_at: "2026-04-23T10:00:00.000Z".into(),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["id"], "sid");
    assert_eq!(v["title"], "Titre");
    assert_eq!(v["updatedAt"], "2026-04-23T10:00:00.000Z");
    assert!(v.get("updated_at").is_none(), "snake_case must not leak");
}

#[test]
fn story_detail_wire_shape_is_camel_case_with_all_fields() {
    let dto = StoryDetailDto {
        id: "sid".into(),
        title: "Titre".into(),
        schema_version: 1,
        structure_json: "{\"schemaVersion\":1,\"nodes\":[]}".into(),
        content_checksum: "0".repeat(64),
        created_at: "2026-04-23T09:00:00.000Z".into(),
        updated_at: "2026-04-23T10:00:00.000Z".into(),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["id"], "sid");
    assert_eq!(v["title"], "Titre");
    assert_eq!(v["schemaVersion"], 1);
    assert_eq!(v["structureJson"], "{\"schemaVersion\":1,\"nodes\":[]}");
    assert_eq!(v["contentChecksum"].as_str().unwrap().len(), 64);
    assert_eq!(v["createdAt"], "2026-04-23T09:00:00.000Z");
    assert_eq!(v["updatedAt"], "2026-04-23T10:00:00.000Z");
    for snake in [
        "schema_version",
        "structure_json",
        "content_checksum",
        "created_at",
        "updated_at",
    ] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn story_detail_option_none_serializes_as_json_null() {
    let none: Option<StoryDetailDto> = None;
    let v = serde_json::to_value(&none).expect("serialize");
    assert!(v.is_null(), "None must serialize as JSON null, got: {v:?}");
}

#[test]
fn app_error_wire_shape_for_library_inconsistent_story_missing() {
    // The frontend matches on `code` + `details.source` to surface an
    // "Histoire introuvable" alert. Freezing the wire shape here prevents a
    // silent drift.
    let err = AppError::library_inconsistent(
        "Histoire introuvable, recharge la bibliothèque.",
        "Retourne à la bibliothèque et recharge la liste.",
    )
    .with_details(serde_json::json!({ "source": "story_missing", "id": "sid" }));
    let v = serde_json::to_value(&err).expect("serialize");
    assert_eq!(v["code"], "LIBRARY_INCONSISTENT");
    assert_eq!(v["details"]["source"], "story_missing");
    assert_eq!(v["details"]["id"], "sid");
}
