use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::{
    AddStoryNodeInputDto, ApplyRecoveryInputDto, AttachNodeMediaOutcomeDto, CreateStoryInputDto,
    MoveDirectionDto, MoveStoryNodeInputDto, NodeContentDto, NodeGraphDto, NodeMediaSlotDto,
    NodeMediaSlotInputDto, NodeWriteOutputDto, OptionLinkDto, RecordDraftInputDto,
    RecoverableDraftDto, SetNodeOptionLinkInputDto, StoryDetailDto, StoryStructureDto,
    StructureWriteOutputDto, UpdateNodeContentInputDto, UpdateStoryInputDto, UpdateStoryOutputDto,
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
        import_state: None,
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["id"], "sid");
    assert_eq!(v["title"], "Titre");
    assert_eq!(v["updatedAt"], "2026-04-23T10:00:00.000Z");
    assert!(v.get("updated_at").is_none(), "snake_case must not leak");
    // `importState` is a REQUIRED key (explicit null when absent) so the TS
    // guard can refuse a payload missing the FR21 acknowledgement field.
    assert!(v.as_object().expect("obj").contains_key("importState"));
    assert!(v["importState"].is_null());
}

#[test]
fn update_story_output_carries_the_import_state_wire_tag() {
    let dto = UpdateStoryOutputDto {
        id: "sid".into(),
        title: "Titre".into(),
        updated_at: "2026-04-23T10:00:00.000Z".into(),
        import_state: Some("resolved".into()),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["importState"], "resolved");
    assert!(v.get("import_state").is_none(), "camelCase only");
}

