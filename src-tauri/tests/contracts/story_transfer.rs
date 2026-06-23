//! Wire-shape contract for the story-transfer (device-write) flow: the exact
//! JSON of every `TransferStateDto` variant, the acceptance DTO, and the `job:*`
//! event payloads with `phase: "transfer"`. The frontend mirror
//! (`src/shared/ipc-contracts/story-transfer.ts`) must stay byte-compatible.

use rustory_lib::application::transfer::TransferStateView;
use rustory_lib::domain::transfer::{TransferCompleteness, TransferFailureCause, VerifiedSummary};
use rustory_lib::ipc::dto::{PreparationStoryDto, StartTransferAcceptedDto, TransferStateDto};
use rustory_lib::ipc::events::{
    JobCompletedEvent, JobCompletedSummary, JobFailedEvent, JobProgressEvent,
    JOB_TYPE_TRANSFER_STORY, TRANSFER_FAILED_CODE,
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
    let v = serde_json::to_value(TransferStateDto::Idle).expect("ser");
    assert_eq!(v, json!({ "kind": "idle" }));
}

#[test]
fn transferring_wire_shape_is_camel_case() {
    let v = serde_json::to_value(TransferStateDto::Transferring {
        device_identifier: DEVICE.into(),
        story: story(),
        progress: None,
    })
    .expect("ser");
    assert_eq!(v["kind"], "transferring");
    assert_eq!(v["deviceIdentifier"], DEVICE);
    assert_eq!(v["story"]["id"], STORY);
    assert_eq!(v["progress"], json!(null));
    assert!(v.get("device_identifier").is_none(), "no snake_case leak");
}

#[test]
fn verified_wire_shape_carries_the_summary() {
    let dto = TransferStateDto::from_view(TransferStateView::Verified {
        device_identifier: DEVICE.into(),
        story_id: STORY.into(),
        story_title: "Mon histoire".into(),
        summary: VerifiedSummary {
            changed: "« Mon histoire » est maintenant sur la Lunii.".into(),
            unchanged: "4 autres histoires de l'appareil restent inchangées.".into(),
        },
    });
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "verified");
    assert_eq!(v["deviceIdentifier"], DEVICE);
    assert_eq!(v["story"]["title"], "Mon histoire");
    // The AC2 summary travels as READY-MADE lines (composed in Rust), camelCase.
    assert_eq!(
        v["summary"]["changed"],
        "« Mon histoire » est maintenant sur la Lunii."
    );
    assert_eq!(
        v["summary"]["unchanged"],
        "4 autres histoires de l'appareil restent inchangées."
    );
}

#[test]
fn retryable_wire_shape_carries_non_empty_message_and_action() {
    let dto = TransferStateDto::retryable(
        story(),
        TransferFailureCause::DeviceChanged,
        TransferCompleteness::Failed,
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "retryable");
    assert_eq!(v["cause"], "deviceChanged");
    assert_eq!(v["completeness"], "failed");
    assert!(!v["message"].as_str().expect("message").is_empty());
    assert!(!v["userAction"].as_str().expect("userAction").is_empty());
    assert!(v.get("user_action").is_none(), "no snake_case leak");
}

#[test]
fn retryable_incomplete_wire_shape_carries_the_incomplete_completeness() {
    let dto = TransferStateDto::retryable(
        story(),
        TransferFailureCause::WriteRejected,
        TransferCompleteness::Incomplete,
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "retryable");
    assert_eq!(v["completeness"], "incomplete");
    assert!(!v["message"].as_str().expect("message").is_empty());
}

#[test]
fn transferring_wire_shape_carries_a_reliable_progress_fraction() {
    // 0.5 is exactly representable in f32, so the wire value round-trips cleanly.
    let v = serde_json::to_value(TransferStateDto::Transferring {
        device_identifier: DEVICE.into(),
        story: story(),
        progress: Some(0.5),
    })
    .expect("ser");
    assert_eq!(v["kind"], "transferring");
    assert_eq!(v["progress"], json!(0.5));
}

