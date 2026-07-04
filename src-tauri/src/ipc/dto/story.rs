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
///
/// `editable` says whether the story may be edited (native stories) or is
/// projected read-only (imported stories — their declared edit scope is a
/// later iteration). `structure` is the LIGHT node graph PROJECTED BY RUST for
/// the structure navigator; `node` is the SELECTED node's full content (the
/// start node by default). The UI consumes both projections and never
/// recomposes anything from `structureJson`. Both are `None` when a BLOCKING
/// canonical issue prevents projecting (corrupt / drifted), which the UI
/// renders as the named degraded state; a FIXABLE issue (a broken option
/// link) keeps them projected so the flagged spot stays visible and
/// repairable.
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
    pub editable: bool,
    pub structure: Option<StoryStructureDto>,
    pub node: Option<NodeContentDto>,
}

/// The story's node graph, projected LIGHT for the structure navigator: the
/// start node id plus the ordered node list (order = canonical `nodes[]`
/// order — the display / navigation order). No text, no media resolution
/// (that stays on the selected node's `NodeContentDto`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryStructureDto {
    pub start_node_id: String,
    pub nodes: Vec<NodeGraphDto>,
}

/// One node of the projected graph. `is_start` marks the entry point;
/// `has_issue` is true when at least one of the node's options points at a
/// node absent from the graph (a localized, repairable issue — the navigator
/// flags the entry without hiding the rest).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeGraphDto {
    pub id: String,
    pub label: String,
    pub is_start: bool,
    pub has_issue: bool,
    pub options: Vec<OptionLinkDto>,
}

/// One option of a node, with its link state DERIVED BY RUST — the frontend
/// never re-derives it from `target`:
/// `target = None` → `unlinked`; `target` present in the graph → `linked`;
/// `target` ABSENT from the graph → `broken` (rendered as `destination à
/// corriger`, never prose containing "broken").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionLinkDto {
    pub label: String,
    pub target: Option<String>,
    pub state: String,
}

/// Wire outcome of every STRUCTURAL write (`add_story_node`,
/// `delete_story_node`, `move_story_node`, `add_node_option`,
/// `set_node_option_link`, `remove_node_option`). Carries the freshly
/// committed `updatedAt` / `contentChecksum`, the EXACT committed
/// `structureJson` bytes (the contract says those bytes are what the
/// checksum covers — the local detail must never hold a stale pair), and
/// the RE-PROJECTED graph so the UI reconciles from Rust's truth without a
/// follow-up read.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureWriteOutputDto {
    pub id: String,
    pub updated_at: String,
    pub content_checksum: String,
    pub structure_json: String,
    pub structure: StoryStructureDto,
}

/// The current node of a story, projected for the editor. Carries the stable
/// `id` (keeps the node identified across a long session), the `text` and
/// `label` fields, and a resolved state for each optional media slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeContentDto {
    pub id: String,
    pub text: String,
    pub label: String,
    pub image: Option<NodeMediaSlotDto>,
    pub audio: Option<NodeMediaSlotDto>,
}

/// A resolved node media slot. `state` is `ready` (the asset bytes are
/// present) or `attention` (the node references an asset whose source can no
/// longer be resolved — repairable, the rest of the node stays editable).
/// `format` / `byteSize` are present only when `ready`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMediaSlotDto {
    pub asset_id: String,
    pub media_type: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_size: Option<u64>,
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

/// Input for `update_node_content`. The node `text` / `label` are free-form
/// (may be empty); `deny_unknown_fields` keeps the wire under Rust authority.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateNodeContentInputDto {
    pub story_id: String,
    pub node_id: String,
    pub text: String,
    pub label: String,
}

/// Input for `attach_node_media` / `remove_node_media`. `slot` is the target
/// media slot — `image` or `audio`. For `attach`, the file itself is chosen
/// through a native dialog opened by the command, never carried on the wire.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeMediaSlotInputDto {
    pub story_id: String,
    pub node_id: String,
    pub slot: String,
}

/// Wire outcome of every node write (`update_node_content`, `attach_node_media`,
/// `remove_node_media`). Carries the freshly committed `updatedAt` /
/// `contentChecksum` and the RE-PROJECTED node so the UI reconciles without a
/// follow-up read.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeWriteOutputDto {
    pub id: String,
    pub updated_at: String,
    pub content_checksum: String,
    pub node: NodeContentDto,
}