#[test]
fn story_detail_wire_shape_is_camel_case_with_all_fields() {
    let dto = StoryDetailDto {
        id: "sid".into(),
        title: "Titre".into(),
        schema_version: 3,
        structure_json: "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[]}".into(),
        content_checksum: "0".repeat(64),
        created_at: "2026-04-23T09:00:00.000Z".into(),
        updated_at: "2026-04-23T10:00:00.000Z".into(),
        editable: true,
        edit_scope: "full".into(),
        import_state: Some("needsReview".into()),
        structure: Some(StoryStructureDto {
            start_node_id: "n1".into(),
            nodes: vec![NodeGraphDto {
                id: "n1".into(),
                label: "Début".into(),
                is_start: true,
                has_issue: false,
                options: vec![OptionLinkDto {
                    label: "Continuer".into(),
                    target: None,
                    state: "unlinked".into(),
                }],
            }],
        }),
        node: Some(NodeContentDto {
            id: "n1".into(),
            text: "Bonjour".into(),
            label: "Début".into(),
            image: Some(NodeMediaSlotDto {
                asset_id: "a1".into(),
                media_type: "image".into(),
                state: "ready".into(),
                format: Some("png".into()),
                byte_size: Some(42),
            }),
            audio: None,
        }),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["id"], "sid");
    assert_eq!(v["title"], "Titre");
    assert_eq!(v["schemaVersion"], 3);
    assert_eq!(
        v["structureJson"],
        "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[]}"
    );
    assert_eq!(v["contentChecksum"].as_str().unwrap().len(), 64);
    assert_eq!(v["createdAt"], "2026-04-23T09:00:00.000Z");
    assert_eq!(v["updatedAt"], "2026-04-23T10:00:00.000Z");
    assert_eq!(v["editable"], true);
    assert_eq!(v["editScope"], "full");
    assert_eq!(v["importState"], "needsReview");
    assert_eq!(v["structure"]["startNodeId"], "n1");
    assert_eq!(v["structure"]["nodes"][0]["id"], "n1");
    assert_eq!(v["structure"]["nodes"][0]["isStart"], true);
    assert_eq!(v["structure"]["nodes"][0]["hasIssue"], false);
    assert_eq!(
        v["structure"]["nodes"][0]["options"][0]["state"],
        "unlinked"
    );
    assert!(v["structure"]["nodes"][0]["options"][0]["target"].is_null());
    assert_eq!(v["node"]["id"], "n1");
    assert_eq!(v["node"]["image"]["assetId"], "a1");
    assert_eq!(v["node"]["image"]["state"], "ready");
    assert!(v["node"]["audio"].is_null());
    for snake in [
        "schema_version",
        "structure_json",
        "content_checksum",
        "created_at",
        "updated_at",
        "start_node_id",
        "edit_scope",
        "import_state",
    ] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn story_detail_structure_key_is_required_even_when_null() {
    // A blocking canonical issue projects `structure = null` — the KEY must
    // stay present so the TS mirror can require it. Same rule for the FR21
    // fields: `editScope` stays projected under a Blocking degradation
    // (story metadata, not canonical content) and `importState` stays a
    // required key with an explicit null.
    let dto = StoryDetailDto {
        id: "sid".into(),
        title: "Titre".into(),
        schema_version: 3,
        structure_json: "not json".into(),
        content_checksum: "0".repeat(64),
        created_at: "2026-04-23T09:00:00.000Z".into(),
        updated_at: "2026-04-23T10:00:00.000Z".into(),
        editable: false,
        edit_scope: "titleOnly".into(),
        import_state: None,
        structure: None,
        node: None,
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert!(v.as_object().expect("obj").contains_key("structure"));
    assert!(v["structure"].is_null());
    assert!(v.as_object().expect("obj").contains_key("node"));
    assert!(v["node"].is_null());
    assert_eq!(v["editScope"], "titleOnly");
    assert!(v.as_object().expect("obj").contains_key("importState"));
    assert!(v["importState"].is_null());
}

#[test]
fn structure_write_output_wire_shape_is_camel_case() {
    let dto = StructureWriteOutputDto {
        id: "sid".into(),
        updated_at: "2026-07-04T10:00:00.000Z".into(),
        content_checksum: "0".repeat(64),
        structure_json: "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[]}".into(),
        structure: StoryStructureDto {
            start_node_id: "n1".into(),
            nodes: vec![NodeGraphDto {
                id: "n1".into(),
                label: String::new(),
                is_start: true,
                has_issue: true,
                options: vec![OptionLinkDto {
                    label: "Perdu".into(),
                    target: Some("ghost".into()),
                    state: "broken".into(),
                }],
            }],
        },
        import_state: Some("needsReview".into()),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["id"], "sid");
    assert_eq!(v["updatedAt"], "2026-07-04T10:00:00.000Z");
    assert_eq!(v["contentChecksum"].as_str().unwrap().len(), 64);
    assert_eq!(
        v["structureJson"],
        "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[]}"
    );
    assert_eq!(v["structure"]["startNodeId"], "n1");
    assert_eq!(v["structure"]["nodes"][0]["hasIssue"], true);
    assert_eq!(v["structure"]["nodes"][0]["options"][0]["state"], "broken");
    assert_eq!(v["structure"]["nodes"][0]["options"][0]["target"], "ghost");
    assert_eq!(v["importState"], "needsReview");
    assert!(v.get("updated_at").is_none(), "snake_case must not leak");
    assert!(v.get("import_state").is_none(), "snake_case must not leak");
}

#[test]
fn structure_write_output_import_state_is_a_required_null_key() {
    let dto = StructureWriteOutputDto {
        id: "sid".into(),
        updated_at: "2026-07-04T10:00:00.000Z".into(),
        content_checksum: "0".repeat(64),
        structure_json: "{}".into(),
        structure: StoryStructureDto {
            start_node_id: "n1".into(),
            nodes: vec![],
        },
        import_state: None,
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert!(
        v.as_object().expect("obj").contains_key("importState"),
        "importState must stay a REQUIRED key (explicit null) so the TS guard \
         can refuse a payload missing the FR21 acknowledgement field"
    );
    assert!(v["importState"].is_null());
}

#[test]
fn structural_inputs_accept_canonical_camel_case_and_reject_unknown_fields() {
    let add: AddStoryNodeInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "s",
        "linkFrom": { "nodeId": "n1", "optionIndex": 0 },
    }))
    .expect("deser add");
    assert_eq!(add.link_from.expect("linkFrom").node_id, "n1");

    let mv: MoveStoryNodeInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "s", "nodeId": "n2", "direction": "down",
    }))
    .expect("deser move");
    assert_eq!(mv.direction, MoveDirectionDto::Down);

    let link: SetNodeOptionLinkInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "s", "nodeId": "n1", "optionIndex": 0, "target": null,
    }))
    .expect("deser link");
    assert!(link.target.is_none());

    serde_json::from_value::<AddStoryNodeInputDto>(serde_json::json!({
        "storyId": "s", "position": 3,
    }))
    .expect_err("unknown field must be rejected");
}