#[test]
fn every_cause_serializes_to_a_distinct_camel_case_discriminant() {
    let mut seen = Vec::new();
    for cause in [
        TransferFailureCause::WriteNotAuthorized,
        TransferFailureCause::NotPrepared,
        TransferFailureCause::NotTransferable,
        TransferFailureCause::DeviceChanged,
        TransferFailureCause::WriteRejected,
        TransferFailureCause::Interrupted,
    ] {
        let v = serde_json::to_value(TransferStateDto::retryable(
            story(),
            cause,
            TransferCompleteness::Failed,
        ))
        .expect("ser");
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
    let v = serde_json::to_value(StartTransferAcceptedDto {
        job_id: "job-1".into(),
        story_id: STORY.into(),
    })
    .expect("ser");
    assert_eq!(v, json!({ "jobId": "job-1", "storyId": STORY }));
}

#[test]
fn job_progress_event_carries_the_transfer_phase() {
    let v = serde_json::to_value(JobProgressEvent {
        job_id: "j1".into(),
        job_type: JOB_TYPE_TRANSFER_STORY.into(),
        target_story_id: STORY.into(),
        phase: "transfer".into(),
        progress: None,
        sequence: 2,
        message: None,
    })
    .expect("ser");
    assert_eq!(
        v,
        json!({
            "jobId": "j1",
            "jobType": "transfer_story",
            "targetStoryId": STORY,
            "phase": "transfer",
            "progress": null,
            "sequence": 2,
            "message": null,
        })
    );
}

#[test]
fn job_failed_event_uses_the_transfer_error_code() {
    let v = serde_json::to_value(JobFailedEvent {
        job_id: "j1".into(),
        job_type: JOB_TYPE_TRANSFER_STORY.into(),
        target_story_id: STORY.into(),
        sequence: 3,
        error_code: TRANSFER_FAILED_CODE.into(),
        error_message: "Envoi incomplet.".into(),
        user_action: "Relance l'envoi.".into(),
        completeness: Some("incomplete".into()),
        cause: Some("writeRejected".into()),
        verify_verdict: None,
    })
    .expect("ser");
    assert_eq!(v["errorCode"], "TRANSFER_FAILED");
    assert_eq!(v["completeness"], "incomplete");
    assert_eq!(v["cause"], "writeRejected");
    assert!(!v["errorMessage"].as_str().expect("message").is_empty());
    assert!(!v["userAction"].as_str().expect("userAction").is_empty());
    assert!(v.get("error_code").is_none(), "no snake_case leak");
}

#[test]
fn job_completed_event_carries_the_verified_summary() {
    // F1/F5: the verified success travels ON the terminal as ready-made lines, so
    // the UI renders it without a stale-identifier re-read and without recomposing
    // the text in React.
    let v = serde_json::to_value(JobCompletedEvent {
        job_id: "j1".into(),
        job_type: JOB_TYPE_TRANSFER_STORY.into(),
        target_story_id: STORY.into(),
        sequence: 4,
        summary: Some(JobCompletedSummary {
            changed: "« Mon histoire » est maintenant sur la Lunii.".into(),
            unchanged: "2 autres histoires de l'appareil restent inchangées.".into(),
        }),
    })
    .expect("ser");
    assert_eq!(
        v["summary"]["changed"],
        "« Mon histoire » est maintenant sur la Lunii."
    );
    assert_eq!(
        v["summary"]["unchanged"],
        "2 autres histoires de l'appareil restent inchangées."
    );
}

#[test]
fn job_progress_event_carries_the_verify_phase() {
    // The `verify` phase is the FINAL phase of the same `transfer_story` job.
    let v = serde_json::to_value(JobProgressEvent {
        job_id: "j1".into(),
        job_type: JOB_TYPE_TRANSFER_STORY.into(),
        target_story_id: STORY.into(),
        phase: "verify".into(),
        progress: None,
        sequence: 3,
        message: None,
    })
    .expect("ser");
    assert_eq!(v["phase"], "verify");
    assert_eq!(v["progress"], json!(null));
}

#[test]
fn job_failed_event_carries_the_verify_partial_verdict() {
    // The `partial` terminal reuses the failure channel with the verify-verdict
    // discriminant ONLY — never a write-phase completeness/cause.
    let v = serde_json::to_value(JobFailedEvent {
        job_id: "j1".into(),
        job_type: JOB_TYPE_TRANSFER_STORY.into(),
        target_story_id: STORY.into(),
        sequence: 4,
        error_code: TRANSFER_FAILED_CODE.into(),
        error_message: "Envoi dans un état partiel : certains éléments n'ont pas pu être confirmés sur la Lunii.".into(),
        user_action: "Relance l'envoi pour rétablir un état sûr.".into(),
        completeness: None,
        cause: None,
        verify_verdict: Some("partial".into()),
    })
    .expect("ser");
    assert_eq!(v["verifyVerdict"], "partial");
    assert!(
        v.get("completeness").is_none(),
        "no write-phase completeness"
    );
    assert!(v.get("cause").is_none(), "no write-phase cause");
    assert!(!v["errorMessage"].as_str().expect("message").is_empty());
    assert!(!v["userAction"].as_str().expect("userAction").is_empty());
}