/// Attaching media can also report a typed validation result without an error
/// when the file is refused — but a refusal is surfaced as a `MEDIA_INVALID`
/// `AppError`, so the attach command resolves with [`NodeWriteOutputDto`] on
/// success only. This tagged outcome covers the cancelled-dialog case.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AttachNodeMediaOutcomeDto {
    Cancelled,
    // Boxed so the unit `Cancelled` variant does not inflate the enum to the
    // size of the (much larger) write output. `serde` sees through the `Box`,
    // so the wire shape is unchanged.
    Attached { output: Box<NodeWriteOutputDto> },
}

/// Wire bytes for a node-media preview (`read_node_media`). `dataUrl` is a
/// self-contained `data:` URL — the frontend never owns the raw file bytes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMediaPreviewDto {
    pub data_url: String,
}

/// Input for `record_node_draft` (NFR8 recovery buffer for node content).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordNodeDraftInputDto {
    pub story_id: String,
    pub node_id: String,
    pub draft_text: String,
    pub draft_label: String,
}

/// Input for `discard_node_draft`. Optional `expectedDraftAt` is a CAS guard
/// (mirrors the title discard) so a concurrent buffer refresh is preserved.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscardNodeDraftInputDto {
    pub story_id: String,
    pub expected_draft_at: Option<String>,
}

/// Input for `add_story_node`. `linkFrom` is the OPTIONAL atomic link-back:
/// when present, the newly created node becomes the destination of the
/// referenced option IN THE SAME transaction (the "create and link" gesture —
/// never an intermediate half-state).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddStoryNodeInputDto {
    pub story_id: String,
    pub link_from: Option<OptionRefDto>,
}

/// A reference to one option of one node (used by `linkFrom`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptionRefDto {
    pub node_id: String,
    pub option_index: usize,
}

/// Input for `delete_story_node`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteStoryNodeInputDto {
    pub story_id: String,
    pub node_id: String,
}

/// Input for `move_story_node` — swap the node with its neighbor in the
/// DISPLAY order (`nodes[]`); the start node is designated by `startNodeId`,
/// not by position, so moving it is legitimate.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveStoryNodeInputDto {
    pub story_id: String,
    pub node_id: String,
    pub direction: MoveDirectionDto,
}

/// Direction of a node move.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MoveDirectionDto {
    Up,
    Down,
}

/// Input for `add_node_option` — the option's label is typed at creation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddNodeOptionInputDto {
    pub story_id: String,
    pub node_id: String,
    pub label: String,
}

/// Input for `set_node_option_link`. `target = Some(id)` links the option to
/// an EXISTING node (a missing destination is refused, never written);
/// `target = None` unlinks it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetNodeOptionLinkInputDto {
    pub story_id: String,
    pub node_id: String,
    pub option_index: usize,
    pub target: Option<String>,
}

/// Input for `remove_node_option`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveNodeOptionInputDto {
    pub story_id: String,
    pub node_id: String,
    pub option_index: usize,
}

