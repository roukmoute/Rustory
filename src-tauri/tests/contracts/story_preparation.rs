//! Wire-shape contract for the story-preparation flow: the exact JSON of every
//! `PreparationStateDto` variant, the acceptance DTO, and the three `job:*`
//! event payloads. The frontend mirror (`src/shared/ipc-contracts/
//! story-preparation.ts`) must stay byte-compatible with these shapes.

use rustory_lib::application::transfer::PreparationStateView;
use rustory_lib::domain::story::{Axis, CanonicalBlocker, CanonicalCause, Severity};
use rustory_lib::domain::transfer::PreparationFailureCause;
use rustory_lib::ipc::dto::{
    PreparationStateDto, PreparationStoryDto, StartPreparationAcceptedDto,
};
use rustory_lib::ipc::events::{
    JobCompletedEvent, JobFailedEvent, JobProgressEvent, JOB_TYPE_PREPARE_STORY,
    PREPARATION_FAILED_CODE,
};
use serde_json::json;

const DEVICE: &str = "0123456789abcdef0123456789abcdef";
const STORY: &str = "0197a5d0-0000-7000-8000-000000000000";

fn story() -> PreparationStoryDto {
    PreparationStoryDto {
        id: STORY.into(),
        title: "Mon histoire".into(),
    }
}

#[test]
fn idle_is_a_single_kind_key() {
    let v = serde_json::to_value(PreparationStateDto::Idle).expect("ser");
    assert_eq!(v, json!({ "kind": "idle" }));
}

#[test]
fn preflight_and_preparing_wire_shapes() {
    let v = serde_json::to_value(PreparationStateDto::Preflight {
        device_identifier: DEVICE.into(),
        story: story(),
    })
    .expect("ser");
    assert_eq!(v["kind"], "preflight");
    assert_eq!(v["deviceIdentifier"], DEVICE);
    assert_eq!(v["story"]["id"], STORY);

    let v = serde_json::to_value(PreparationStateDto::Preparing {
        device_identifier: DEVICE.into(),
        story: story(),
        progress: None,
    })
    .expect("ser");
    assert_eq!(v["kind"], "preparing");
    assert_eq!(v["progress"], json!(null));
}

#[test]
fn prepared_wire_shape_is_camel_case() {
    let dto = PreparationStateDto::from_view(PreparationStateView::Prepared {
        device_identifier: DEVICE.into(),
        story_id: STORY.into(),
        story_title: "Mon histoire".into(),
        target_cohort: "v3".into(),
        transferable: false,
    });
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "prepared");
    assert_eq!(v["deviceIdentifier"], DEVICE);
    assert_eq!(v["story"]["title"], "Mon histoire");
    assert_eq!(v["targetCohort"], "v3");
    assert_eq!(v["transferable"], false);
    assert!(v.get("target_cohort").is_none(), "no snake_case leak");
}

#[test]
fn retryable_wire_shape_carries_non_empty_action_and_blockers() {
    let dto = PreparationStateDto::from_view(PreparationStateView::Retryable {
        story_id: STORY.into(),
        story_title: "Mon histoire".into(),
        cause: PreparationFailureCause::PreflightNotPassing,
        blockers: vec![CanonicalBlocker {
            axis: Axis::Structure,
            cause: CanonicalCause::ChecksumMismatch,
            severity: Severity::Blocking,
        }],
    });
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "retryable");
    assert_eq!(v["cause"], "preflightNotPassing");
    assert!(!v["message"].as_str().expect("message").is_empty());
    assert!(!v["userAction"].as_str().expect("userAction").is_empty());
    assert_eq!(v["blockers"][0]["axis"], "structure");
    assert_eq!(v["blockers"][0]["cause"], "checksumMismatch");
    assert!(!v["blockers"][0]["userAction"]
        .as_str()
        .expect("blocker userAction")
        .is_empty());
}

#[test]
fn every_cause_serializes_to_a_distinct_camel_case_discriminant() {
    let mut seen = Vec::new();
    for cause in [
        PreparationFailureCause::PreflightNotPassing,
        PreparationFailureCause::ArtifactMissing,
        PreparationFailureCause::ArtifactCorrupt,
        PreparationFailureCause::DeviceChanged,
        PreparationFailureCause::Interrupted,
    ] {
        let dto = PreparationStateDto::from_view(PreparationStateView::Retryable {
            story_id: STORY.into(),
            story_title: "T".into(),
            cause,
            blockers: vec![],
        });
        let v = serde_json::to_value(&dto).expect("ser");
        let tag = v["cause"].as_str().expect("cause").to_string();
        assert!(!tag.is_empty());
        seen.push(tag);
    }
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        seen.len(),
        "causes must be distinct on the wire"
    );
}

#[test]
fn acceptance_dto_wire_shape() {
    let v = serde_json::to_value(StartPreparationAcceptedDto {
        job_id: "job-1".into(),
        story_id: STORY.into(),
    })
    .expect("ser");
    assert_eq!(v, json!({ "jobId": "job-1", "storyId": STORY }));
}

#[test]
fn job_progress_event_wire_shape() {
    let v = serde_json::to_value(JobProgressEvent {
        job_id: "j1".into(),
        job_type: JOB_TYPE_PREPARE_STORY.into(),
        target_story_id: STORY.into(),
        phase: "preflight".into(),
        progress: None,
        sequence: 1,
        message: None,
    })
    .expect("ser");
    assert_eq!(
        v,
        json!({
            "jobId": "j1",
            "jobType": "prepare_story",
            "targetStoryId": STORY,
            "phase": "preflight",
            "progress": null,
            "sequence": 1,
            "message": null,
        })
    );
}

#[test]
fn job_completed_event_wire_shape() {
    let v = serde_json::to_value(JobCompletedEvent {
        job_id: "j1".into(),
        job_type: JOB_TYPE_PREPARE_STORY.into(),
        target_story_id: STORY.into(),
        sequence: 3,
    })
    .expect("ser");
    assert_eq!(v["jobId"], "j1");
    assert_eq!(v["jobType"], "prepare_story");
    assert_eq!(v["targetStoryId"], STORY);
    assert_eq!(v["sequence"], 3);
}

#[test]
fn job_failed_event_carries_non_empty_message_and_action() {
    let v = serde_json::to_value(JobFailedEvent {
        job_id: "j1".into(),
        job_type: JOB_TYPE_PREPARE_STORY.into(),
        target_story_id: STORY.into(),
        sequence: 2,
        error_code: PREPARATION_FAILED_CODE.into(),
        error_message: "Préparation interrompue.".into(),
        user_action: "Relance la préparation.".into(),
        completeness: None,
        cause: None,
    })
    .expect("ser");
    assert_eq!(v["errorCode"], "PREPARATION_FAILED");
    assert!(
        v.get("completeness").is_none(),
        "preparation failures carry no completeness"
    );
    assert!(!v["errorMessage"].as_str().expect("message").is_empty());
    assert!(!v["userAction"].as_str().expect("userAction").is_empty());
    assert!(v.get("error_code").is_none(), "no snake_case leak");
}
