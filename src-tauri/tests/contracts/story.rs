use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::CreateStoryInputDto;

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