/// Wire outcome of `read_recoverable_node_draft`. Tagged union over `none`
/// and `recoverable`, mirroring `RecoverableDraftDto` for the title.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RecoverableNodeDraftDto {
    None,
    Recoverable {
        #[serde(rename = "storyId")]
        story_id: String,
        #[serde(rename = "nodeId")]
        node_id: String,
        #[serde(rename = "draftText")]
        draft_text: String,
        #[serde(rename = "draftLabel")]
        draft_label: String,
        #[serde(rename = "draftAt")]
        draft_at: String,
        #[serde(rename = "persistedText")]
        persisted_text: String,
        #[serde(rename = "persistedLabel")]
        persisted_label: String,
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
            schema_version: 3,
            structure_json: "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[]}".into(),
            content_checksum: "0".repeat(64),
            created_at: "2026-04-23T09:00:00.000Z".into(),
            updated_at: "2026-04-23T10:00:00.000Z".into(),
            editable: true,
            structure: Some(StoryStructureDto {
                start_node_id: "n1".into(),
                nodes: vec![NodeGraphDto {
                    id: "n1".into(),
                    label: "Début".into(),
                    is_start: true,
                    has_issue: false,
                    options: vec![],
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
        assert_eq!(v["structure"]["startNodeId"], "n1");
        assert_eq!(v["structure"]["nodes"][0]["id"], "n1");
        assert_eq!(v["structure"]["nodes"][0]["isStart"], true);
        assert_eq!(v["node"]["id"], "n1");
        assert_eq!(v["node"]["text"], "Bonjour");
        assert_eq!(v["node"]["label"], "Début");
        assert_eq!(v["node"]["image"]["assetId"], "a1");
        assert_eq!(v["node"]["image"]["mediaType"], "image");
        assert_eq!(v["node"]["image"]["state"], "ready");
        assert_eq!(v["node"]["image"]["format"], "png");
        assert_eq!(v["node"]["image"]["byteSize"], 42);
        assert!(v["node"]["audio"].is_null());
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
    fn node_media_slot_attention_omits_format_and_size() {
        // An attention slot (source missing) carries no format / byteSize.
        let slot = NodeMediaSlotDto {
            asset_id: "a1".into(),
            media_type: "audio".into(),
            state: "attention".into(),
            format: None,
            byte_size: None,
        };
        let v = serde_json::to_value(&slot).expect("serialize");
        assert_eq!(v["state"], "attention");
        assert!(v.get("format").is_none(), "format omitted when absent");
        assert!(v.get("byteSize").is_none(), "byteSize omitted when absent");
    }

    #[test]
    fn update_node_content_input_rejects_unknown_field() {
        let err = serde_json::from_value::<UpdateNodeContentInputDto>(serde_json::json!({
            "storyId": "s", "nodeId": "n1", "text": "t", "label": "l", "extra": 1,
        }))
        .expect_err("must reject");
        assert!(err.to_string().contains("extra"));
    }

    #[test]
    fn node_media_slot_input_accepts_canonical_camel_case() {
        let dto: NodeMediaSlotInputDto = serde_json::from_value(serde_json::json!({
            "storyId": "s", "nodeId": "n1", "slot": "image",
        }))
        .expect("deser");
        assert_eq!(dto.slot, "image");
    }

    #[test]
    fn recoverable_node_draft_recoverable_serializes_in_camel_case() {
        let v = serde_json::to_value(&RecoverableNodeDraftDto::Recoverable {
            story_id: "s".into(),
            node_id: "n1".into(),
            draft_text: "buf".into(),
            draft_label: "lab".into(),
            draft_at: "2026-06-27T12:00:00.000Z".into(),
            persisted_text: "saved".into(),
            persisted_label: "savedlab".into(),
        })
        .expect("serialize");
        assert_eq!(v["kind"], "recoverable");
        assert_eq!(v["storyId"], "s");
        assert_eq!(v["nodeId"], "n1");
        assert_eq!(v["draftText"], "buf");
        assert_eq!(v["persistedText"], "saved");
        for snake in ["story_id", "node_id", "draft_text", "persisted_text"] {
            assert!(v.get(snake).is_none(), "{snake} must be camelCase");
        }
    }

    #[test]
    fn attach_node_media_outcome_cancelled_serializes_with_kind() {
        let v = serde_json::to_value(&AttachNodeMediaOutcomeDto::Cancelled).expect("serialize");
        assert_eq!(v, serde_json::json!({ "kind": "cancelled" }));
    }

    // ------ StoryStructureDto / NodeGraphDto / OptionLinkDto ------

    #[test]
    fn story_structure_serializes_in_camel_case_with_derived_states() {
        let dto = StoryStructureDto {
            start_node_id: "n1".into(),
            nodes: vec![NodeGraphDto {
                id: "n1".into(),
                label: "Début".into(),
                is_start: true,
                has_issue: true,
                options: vec![
                    OptionLinkDto {
                        label: "Continuer".into(),
                        target: Some("n2".into()),
                        state: "linked".into(),
                    },
                    OptionLinkDto {
                        label: "Plus tard".into(),
                        target: None,
                        state: "unlinked".into(),
                    },
                    OptionLinkDto {
                        label: "Perdu".into(),
                        target: Some("ghost".into()),
                        state: "broken".into(),
                    },
                ],
            }],
        };
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(v["startNodeId"], "n1");
        assert_eq!(v["nodes"][0]["id"], "n1");
        assert_eq!(v["nodes"][0]["isStart"], true);
        assert_eq!(v["nodes"][0]["hasIssue"], true);
        assert_eq!(v["nodes"][0]["options"][0]["state"], "linked");
        assert_eq!(v["nodes"][0]["options"][0]["target"], "n2");
        assert_eq!(v["nodes"][0]["options"][1]["state"], "unlinked");
        // An unlinked option's target is an EXPLICIT null, never an absent key
        // (the TS guard checks the state↔target coherence).
        assert!(v["nodes"][0]["options"][1]
            .as_object()
            .expect("option obj")
            .contains_key("target"));
        assert!(v["nodes"][0]["options"][1]["target"].is_null());
        assert_eq!(v["nodes"][0]["options"][2]["state"], "broken");
        for snake in ["start_node_id", "is_start", "has_issue"] {
            assert!(v.get(snake).is_none(), "{snake} must be camelCase");
        }
    }

    #[test]
    fn story_detail_structure_is_a_required_key_even_when_null() {
        // The `structure` field must serialize as an explicit null (not an
        // absent key) so the TS mirror can require the key.
        let dto = StoryDetailDto {
            id: "sid".into(),
            title: "Titre".into(),
            schema_version: 3,
            structure_json: "{}".into(),
            content_checksum: "0".repeat(64),
            created_at: "2026-07-04T09:00:00.000Z".into(),
            updated_at: "2026-07-04T10:00:00.000Z".into(),
            editable: false,
            structure: None,
            node: None,
        };
        let v = serde_json::to_value(&dto).expect("serialize");
        assert!(v.as_object().expect("obj").contains_key("structure"));
        assert!(v["structure"].is_null());
    }

    #[test]
    fn structure_write_output_serializes_in_camel_case() {
        let dto = StructureWriteOutputDto {
            id: "sid".into(),
            updated_at: "2026-07-04T10:00:00.000Z".into(),
            content_checksum: "0".repeat(64),
            structure_json: "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[]}".into(),
            structure: StoryStructureDto {
                start_node_id: "n1".into(),
                nodes: vec![],
            },
        };
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(v["id"], "sid");
        assert_eq!(v["updatedAt"], "2026-07-04T10:00:00.000Z");
        assert_eq!(v["contentChecksum"].as_str().expect("str").len(), 64);
        assert_eq!(
            v["structureJson"],
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[]}"
        );
        assert_eq!(v["structure"]["startNodeId"], "n1");
        assert!(v.get("updated_at").is_none());
        assert!(v.get("content_checksum").is_none());
        assert!(v.get("structure_json").is_none());
    }

    // ------ Structural mutation inputs ------

    #[test]
    fn add_story_node_input_accepts_optional_link_from() {
        let bare: AddStoryNodeInputDto =
            serde_json::from_value(serde_json::json!({ "storyId": "s" })).expect("deser");
        assert!(bare.link_from.is_none());

        let linked: AddStoryNodeInputDto = serde_json::from_value(serde_json::json!({
            "storyId": "s",
            "linkFrom": { "nodeId": "n1", "optionIndex": 2 },
        }))
        .expect("deser");
        let link = linked.link_from.expect("linkFrom");
        assert_eq!(link.node_id, "n1");
        assert_eq!(link.option_index, 2);
    }

    #[test]
    fn add_story_node_input_rejects_unknown_field() {
        serde_json::from_value::<AddStoryNodeInputDto>(serde_json::json!({
            "storyId": "s", "position": 0,
        }))
        .expect_err("must reject unknown field");
    }

    #[test]
    fn move_story_node_input_parses_camel_case_directions() {
        for (wire, expected) in [
            ("up", MoveDirectionDto::Up),
            ("down", MoveDirectionDto::Down),
        ] {
            let dto: MoveStoryNodeInputDto = serde_json::from_value(serde_json::json!({
                "storyId": "s", "nodeId": "n1", "direction": wire,
            }))
            .expect("deser");
            assert_eq!(dto.direction, expected);
        }
        serde_json::from_value::<MoveStoryNodeInputDto>(serde_json::json!({
            "storyId": "s", "nodeId": "n1", "direction": "left",
        }))
        .expect_err("unknown direction must be rejected");
    }

    #[test]
    fn set_node_option_link_input_accepts_null_and_string_target() {
        let unlink: SetNodeOptionLinkInputDto = serde_json::from_value(serde_json::json!({
            "storyId": "s", "nodeId": "n1", "optionIndex": 0, "target": null,
        }))
        .expect("deser");
        assert!(unlink.target.is_none());

        let link: SetNodeOptionLinkInputDto = serde_json::from_value(serde_json::json!({
            "storyId": "s", "nodeId": "n1", "optionIndex": 1, "target": "n2",
        }))
        .expect("deser");
        assert_eq!(link.target.as_deref(), Some("n2"));
        assert_eq!(link.option_index, 1);
    }

    #[test]
    fn remove_node_option_input_rejects_snake_case() {
        serde_json::from_value::<RemoveNodeOptionInputDto>(serde_json::json!({
            "storyId": "s", "nodeId": "n1", "option_index": 0,
        }))
        .expect_err("must reject snake_case field");
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