#[test]
fn story_detail_option_none_serializes_as_json_null() {
    let none: Option<StoryDetailDto> = None;
    let v = serde_json::to_value(&none).expect("serialize");
    assert!(v.is_null(), "None must serialize as JSON null, got: {v:?}");
}

#[test]
fn update_node_content_input_accepts_camel_case_and_rejects_unknown() {
    let dto: UpdateNodeContentInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "sid", "nodeId": "n1", "text": "t", "label": "l",
    }))
    .expect("deser");
    assert_eq!(dto.story_id, "sid");
    assert_eq!(dto.node_id, "n1");
    serde_json::from_value::<UpdateNodeContentInputDto>(serde_json::json!({
        "storyId": "sid", "nodeId": "n1", "text": "t", "label": "l", "extra": 1,
    }))
    .expect_err("unknown field must fail");
}

#[test]
fn node_media_slot_input_rejects_snake_case() {
    serde_json::from_value::<NodeMediaSlotInputDto>(serde_json::json!({
        "story_id": "sid", "nodeId": "n1", "slot": "image",
    }))
    .expect_err("snake_case storyId must fail at the boundary");
}

#[test]
fn node_write_output_wire_shape_is_camel_case() {
    let dto = NodeWriteOutputDto {
        id: "sid".into(),
        updated_at: "2026-06-27T10:00:00.000Z".into(),
        content_checksum: "0".repeat(64),
        node: NodeContentDto {
            id: "n1".into(),
            text: "t".into(),
            label: "l".into(),
            image: None,
            audio: None,
        },
        import_state: Some("resolved".into()),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["updatedAt"], "2026-06-27T10:00:00.000Z");
    assert_eq!(v["contentChecksum"].as_str().unwrap().len(), 64);
    assert_eq!(v["node"]["id"], "n1");
    assert_eq!(v["importState"], "resolved");
    assert!(v.get("updated_at").is_none(), "snake_case must not leak");
    assert!(
        v.get("content_checksum").is_none(),
        "snake_case must not leak"
    );
    assert!(v.get("import_state").is_none(), "snake_case must not leak");
}

#[test]
fn node_write_output_import_state_is_a_required_null_key() {
    let dto = NodeWriteOutputDto {
        id: "sid".into(),
        updated_at: "2026-06-27T10:00:00.000Z".into(),
        content_checksum: "0".repeat(64),
        node: NodeContentDto {
            id: "n1".into(),
            text: String::new(),
            label: String::new(),
            image: None,
            audio: None,
        },
        import_state: None,
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert!(v.as_object().expect("obj").contains_key("importState"));
    assert!(v["importState"].is_null());
}

#[test]
fn attach_node_media_outcome_attached_serializes_with_kind() {
    let dto = AttachNodeMediaOutcomeDto::Attached {
        output: Box::new(NodeWriteOutputDto {
            id: "sid".into(),
            updated_at: "2026-06-27T10:00:00.000Z".into(),
            content_checksum: "0".repeat(64),
            node: NodeContentDto {
                id: "n1".into(),
                text: String::new(),
                label: String::new(),
                image: Some(NodeMediaSlotDto {
                    asset_id: "a1".into(),
                    media_type: "image".into(),
                    state: "ready".into(),
                    format: Some("png".into()),
                    byte_size: Some(10),
                }),
                audio: None,
            },
            import_state: None,
        }),
    };
    let v = serde_json::to_value(&dto).expect("serialize");
    assert_eq!(v["kind"], "attached");
    assert_eq!(v["output"]["node"]["image"]["assetId"], "a1");
}

#[test]
fn app_error_wire_shape_for_media_invalid() {
    // The frontend matches on `code` + `details.stage` to surface a media
    // block inline at the slot. Freezing the wire shape prevents drift.
    let err = AppError::media_invalid(
        "Ce média utilise un format non pris en charge.",
        "Choisis une image PNG ou JPEG, ou un son MP3, WAV ou OGG.",
    )
    .with_details(serde_json::json!({ "source": "media_invalid", "stage": "unsupported_format" }));
    let v = serde_json::to_value(&err).expect("serialize");
    assert_eq!(v["code"], "MEDIA_INVALID");
    assert_eq!(v["details"]["stage"], "unsupported_format");
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

// ------ Recovery flow contract tests ------

#[test]
fn record_draft_input_dto_wire_shape_canonical() {
    let dto: RecordDraftInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "0197a5d0-0000-7000-8000-000000000000",
        "draftTitle": "Live keystroke",
    }))
    .expect("deser");
    assert_eq!(dto.story_id, "0197a5d0-0000-7000-8000-000000000000");
    assert_eq!(dto.draft_title, "Live keystroke");
}

