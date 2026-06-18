use serde::{Deserialize, Serialize};

use crate::application::device::transfer::TransferPreviewOutcome;

use super::device::{reason_dto, UnsupportedReasonDto};

/// Input accepted by the `read_transfer_preview` Tauri command.
/// `deny_unknown_fields` fails deserialization if the UI ever sends a field
/// ahead of the Rust contract, so the boundary stays authoritative.
///
/// Exactly the two identifiers the UI legitimately holds: the local
/// `story_id` (the selected story) and the opaque hashed `device_identifier`
/// from detection. No path, no pack short id — Rust re-resolves the rest.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadTransferPreviewInputDto {
    pub story_id: String,
    pub device_identifier: String,
}

/// The selected local story, identified for the comparison. Title is the
/// local `stories.title` (no recognition needed — the user owns this story).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransferPreviewStoryDto {
    pub id: String,
    pub title: String,
}

/// Wire shape returned by the `read_transfer_preview` Tauri command.
///
/// Tagged enum on `kind`: `"noDevice"`, `"unsupported"`, `"ready"`. All field
/// names are camelCase. The frontend mirror lives at
/// `src/shared/ipc-contracts/transfer-preview.ts` — drift is enforced by the
/// contract tests in `src-tauri/tests/contracts/transfer_preview.rs` AND the
/// runtime guard `isTransferPreviewDto`.
///
/// Scope reminder: `onDevice` / `unchangedCount` are composed by RUST from the
/// live device inventory and the `story_imports` join — the frontend never
/// recomputes them. No size metric is carried (no decisional volume before
/// media preparation). The OS mount path is never part of this DTO.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransferPreviewDto {
    NoDevice,
    #[serde(rename_all = "camelCase")]
    Unsupported {
        reason: UnsupportedReasonDto,
    },
    #[serde(rename_all = "camelCase")]
    Ready {
        device_identifier: String,
        story: TransferPreviewStoryDto,
        /// The selected story's pack already lives on the device — a send
        /// would replace it. `false` ⇒ a send would add it.
        on_device: bool,
        /// How many other device stories a send would leave untouched.
        unchanged_count: u32,
        /// Whether a transfer is allowed (the `WriteStory` capability). Always
        /// `false` in MVP Phase 1 — the preview is read-only.
        transferable: bool,
    },
}

impl TransferPreviewDto {
    /// Map the application outcome to the wire shape.
    pub fn from_outcome(outcome: TransferPreviewOutcome) -> Self {
        match outcome {
            TransferPreviewOutcome::NoDevice => Self::NoDevice,
            TransferPreviewOutcome::Unsupported { reason } => Self::Unsupported {
                reason: reason_dto(reason),
            },
            TransferPreviewOutcome::Ready {
                device_identifier,
                story_id,
                story_title,
                on_device,
                unchanged_count,
                transferable,
            } => Self::Ready {
                device_identifier,
                story: TransferPreviewStoryDto {
                    id: story_id,
                    title: story_title,
                },
                on_device,
                unchanged_count,
                transferable,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::UnsupportedReason;
    use serde_json::json;

    fn ready_outcome() -> TransferPreviewOutcome {
        TransferPreviewOutcome::Ready {
            device_identifier: "0123456789abcdef0123456789abcdef".into(),
            story_id: "0197a5d0-0000-7000-8000-000000000000".into(),
            story_title: "Mon histoire".into(),
            on_device: true,
            unchanged_count: 2,
            transferable: false,
        }
    }

    #[test]
    fn no_device_serializes_with_single_kind_key() {
        let v = serde_json::to_value(TransferPreviewDto::NoDevice).expect("ser");
        assert_eq!(v, json!({ "kind": "noDevice" }));
        assert_eq!(v.as_object().expect("obj").len(), 1);
    }

    #[test]
    fn ready_round_trips_with_camel_case_fields() {
        let v =
            serde_json::to_value(TransferPreviewDto::from_outcome(ready_outcome())).expect("ser");
        assert_eq!(v["kind"], "ready");
        assert_eq!(v["deviceIdentifier"], "0123456789abcdef0123456789abcdef");
        assert!(v["deviceIdentifier"].is_string());
        assert_eq!(v["story"]["id"], "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(v["story"]["title"], "Mon histoire");
        assert_eq!(v["onDevice"], true);
        assert_eq!(v["unchangedCount"], 2);
        assert_eq!(v["transferable"], false);
        // No snake_case leak.
        assert!(v.get("device_identifier").is_none());
        assert!(v.get("on_device").is_none());
        assert!(v.get("unchanged_count").is_none());
    }

    #[test]
    fn unsupported_serializes_typed_reason() {
        let dto = TransferPreviewDto::from_outcome(TransferPreviewOutcome::Unsupported {
            reason: UnsupportedReason::MultipleCandidates,
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "unsupported");
        assert_eq!(v["reason"], "multipleCandidates");
        // The unsupported variant carries nothing but kind + reason.
        assert_eq!(v.as_object().expect("obj").len(), 2);
    }

    #[test]
    fn new_on_device_maps_on_device_false() {
        let dto = TransferPreviewDto::from_outcome(TransferPreviewOutcome::Ready {
            device_identifier: "0123456789abcdef0123456789abcdef".into(),
            story_id: "s1".into(),
            story_title: "Nouvelle".into(),
            on_device: false,
            unchanged_count: 5,
            transferable: false,
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["onDevice"], false);
        assert_eq!(v["unchangedCount"], 5);
    }

    #[test]
    fn input_accepts_canonical_camel_case_payload() {
        let dto: ReadTransferPreviewInputDto = serde_json::from_value(json!({
            "storyId": "0197a5d0-0000-7000-8000-000000000000",
            "deviceIdentifier": "0123456789abcdef0123456789abcdef",
        }))
        .expect("deser");
        assert_eq!(dto.story_id, "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(dto.device_identifier, "0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn input_rejects_snake_case_field() {
        let err = serde_json::from_value::<ReadTransferPreviewInputDto>(json!({
            "story_id": "x",
            "deviceIdentifier": "y",
        }))
        .expect_err("must reject snake_case");
        let message = err.to_string().to_lowercase();
        assert!(
            message.contains("story_id") || message.contains("unknown field"),
            "expected snake_case rejection, got: {message}"
        );
    }

    #[test]
    fn input_rejects_unknown_field() {
        let err = serde_json::from_value::<ReadTransferPreviewInputDto>(json!({
            "storyId": "x",
            "deviceIdentifier": "y",
            "mountPath": "/sneaky",
        }))
        .expect_err("must reject unknown field — no path crosses IPC");
        assert!(err.to_string().contains("mountPath"));
    }

    #[test]
    fn input_rejects_missing_fields() {
        serde_json::from_value::<ReadTransferPreviewInputDto>(json!({ "storyId": "x" }))
            .expect_err("must reject missing deviceIdentifier");
        serde_json::from_value::<ReadTransferPreviewInputDto>(json!({ "deviceIdentifier": "y" }))
            .expect_err("must reject missing storyId");
    }
}
