//! Wire DTOs for the story-transfer (device-write) flow (commands + state).
//!
//! `start_transfer_story` returns a [`StartTransferAcceptedDto`] immediately (the
//! job continues via `job:*` events with `jobType = "transfer_story"`);
//! `read_transfer_state` returns the authoritative [`TransferStateDto`]. All
//! field names are camelCase; the frontend mirror lives at
//! `src/shared/ipc-contracts/story-transfer.ts` — drift is enforced by the
//! contract tests + the runtime guards.
//!
//! The OS mount path NEVER crosses this boundary. The terminal `transferred` is
//! the HONEST non-success "écriture effectuée — vérification à venir" — it
//! carries NO success vocabulary (verification belongs to a later story). The
//! `transferring` and `retryable` variants describe the in-flight phase and the
//! event-driven failure terminal the frontend renders from `job:*`; the Rust
//! re-read itself only ever produces `idle` or `transferred`.

use serde::{Deserialize, Serialize};

use crate::application::transfer::TransferStateView;
use crate::domain::transfer::TransferFailureCause;

use super::PreparationStoryDto;

/// Input accepted by `start_transfer_story`. `deny_unknown_fields` keeps the
/// boundary authoritative. Exactly the two identifiers the UI holds.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartTransferStoryInputDto {
    pub story_id: String,
    pub device_identifier: String,
}

/// Input accepted by `read_transfer_state`. Carries the TARGETED device so the
/// authoritative re-read is pinned to it: a pack present on a DIFFERENT writable
/// device must never be reported as transferred for this target (AC3 — the device
/// is the truth, no false success / wrong-device attribution).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadTransferStateInputDto {
    pub story_id: String,
    pub device_identifier: String,
}

/// Acceptance returned by `start_transfer_story`: the generated `jobId` to
/// correlate the `job:*` events, plus the target `storyId`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartTransferAcceptedDto {
    pub job_id: String,
    pub story_id: String,
}

/// Closed set of functional transfer-failure causes (camelCase). The UI branches
/// on this discriminant, never on the message text.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransferCauseDto {
    WriteNotAuthorized,
    NotPrepared,
    NotTransferable,
    DeviceChanged,
    WriteRejected,
    Interrupted,
}

/// Wire shape of the transfer state. Tagged enum on `kind`. The `transferring`
/// variant describes the in-flight phase the frontend derives from
/// `job:progress`; the `retryable` variant is built from the `job:failed` event;
/// `read_transfer_state` itself only ever produces `idle` or `transferred`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransferStateDto {
    /// No writable device / not yet transferred.
    Idle,
    /// Writing the prepared pack to the device (`en transfert`). `progress` is
    /// `null` unless a reliable fraction is known.
    #[serde(rename_all = "camelCase")]
    Transferring {
        device_identifier: String,
        story: PreparationStoryDto,
        progress: Option<f32>,
    },
    /// The pack was written (`écriture effectuée — vérification à venir`). NOT a
    /// verified success — the send is not "transférée et vérifiée" (reserved for
    /// a later story), and nothing here carries success vocabulary.
    #[serde(rename_all = "camelCase")]
    Transferred {
        device_identifier: String,
        story: PreparationStoryDto,
    },
    /// A recoverable failure (`échec récupérable`) consultable in context, with
    /// the canonical message + next gesture.
    #[serde(rename_all = "camelCase")]
    Retryable {
        story: PreparationStoryDto,
        cause: TransferCauseDto,
        message: String,
        user_action: String,
    },
}

impl TransferStateDto {
    /// Map the authoritative read-only view to the wire shape. The view only ever
    /// resolves to `idle` or `transferred`.
    pub fn from_view(view: TransferStateView) -> Self {
        match view {
            TransferStateView::Idle => Self::Idle,
            TransferStateView::Transferred {
                device_identifier,
                story_id,
                story_title,
            } => Self::Transferred {
                device_identifier,
                story: PreparationStoryDto {
                    id: story_id,
                    title: story_title,
                },
            },
        }
    }

    /// Build the `retryable` wire variant for a failure cause, generating the
    /// canonical FR `message` / `userAction` from the single domain source. Used
    /// by the contract tests and available to any consumer that surfaces a
    /// terminal failure from its cause.
    pub fn retryable(story: PreparationStoryDto, cause: TransferFailureCause) -> Self {
        let (message, user_action) = cause.copy();
        Self::Retryable {
            story,
            cause: cause_dto(cause),
            message: message.to_string(),
            user_action: user_action.to_string(),
        }
    }
}

