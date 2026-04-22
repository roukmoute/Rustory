use serde::Deserialize;

/// Input accepted by the `create_story` Tauri command. `deny_unknown_fields`
/// fails the deserialization if the UI ever adds a field ahead of the
/// contract; the wire shape stays under Rust authority, even during
/// boundary evolution.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateStoryInputDto {
    pub title: String,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
