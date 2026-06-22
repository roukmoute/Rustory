//! Typed IPC event payloads for long-running jobs.
//!
//! First occupant of the event channel (story 3.x): a `start_*` command returns
//! an acceptance immediately and reports progress through these three typed,
//! versionable payloads, correlated by `job_id`. Each carries a MONOTONIC
//! `sequence` so a consumer applying events out of order or twice still
//! converges (idempotence) — see the frontend subscription helper and hook.
//!
//! Names are lowercase, `:`-separated (architecture naming patterns). Payloads
//! are camelCase on the wire (the contract tests + the frontend guards enforce
//! the exact shape). The terminal authoritative truth is re-read via
//! `read_preparation_state`, never reconstructed from these events alone.

use serde::Serialize;

/// `jobType` discriminant of the story-preparation flow. The frontend filters
/// `job:*` events by this value + `targetStoryId`.
pub const JOB_TYPE_PREPARE_STORY: &str = "prepare_story";

/// Wire event name: a phase transition / progress update.
pub const EVENT_JOB_PROGRESS: &str = "job:progress";
/// Wire event name: the job reached a successful terminal state.
pub const EVENT_JOB_COMPLETED: &str = "job:completed";
/// Wire event name: the job reached a failure terminal state. The structured
/// cause is re-read authoritatively; this payload carries enough to surface a
/// message immediately.
pub const EVENT_JOB_FAILED: &str = "job:failed";

/// Stable `errorCode` carried by every preparation `job:failed` — both
/// functional (`retryable`) and transport failures. The structured cause +
/// blockers come from the authoritative re-read, not this label.
pub const PREPARATION_FAILED_CODE: &str = "PREPARATION_FAILED";

/// `job:progress` payload. `progress` is `null` unless a RELIABLE fraction is
/// known (MVP never sends a fake percentage). `message` stays `null` here — the
/// UI labels the phase itself.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobProgressEvent {
    pub job_id: String,
    pub job_type: String,
    pub target_story_id: String,
    pub phase: String,
    pub progress: Option<f32>,
    pub sequence: u64,
    pub message: Option<String>,
}

/// `job:completed` payload — a successful terminal state.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobCompletedEvent {
    pub job_id: String,
    pub job_type: String,
    pub target_story_id: String,
    pub sequence: u64,
}

/// `job:failed` payload — a failure terminal state with a non-empty
/// `errorMessage` + `userAction` so the UI can react before the authoritative
/// re-read returns.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobFailedEvent {
    pub job_id: String,
    pub job_type: String,
    pub target_story_id: String,
    pub sequence: u64,
    pub error_code: String,
    pub error_message: String,
    pub user_action: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn progress_serializes_camel_case_with_null_progress_and_message() {
        let ev = JobProgressEvent {
            job_id: "j1".into(),
            job_type: JOB_TYPE_PREPARE_STORY.into(),
            target_story_id: "s1".into(),
            phase: "preflight".into(),
            progress: None,
            sequence: 1,
            message: None,
        };
        let v = serde_json::to_value(&ev).expect("ser");
        assert_eq!(
            v,
            json!({
                "jobId": "j1",
                "jobType": "prepare_story",
                "targetStoryId": "s1",
                "phase": "preflight",
                "progress": null,
                "sequence": 1,
                "message": null,
            })
        );
        assert!(v.get("job_id").is_none(), "snake_case must not leak");
    }

    #[test]
    fn completed_serializes_camel_case() {
        let ev = JobCompletedEvent {
            job_id: "j1".into(),
            job_type: JOB_TYPE_PREPARE_STORY.into(),
            target_story_id: "s1".into(),
            sequence: 3,
        };
        let v = serde_json::to_value(&ev).expect("ser");
        assert_eq!(v["jobId"], "j1");
        assert_eq!(v["targetStoryId"], "s1");
        assert_eq!(v["sequence"], 3);
    }

    #[test]
    fn failed_serializes_camel_case_with_non_empty_action() {
        let ev = JobFailedEvent {
            job_id: "j1".into(),
            job_type: JOB_TYPE_PREPARE_STORY.into(),
            target_story_id: "s1".into(),
            sequence: 2,
            error_code: PREPARATION_FAILED_CODE.into(),
            error_message: "Préparation interrompue.".into(),
            user_action: "Relance la préparation.".into(),
        };
        let v = serde_json::to_value(&ev).expect("ser");
        assert_eq!(v["errorCode"], "PREPARATION_FAILED");
        assert!(!v["errorMessage"].as_str().expect("msg").is_empty());
        assert!(!v["userAction"].as_str().expect("action").is_empty());
        assert!(v.get("error_code").is_none(), "snake_case must not leak");
    }

    #[test]
    fn event_names_are_lowercase_colon_separated() {
        assert_eq!(EVENT_JOB_PROGRESS, "job:progress");
        assert_eq!(EVENT_JOB_COMPLETED, "job:completed");
        assert_eq!(EVENT_JOB_FAILED, "job:failed");
    }
}
