use serde::{Deserialize, Serialize};

/// Input accepted by the `create_story` Tauri command. `deny_unknown_fields`
/// fails the deserialization if the UI ever adds a field ahead of the
/// contract; the wire shape stays under Rust authority, even during
/// boundary evolution.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateStoryInputDto {
    pub title: String,
}

/// Input accepted by the `update_story` Tauri command. Same
/// `deny_unknown_fields` discipline as `CreateStoryInputDto` so a stray
/// frontend field breaks at the boundary rather than at write time.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateStoryInputDto {
    pub id: String,
    pub title: String,
}

/// Wire-level return shape for `update_story`. Carries the freshly
/// persisted values so the UI can reconcile its draft against the source of
/// truth without issuing a second read.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStoryOutputDto {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

/// Full projection of a single story used by the edit surface. Mirrors the
/// columns of the `stories` table, minus any columns the UI has no business
/// reading. `structureJson` is forwarded as a string — its canonical bytes
/// are what the `contentChecksum` covers, so the UI must never reserialize
/// or reformat it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryDetailDto {
    pub id: String,
    pub title: String,
    pub schema_version: u32,
    pub structure_json: String,
    pub content_checksum: String,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------ CreateStoryInputDto ------

    #[test]
    fn accepts_canonical_payload() {
        let dto: CreateStoryInputDto =
            serde_json::from_value(serde_json::json!({ "title": "Un titre" })).expect("deser");
        assert_eq!(dto.title, "Un titre");
    }

    #[test]
    fn rejects_unknown_field() {
        let err = serde_json::from_value::<CreateStoryInputDto>(
            serde_json::json!({ "title": "x", "description": "y" }),
        )
        .expect_err("must reject");
        assert!(err.to_string().contains("description"));
    }

    #[test]
    fn rejects_missing_title() {
        let err = serde_json::from_value::<CreateStoryInputDto>(serde_json::json!({}))
            .expect_err("must reject");
        assert!(err.to_string().to_lowercase().contains("title"));
    }

    // ------ UpdateStoryInputDto ------

    #[test]
    fn update_story_input_accepts_canonical_payload() {
        let dto: UpdateStoryInputDto = serde_json::from_value(
            serde_json::json!({ "id": "0197a5d0-0000-7000-8000-000000000000", "title": "Nouveau titre" }),
        )
        .expect("deser");
        assert_eq!(dto.id, "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(dto.title, "Nouveau titre");
    }

    #[test]
    fn update_story_input_rejects_unknown_field() {
        let err = serde_json::from_value::<UpdateStoryInputDto>(
            serde_json::json!({ "id": "x", "title": "y", "extra": "z" }),
        )
        .expect_err("must reject unknown field");
        assert!(err.to_string().contains("extra"));
    }

    #[test]
    fn update_story_input_rejects_snake_case_id() {
        // Proof the wire expects `id`, not `story_id` — a frontend that
        // drifts to snake_case will break at the boundary, not silently.
        let err = serde_json::from_value::<UpdateStoryInputDto>(
            serde_json::json!({ "story_id": "x", "title": "y" }),
        )
        .expect_err("must reject snake_case field");
        let message = err.to_string().to_lowercase();
        assert!(
            message.contains("story_id") || message.contains("unknown field"),
            "expected unknown-field error, got: {message}"
        );
    }

    #[test]
    fn update_story_input_rejects_missing_id() {
        serde_json::from_value::<UpdateStoryInputDto>(serde_json::json!({ "title": "x" }))
            .expect_err("must reject");
    }

    #[test]
    fn update_story_input_rejects_missing_title() {
        serde_json::from_value::<UpdateStoryInputDto>(serde_json::json!({ "id": "x" }))
            .expect_err("must reject");
    }

    // ------ UpdateStoryOutputDto ------

    #[test]
    fn update_story_output_serializes_in_camel_case() {
        let dto = UpdateStoryOutputDto {
            id: "sid".into(),
            title: "Titre".into(),
            updated_at: "2026-04-23T10:00:00.000Z".into(),
        };
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(
            v,
            serde_json::json!({
                "id": "sid",
                "title": "Titre",
                "updatedAt": "2026-04-23T10:00:00.000Z",
            })
        );
        assert!(v.get("updated_at").is_none());
    }

    // ------ StoryDetailDto ------

    #[test]
    fn story_detail_serializes_in_camel_case_with_all_fields() {
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
        // snake_case must never leak
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
    fn optional_story_detail_none_serializes_as_null() {
        let none: Option<StoryDetailDto> = None;
        let v = serde_json::to_value(&none).expect("serialize");
        assert_eq!(v, serde_json::Value::Null);
    }
}