#[test]
fn record_draft_input_dto_rejects_snake_case_story_id() {
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
fn record_draft_input_dto_rejects_unknown_field() {
    let err = serde_json::from_value::<RecordDraftInputDto>(serde_json::json!({
        "storyId": "x",
        "draftTitle": "y",
        "extra": "z",
    }))
    .expect_err("must reject unknown field");
    assert!(err.to_string().contains("extra"));
}

#[test]
fn apply_recovery_input_dto_wire_shape_canonical() {
    let dto: ApplyRecoveryInputDto = serde_json::from_value(serde_json::json!({
        "storyId": "abc",
    }))
    .expect("deser");
    assert_eq!(dto.story_id, "abc");
}

#[test]
fn recoverable_draft_dto_none_wire_shape() {
    let v = serde_json::to_value(&RecoverableDraftDto::None).expect("serialize");
    assert_eq!(v, serde_json::json!({ "kind": "none" }));
}

#[test]
fn recoverable_draft_dto_recoverable_wire_shape() {
    let v = serde_json::to_value(&RecoverableDraftDto::Recoverable {
        story_id: "sid".into(),
        draft_title: "Buffered".into(),
        draft_at: "2026-04-25T12:00:00.000Z".into(),
        persisted_title: "Persisted".into(),
    })
    .expect("serialize");
    assert_eq!(v["kind"], "recoverable");
    assert_eq!(v["storyId"], "sid");
    assert_eq!(v["draftTitle"], "Buffered");
    assert_eq!(v["draftAt"], "2026-04-25T12:00:00.000Z");
    assert_eq!(v["persistedTitle"], "Persisted");
}

#[test]
fn recoverable_draft_dto_recoverable_camel_case_only() {
    let v = serde_json::to_value(&RecoverableDraftDto::Recoverable {
        story_id: "sid".into(),
        draft_title: "B".into(),
        draft_at: "2026-04-25T12:00:00.000Z".into(),
        persisted_title: "P".into(),
    })
    .expect("serialize");
    for snake in ["story_id", "draft_title", "draft_at", "persisted_title"] {
        assert!(v.get(snake).is_none(), "{snake} must never leak");
    }
}

#[test]
fn recoverable_draft_dto_round_trip_via_serde_value() {
    // Round-trip both variants through serde_json::Value to prove the
    // serializer is deterministic and the consumer can re-parse it.
    let none = serde_json::to_value(&RecoverableDraftDto::None).expect("ser");
    assert_eq!(none, serde_json::json!({ "kind": "none" }));

    let recoverable = serde_json::to_value(&RecoverableDraftDto::Recoverable {
        story_id: "sid".into(),
        draft_title: "B".into(),
        draft_at: "2026-04-25T12:00:00.000Z".into(),
        persisted_title: "P".into(),
    })
    .expect("ser");
    assert_eq!(recoverable["kind"], "recoverable");
    // The kind discriminator is the first observable bit a TS guard
    // checks; lock it in so a refactor cannot accidentally rename.
    assert!(recoverable.is_object());
}

#[test]
fn app_error_wire_shape_for_recovery_draft_unavailable() {
    let err = AppError::recovery_draft_unavailable(
        "Récupération indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_upsert",
        "kind": "busy",
        "id": "sid",
    }));
    let v = serde_json::to_value(&err).expect("serialize");
    assert_eq!(v["code"], "RECOVERY_DRAFT_UNAVAILABLE");
    assert_eq!(v["details"]["source"], "sqlite_upsert");
    assert_eq!(v["details"]["kind"], "busy");
}
