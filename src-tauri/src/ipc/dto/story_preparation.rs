//! Wire DTOs for the story-preparation flow (commands + state).
//!
//! `start_prepare_story` returns a [`StartPreparationAcceptedDto`] immediately
//! (the job continues via `job:*` events); `read_preparation_state` returns the
//! authoritative [`PreparationStateDto`]. All field names are camelCase; the
//! frontend mirror lives at `src/shared/ipc-contracts/story-preparation.ts` —
//! drift is enforced by the contract tests + the runtime guards.
//!
//! The OS mount path NEVER crosses this boundary. The descriptor's internal
//! artifact list is NOT exposed (an implementation detail of the local
//! assembly) — only the cohort + story identity surface on `prepared`.

use serde::{Deserialize, Serialize};

use crate::application::transfer::PreparationStateView;
use crate::domain::transfer::PreparationFailureCause;
use crate::ipc::dto::story_validation::canonical_blocker_dto;
use crate::ipc::dto::BlockerDto;

/// Input accepted by `start_prepare_story`. `deny_unknown_fields` keeps the
/// boundary authoritative. Exactly the two identifiers the UI holds.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartPrepareStoryInputDto {
    pub story_id: String,
    pub device_identifier: String,
}

/// Input accepted by `read_preparation_state`. Only the local story id — Rust
/// re-resolves the connected device itself.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadPreparationStateInputDto {
    pub story_id: String,
}

/// Acceptance returned by `start_prepare_story`: the generated `jobId` to
/// correlate the `job:*` events, plus the target `storyId`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartPreparationAcceptedDto {
    pub job_id: String,
    pub story_id: String,
}

/// The selected local story, identified for the preparation state.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparationStoryDto {
    pub id: String,
    pub title: String,
}

/// Closed set of functional preparation-failure causes (camelCase). The UI
/// branches on this discriminant, never on the message text.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PreparationCauseDto {
    PreflightNotPassing,
    ArtifactMissing,
    ArtifactCorrupt,
    DeviceChanged,
    Interrupted,
}

/// Wire shape of the preparation state. Tagged enum on `kind`. The `preflight`
/// and `preparing` variants describe the in-flight phases the frontend derives
/// from `job:progress` events; `read_preparation_state` itself only ever
/// produces `idle`, `prepared` or `retryable` (a synchronous re-derivation runs
/// to a resting/terminal state).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PreparationStateDto {
    /// No readable supported device — nothing to prepare.
    Idle,
    /// Re-verifying before assembly (`en vérification`).
    #[serde(rename_all = "camelCase")]
    Preflight {
        device_identifier: String,
        story: PreparationStoryDto,
    },
    /// Assembling locally (`en préparation`). `progress` is `null` unless a
    /// reliable fraction is known.
    #[serde(rename_all = "camelCase")]
    Preparing {
        device_identifier: String,
        story: PreparationStoryDto,
        progress: Option<f32>,
    },
    /// Artifacts assembled and fresh (`Préparée`). NOT a transfer success — the
    /// send CTA stays disabled.
    #[serde(rename_all = "camelCase")]
    Prepared {
        device_identifier: String,
        story: PreparationStoryDto,
        target_cohort: String,
    },
    /// A recoverable failure (`échec récupérable`) consultable in context, with
    /// the canonical message + next gesture and any preflight blockers.
    #[serde(rename_all = "camelCase")]
    Retryable {
        story: PreparationStoryDto,
        cause: PreparationCauseDto,
        message: String,
        user_action: String,
        blockers: Vec<BlockerDto>,
    },
}

impl PreparationStateDto {
    /// Map the application view to the wire shape, generating the canonical FR
    /// `message` / `userAction` per cause from the single domain source.
    pub fn from_view(view: PreparationStateView) -> Self {
        match view {
            PreparationStateView::Idle => Self::Idle,
            PreparationStateView::Prepared {
                device_identifier,
                story_id,
                story_title,
                target_cohort,
            } => Self::Prepared {
                device_identifier,
                story: PreparationStoryDto {
                    id: story_id,
                    title: story_title,
                },
                target_cohort,
            },
            PreparationStateView::Retryable {
                story_id,
                story_title,
                cause,
                blockers,
            } => {
                let (message, user_action) = cause.copy();
                Self::Retryable {
                    story: PreparationStoryDto {
                        id: story_id,
                        title: story_title,
                    },
                    cause: cause_dto(cause),
                    message: message.to_string(),
                    user_action: user_action.to_string(),
                    blockers: blockers.iter().map(canonical_blocker_dto).collect(),
                }
            }
        }
    }
}

