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

/// Input accepted by the `record_draft` Tauri command. Same
/// `deny_unknown_fields` discipline as the other story commands so a
/// drifting frontend payload fails at the boundary.
///
/// `draftTitle` may be empty (the user erased everything) and may carry
/// characters that would fail `validate_title` — re-validation only kicks
/// in at apply time, never at record time.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordDraftInputDto {
    pub story_id: String,
    pub draft_title: String,
}

/// Input accepted by the `apply_recovery` Tauri command.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyRecoveryInputDto {
    pub story_id: String,
}

/// Input accepted by the `discard_draft` Tauri command. The optional
/// `expected_draft_at` is forwarded to the application service as a
/// compare-and-swap guard: when present, the DELETE only consumes the
/// row whose `draft_at` matches the value the UI had observed, so a
/// concurrent `record_draft` that refreshed the row between read and
/// click is not silently dropped. When absent (legacy code path,
/// auto-discard from the autosave flow), the DELETE runs
/// unconditionally — that path explicitly accepts the trade-off.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscardDraftInputDto {
    pub story_id: String,
    pub expected_draft_at: Option<String>,
}

/// Wire-level outcome returned by `read_recoverable_draft`.
///
/// Tagged enum (`kind` discriminator) over `none` and `recoverable` so
/// the UI never has to read a `null` and decide what to do — a missing
/// row is an explicit informational state, not an error.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RecoverableDraftDto {
    None,
    Recoverable {
        #[serde(rename = "storyId")]
        story_id: String,
        #[serde(rename = "draftTitle")]
        draft_title: String,
        #[serde(rename = "draftAt")]
        draft_at: String,
        #[serde(rename = "persistedTitle")]
        persisted_title: String,
    },
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

    // ------ RecordDraftInputDto ------

    #[test]
    fn record_draft_input_accepts_canonical_camel_case() {
        let dto: RecordDraftInputDto = serde_json::from_value(serde_json::json!({
            "storyId": "0197a5d0-0000-7000-8000-000000000000",
            "draftTitle": "Live keystroke",
        }))
        .expect("deser");
        assert_eq!(dto.story_id, "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(dto.draft_title, "Live keystroke");
    }

    #[test]
    fn record_draft_input_rejects_unknown_field() {
        let err = serde_json::from_value::<RecordDraftInputDto>(serde_json::json!({
            "storyId": "x",
            "draftTitle": "y",
            "extra": 1,
        }))
        .expect_err("must reject");
        assert!(err.to_string().contains("extra"));
    }

    #[test]
    fn record_draft_input_rejects_snake_case_story_id() {
        let err = serde_json::from_value::<RecordDraftInputDto>(serde_json::json!({
            "story_id": "x",
            "draftTitle": "y",
        }))
        .expect_err("must reject snake_case field");
        let message = err.to_string().to_lowercase();
        assert!(
            message.contains("story_id") || message.contains("unknown field"),
            "expected unknown-field error, got: {message}"
        );
    }

    #[test]
    fn record_draft_input_rejects_missing_story_id() {
        serde_json::from_value::<RecordDraftInputDto>(serde_json::json!({
            "draftTitle": "y",
        }))
        .expect_err("must reject");
    }

    #[test]
    fn record_draft_input_rejects_missing_draft_title() {
        serde_json::from_value::<RecordDraftInputDto>(serde_json::json!({
            "storyId": "x",
        }))
        .expect_err("must reject");
    }

    #[test]
    fn record_draft_input_accepts_empty_draft_title() {
        // Empty value is meaningful: the user erased everything. Wire
        // shape must accept it; the application service is the layer
        // that decides what to do with it.
        let dto: RecordDraftInputDto = serde_json::from_value(serde_json::json!({
            "storyId": "x",
            "draftTitle": "",
        }))
        .expect("empty must be accepted");
        assert_eq!(dto.draft_title, "");
    }

    // ------ ApplyRecoveryInputDto ------

    #[test]
    fn apply_recovery_input_accepts_canonical_camel_case() {
        let dto: ApplyRecoveryInputDto = serde_json::from_value(serde_json::json!({
            "storyId": "abc",
        }))
        .expect("deser");
        assert_eq!(dto.story_id, "abc");
    }

    #[test]
    fn apply_recovery_input_rejects_unknown_field() {
        serde_json::from_value::<ApplyRecoveryInputDto>(serde_json::json!({
            "storyId": "x",
            "force": true,
        }))
        .expect_err("must reject");
    }

    // ------ RecoverableDraftDto ------

    #[test]
    fn recoverable_draft_dto_none_serializes_with_kind_discriminator() {
        let v = serde_json::to_value(&RecoverableDraftDto::None).expect("serialize");
        assert_eq!(v, serde_json::json!({ "kind": "none" }));
    }

    #[test]
    fn recoverable_draft_dto_recoverable_serializes_in_camel_case() {
        let v = serde_json::to_value(&RecoverableDraftDto::Recoverable {
            story_id: "sid".into(),
            draft_title: "Buffered".into(),
            draft_at: "2026-04-25T12:00:00.000Z".into(),
            persisted_title: "Saved".into(),
        })
        .expect("serialize");
        assert_eq!(v["kind"], "recoverable");
        assert_eq!(v["storyId"], "sid");
        assert_eq!(v["draftTitle"], "Buffered");
        assert_eq!(v["draftAt"], "2026-04-25T12:00:00.000Z");
        assert_eq!(v["persistedTitle"], "Saved");
        // snake_case must never leak
        for snake in ["story_id", "draft_title", "draft_at", "persisted_title"] {
            assert!(v.get(snake).is_none(), "{snake} must be camelCase");
        }
    }

    #[test]
    fn recoverable_draft_dto_recoverable_carries_persisted_title_byte_for_byte() {
        // The wire passes both titles verbatim — no NFC, no trim, no
        // length cap on this specific surface. The UI is the consumer
        // and must show what the user actually had.
        let v = serde_json::to_value(&RecoverableDraftDto::Recoverable {
            story_id: "sid".into(),
            draft_title: "  spaces  ".into(),
            draft_at: "2026-04-25T12:00:00.000Z".into(),
            persisted_title: "  Persisted  ".into(),
        })
        .expect("serialize");
        assert_eq!(v["draftTitle"], "  spaces  ");
        assert_eq!(v["persistedTitle"], "  Persisted  ");
    }
}