/// Map a functional failure cause to its closed wire discriminant.
pub fn cause_dto(cause: TransferFailureCause) -> TransferCauseDto {
    match cause {
        TransferFailureCause::WriteNotAuthorized => TransferCauseDto::WriteNotAuthorized,
        TransferFailureCause::NotPrepared => TransferCauseDto::NotPrepared,
        TransferFailureCause::NotTransferable => TransferCauseDto::NotTransferable,
        TransferFailureCause::DeviceChanged => TransferCauseDto::DeviceChanged,
        TransferFailureCause::WriteRejected => TransferCauseDto::WriteRejected,
        TransferFailureCause::Interrupted => TransferCauseDto::Interrupted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const VALID_ID: &str = "0123456789abcdef0123456789abcdef";
    const STORY: &str = "0197a5d0-0000-7000-8000-000000000000";

    fn story_dto() -> PreparationStoryDto {
        PreparationStoryDto {
            id: STORY.into(),
            title: "Mon histoire".into(),
        }
    }

    #[test]
    fn idle_serializes_with_single_kind_key() {
        let v = serde_json::to_value(TransferStateDto::Idle).expect("ser");
        assert_eq!(v, json!({ "kind": "idle" }));
    }

    #[test]
    fn transferred_round_trips_camel_case_without_success_vocabulary() {
        let dto = TransferStateDto::from_view(TransferStateView::Transferred {
            device_identifier: VALID_ID.into(),
            story_id: STORY.into(),
            story_title: "Mon histoire".into(),
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "transferred");
        assert_eq!(v["deviceIdentifier"], VALID_ID);
        assert_eq!(v["story"]["id"], STORY);
        // No success vocabulary leaks: the terminal is neutral, carrying no
        // message that could read as "transférée et vérifiée".
        assert!(v.get("message").is_none());
        assert!(v.get("device_identifier").is_none(), "no snake_case leak");
    }

    #[test]
    fn transferring_serializes_camel_case_with_null_progress() {
        let dto = TransferStateDto::Transferring {
            device_identifier: VALID_ID.into(),
            story: story_dto(),
            progress: None,
        };
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "transferring");
        assert_eq!(v["progress"], json!(null));
        assert_eq!(v["deviceIdentifier"], VALID_ID);
    }

    #[test]
    fn retryable_carries_cause_message_and_user_action() {
        let dto =
            TransferStateDto::retryable(story_dto(), TransferFailureCause::WriteNotAuthorized);
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "retryable");
        assert_eq!(v["cause"], "writeNotAuthorized");
        assert!(!v["message"].as_str().expect("message").is_empty());
        assert!(!v["userAction"].as_str().expect("userAction").is_empty());
        assert!(v.get("user_action").is_none(), "no snake_case leak");
    }

    #[test]
    fn every_cause_maps_to_a_distinct_camel_case_discriminant() {
        for (cause, expected) in [
            (
                TransferFailureCause::WriteNotAuthorized,
                "writeNotAuthorized",
            ),
            (TransferFailureCause::NotPrepared, "notPrepared"),
            (TransferFailureCause::NotTransferable, "notTransferable"),
            (TransferFailureCause::DeviceChanged, "deviceChanged"),
            (TransferFailureCause::WriteRejected, "writeRejected"),
            (TransferFailureCause::Interrupted, "interrupted"),
        ] {
            let v = serde_json::to_value(cause_dto(cause)).expect("ser");
            assert_eq!(v, json!(expected));
        }
    }

    #[test]
    fn accepted_dto_serializes_camel_case() {
        let dto = StartTransferAcceptedDto {
            job_id: "job-1".into(),
            story_id: STORY.into(),
        };
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["jobId"], "job-1");
        assert_eq!(v["storyId"], STORY);
    }

    #[test]
    fn start_input_rejects_unknown_field() {
        let err = serde_json::from_value::<StartTransferStoryInputDto>(json!({
            "storyId": STORY,
            "deviceIdentifier": VALID_ID,
            "mountPath": "/sneaky",
        }))
        .expect_err("must reject unknown field — no path crosses IPC");
        assert!(err.to_string().contains("mountPath"));
    }

    #[test]
    fn read_input_requires_story_id_and_device_identifier() {
        // The re-read is pinned to the targeted device (C1): both identifiers are
        // required, and any extra field (e.g. a path) is still refused.
        let dto: ReadTransferStateInputDto = serde_json::from_value(json!({
            "storyId": STORY,
            "deviceIdentifier": VALID_ID,
        }))
        .expect("deser");
        assert_eq!(dto.story_id, STORY);
        assert_eq!(dto.device_identifier, VALID_ID);
        let err = serde_json::from_value::<ReadTransferStateInputDto>(json!({
            "storyId": STORY,
            "deviceIdentifier": VALID_ID,
            "mountPath": "/sneaky",
        }))
        .expect_err("read input refuses unknown fields — no path crosses IPC");
        assert!(err.to_string().contains("mountPath"));
    }
}