/// Map a functional failure cause to its closed wire discriminant.
pub fn cause_dto(cause: PreparationFailureCause) -> PreparationCauseDto {
    match cause {
        PreparationFailureCause::PreflightNotPassing => PreparationCauseDto::PreflightNotPassing,
        PreparationFailureCause::ArtifactMissing => PreparationCauseDto::ArtifactMissing,
        PreparationFailureCause::ArtifactCorrupt => PreparationCauseDto::ArtifactCorrupt,
        PreparationFailureCause::DeviceChanged => PreparationCauseDto::DeviceChanged,
        PreparationFailureCause::Interrupted => PreparationCauseDto::Interrupted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::story::{Axis, CanonicalBlocker, CanonicalCause, Severity};
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
        let v = serde_json::to_value(PreparationStateDto::Idle).expect("ser");
        assert_eq!(v, json!({ "kind": "idle" }));
    }

    #[test]
    fn prepared_round_trips_camel_case() {
        let dto = PreparationStateDto::from_view(PreparationStateView::Prepared {
            device_identifier: VALID_ID.into(),
            story_id: STORY.into(),
            story_title: "Mon histoire".into(),
            target_cohort: "origine_v1".into(),
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "prepared");
        assert_eq!(v["deviceIdentifier"], VALID_ID);
        assert_eq!(v["story"]["id"], STORY);
        assert_eq!(v["targetCohort"], "origine_v1");
        assert!(v.get("device_identifier").is_none(), "no snake_case leak");
        assert!(v.get("target_cohort").is_none());
    }

    #[test]
    fn retryable_carries_cause_message_and_user_action() {
        let blocker = CanonicalBlocker {
            axis: Axis::Structure,
            cause: CanonicalCause::ChecksumMismatch,
            severity: Severity::Blocking,
        };
        let dto = PreparationStateDto::from_view(PreparationStateView::Retryable {
            story_id: STORY.into(),
            story_title: "Mon histoire".into(),
            cause: PreparationFailureCause::PreflightNotPassing,
            blockers: vec![blocker],
        });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "retryable");
        assert_eq!(v["cause"], "preflightNotPassing");
        assert!(!v["message"].as_str().expect("message").is_empty());
        assert!(!v["userAction"].as_str().expect("userAction").is_empty());
        assert_eq!(v["blockers"][0]["axis"], "structure");
        assert_eq!(v["blockers"][0]["cause"], "checksumMismatch");
        assert!(v.get("user_action").is_none(), "no snake_case leak");
    }

    #[test]
    fn every_cause_maps_to_a_distinct_camel_case_discriminant() {
        for (cause, expected) in [
            (
                PreparationFailureCause::PreflightNotPassing,
                "preflightNotPassing",
            ),
            (PreparationFailureCause::ArtifactMissing, "artifactMissing"),
            (PreparationFailureCause::ArtifactCorrupt, "artifactCorrupt"),
            (PreparationFailureCause::DeviceChanged, "deviceChanged"),
            (PreparationFailureCause::Interrupted, "interrupted"),
        ] {
            let v = serde_json::to_value(cause_dto(cause)).expect("ser");
            assert_eq!(v, json!(expected));
        }
    }

    #[test]
    fn in_flight_variants_serialize_camel_case() {
        let preflight = PreparationStateDto::Preflight {
            device_identifier: VALID_ID.into(),
            story: story_dto(),
        };
        let v = serde_json::to_value(&preflight).expect("ser");
        assert_eq!(v["kind"], "preflight");
        assert_eq!(v["deviceIdentifier"], VALID_ID);

        let preparing = PreparationStateDto::Preparing {
            device_identifier: VALID_ID.into(),
            story: story_dto(),
            progress: None,
        };
        let v = serde_json::to_value(&preparing).expect("ser");
        assert_eq!(v["kind"], "preparing");
        assert_eq!(v["progress"], json!(null));
    }

    #[test]
    fn accepted_dto_serializes_camel_case() {
        let dto = StartPreparationAcceptedDto {
            job_id: "job-1".into(),
            story_id: STORY.into(),
        };
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["jobId"], "job-1");
        assert_eq!(v["storyId"], STORY);
    }

    #[test]
    fn start_input_rejects_unknown_field() {
        let err = serde_json::from_value::<StartPrepareStoryInputDto>(json!({
            "storyId": STORY,
            "deviceIdentifier": VALID_ID,
            "mountPath": "/sneaky",
        }))
        .expect_err("must reject unknown field — no path crosses IPC");
        assert!(err.to_string().contains("mountPath"));
    }

    #[test]
    fn read_input_accepts_only_story_id() {
        let dto: ReadPreparationStateInputDto = serde_json::from_value(json!({
            "storyId": STORY,
        }))
        .expect("deser");
        assert_eq!(dto.story_id, STORY);
        let err = serde_json::from_value::<ReadPreparationStateInputDto>(json!({
            "storyId": STORY,
            "deviceIdentifier": VALID_ID,
        }))
        .expect_err("read input takes only storyId");
        assert!(err.to_string().contains("deviceIdentifier"));
    }
}
