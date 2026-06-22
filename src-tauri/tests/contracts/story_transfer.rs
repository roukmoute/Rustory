//! Wire-shape contract for the story-transfer (device-write) flow: the exact
//! JSON of every `TransferStateDto` variant, the acceptance DTO, and the `job:*`
//! event payloads with `phase: "transfer"`. The frontend mirror
//! (`src/shared/ipc-contracts/story-transfer.ts`) must stay byte-compatible.

use rustory_lib::application::transfer::TransferStateView;
use rustory_lib::domain::transfer::TransferFailureCause;
use rustory_lib::ipc::dto::{PreparationStoryDto, StartTransferAcceptedDto, TransferStateDto};
use rustory_lib::ipc::events::{
    JobFailedEvent, JobProgressEvent, JOB_TYPE_TRANSFER_STORY, TRANSFER_FAILED_CODE,
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
fn transferred_wire_shape_carries_no_success_vocabulary() {
    let dto = TransferStateDto::from_view(TransferStateView::Transferred {
        device_identifier: DEVICE.into(),
        story_id: STORY.into(),
        story_title: "Mon histoire".into(),
    });
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "transferred");
    assert_eq!(v["deviceIdentifier"], DEVICE);
    assert_eq!(v["story"]["title"], "Mon histoire");
    // The terminal is the honest "écriture effectuée — vérification à venir":
    // it carries NO message, so no "transférée et vérifiée" can leak.
    assert!(
        v.get("message").is_none(),
        "no success message on the terminal"
    );
}

#[test]
fn retryable_wire_shape_carries_non_empty_message_and_action() {
    let dto = TransferStateDto::retryable(story(), TransferFailureCause::DeviceChanged);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "retryable");
    assert_eq!(v["cause"], "deviceChanged");
    assert!(!v["message"].as_str().expect("message").is_empty());
    assert!(!v["userAction"].as_str().expect("userAction").is_empty());
    assert!(v.get("user_action").is_none(), "no snake_case leak");
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
        let v = serde_json::to_value(TransferStateDto::retryable(story(), cause)).expect("ser");
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
        error_message: "Envoi interrompu avant la fin.".into(),
        user_action: "Relance l'envoi.".into(),
    })
    .expect("ser");
    assert_eq!(v["errorCode"], "TRANSFER_FAILED");
    assert!(!v["errorMessage"].as_str().expect("message").is_empty());
    assert!(!v["userAction"].as_str().expect("userAction").is_empty());
    assert!(v.get("error_code").is_none(), "no snake_case leak");
}
