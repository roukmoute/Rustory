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

/// `jobType` discriminant of the story-transfer (device-write) flow. The same
/// `job:*` payloads carry it; the `phase` field then also takes `"transfer"`.
pub const JOB_TYPE_TRANSFER_STORY: &str = "transfer_story";

/// Wire event name: a phase transition / progress update.
pub const EVENT_JOB_PROGRESS: &str = "job:progress";
/// Wire event name: the job reached a successful terminal state.
pub const EVENT_JOB_COMPLETED: &str = "job:completed";
/// Wire event name: the job reached a failure terminal state. The structured
/// cause is re-read authoritatively; this payload carries enough to surface a
/// message immediately.
pub const EVENT_JOB_FAILED: &str = "job:failed";

/// Wire event name: an OS-open intent arrived while the app is ALREADY
/// running (single-instance relay, macOS `Opened`). A pure SIGNAL — the
/// frontend pulls the verdict through `analyze_os_open_request`, never from
/// the event. Cold start emits nothing (the library-mount pull covers it).
pub const EVENT_OS_OPEN_REQUESTED: &str = "os-open:requested";

/// `os-open:requested` payload: an EMPTY versionable object (`{}`). The
/// event carries no data by design — the absolute path never crosses the
/// boundary, and the verdict is pulled by command.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OsOpenRequestedEvent {}

/// Stable `errorCode` carried by every preparation `job:failed` — both
/// functional (`retryable`) and transport failures. The structured cause +
/// blockers come from the authoritative re-read, not this label.
pub const PREPARATION_FAILED_CODE: &str = "PREPARATION_FAILED";

/// Stable `errorCode` carried by every transfer `job:failed` — both functional
/// (`retryable`) and transport failures. The canonical `message` / `userAction`
/// travel in the same payload; the structured truth is the authoritative re-read.
pub const TRANSFER_FAILED_CODE: &str = "TRANSFER_FAILED";

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

/// The `verified` confirmation summary carried on a transfer `job:completed`
/// (AC2/FR15) — composed in Rust, rendered VERBATIM by the panel.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobCompletedSummary {
    /// "« <Titre> » est maintenant sur la Lunii." — what changed + final state.
    pub changed: String,
    /// "N autres histoires de l'appareil restent inchangées." — what stayed.
    pub unchanged: String,
}

/// `job:completed` payload — a successful terminal state.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobCompletedEvent {
    pub job_id: String,
    pub job_type: String,
    pub target_story_id: String,
    pub sequence: u64,
    /// Transfer-`verified`-only: the AC2 confirmation summary. The UI renders the
    /// success straight from this event (no stale-identifier re-read). ABSENT for
    /// preparation `job:completed` (no verified summary).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<JobCompletedSummary>,
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
    /// Transfer-only: `"failed"` (the device stayed intact) vs `"incomplete"` (a
    /// possible partial copy on the device). ABSENT for flows without the
    /// distinction (preparation) — the UI then renders a plain recoverable
    /// failure. The structured truth still comes from the authoritative re-read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completeness: Option<String>,
    /// Transfer-only: the closed structured cause (camelCase, matching
    /// `TransferCauseDto`) so the UI keeps "cause + issue + next action" in context
    /// (AC3) instead of only the message. ABSENT for preparation and for the
    /// non-classifiable defensive terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    /// Verify-only: the `verify` verdict discriminant — `"partial"` (`état
    /// partiel`) or `"failed"` (`échec récupérable`). PRESENT only on a
    /// verify-phase terminal so the UI renders the right non-success label,
    /// DISTINCT from a write-phase `transfert incomplet` / `échec récupérable`.
    /// ABSENT for write-phase failures and for preparation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_verdict: Option<String>,
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
    fn completed_serializes_camel_case_and_omits_absent_summary() {
        let ev = JobCompletedEvent {
            job_id: "j1".into(),
            job_type: JOB_TYPE_PREPARE_STORY.into(),
            target_story_id: "s1".into(),
            sequence: 3,
            summary: None,
        };
        let v = serde_json::to_value(&ev).expect("ser");
        assert_eq!(v["jobId"], "j1");
        assert_eq!(v["targetStoryId"], "s1");
        assert_eq!(v["sequence"], 3);
        assert!(
            v.get("summary").is_none(),
            "a preparation completion carries no verified summary"
        );
    }

    #[test]
    fn completed_carries_the_verified_summary_when_present() {
        let ev = JobCompletedEvent {
            job_id: "j1".into(),
            job_type: JOB_TYPE_TRANSFER_STORY.into(),
            target_story_id: "s1".into(),
            sequence: 4,
            summary: Some(JobCompletedSummary {
                changed: "« Mon histoire » est maintenant sur la Lunii.".into(),
                unchanged: "2 autres histoires de l'appareil restent inchangées.".into(),
            }),
        };
        let v = serde_json::to_value(&ev).expect("ser");
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
    fn failed_serializes_camel_case_with_non_empty_action() {
        let ev = JobFailedEvent {
            job_id: "j1".into(),
            job_type: JOB_TYPE_PREPARE_STORY.into(),
            target_story_id: "s1".into(),
            sequence: 2,
            error_code: PREPARATION_FAILED_CODE.into(),
            error_message: "Préparation interrompue.".into(),
            user_action: "Relance la préparation.".into(),
            completeness: None,
            cause: None,
            verify_verdict: None,
        };
        let v = serde_json::to_value(&ev).expect("ser");
        assert_eq!(v["errorCode"], "PREPARATION_FAILED");
        assert!(!v["errorMessage"].as_str().expect("msg").is_empty());
        assert!(!v["userAction"].as_str().expect("action").is_empty());
        assert!(v.get("error_code").is_none(), "snake_case must not leak");
        // A preparation failure carries NO completeness — the field is omitted.
        assert!(
            v.get("completeness").is_none(),
            "completeness omitted when None"
        );
        assert!(v.get("cause").is_none(), "cause omitted when None");
        assert!(
            v.get("verifyVerdict").is_none(),
            "verifyVerdict omitted when None"
        );
    }

    #[test]
    fn failed_carries_transfer_completeness_when_present() {
        let ev = JobFailedEvent {
            job_id: "j1".into(),
            job_type: JOB_TYPE_TRANSFER_STORY.into(),
            target_story_id: "s1".into(),
            sequence: 4,
            error_code: TRANSFER_FAILED_CODE.into(),
            error_message: "Envoi incomplet.".into(),
            user_action: "Relance l'envoi.".into(),
            completeness: Some("incomplete".into()),
            cause: Some("writeRejected".into()),
            verify_verdict: None,
        };
        let v = serde_json::to_value(&ev).expect("ser");
        assert_eq!(v["completeness"], "incomplete");
        assert_eq!(v["cause"], "writeRejected");
        assert!(
            v.get("verifyVerdict").is_none(),
            "a write-phase failure carries no verify verdict"
        );
    }

    #[test]
    fn failed_carries_the_verify_verdict_when_present() {
        let ev = JobFailedEvent {
            job_id: "j1".into(),
            job_type: JOB_TYPE_TRANSFER_STORY.into(),
            target_story_id: "s1".into(),
            sequence: 4,
            error_code: TRANSFER_FAILED_CODE.into(),
            error_message: "Envoi dans un état partiel.".into(),
            user_action: "Relance l'envoi.".into(),
            // A verify terminal carries ONLY the verdict — never a write-phase
            // completeness/cause (those describe how a WRITE ended, not a re-read).
            completeness: None,
            cause: None,
            verify_verdict: Some("partial".into()),
        };
        let v = serde_json::to_value(&ev).expect("ser");
        assert_eq!(v["verifyVerdict"], "partial");
        assert!(v.get("completeness").is_none());
        assert!(v.get("cause").is_none());
    }

    #[test]
    fn event_names_are_lowercase_colon_separated() {
        assert_eq!(EVENT_JOB_PROGRESS, "job:progress");
        assert_eq!(EVENT_JOB_COMPLETED, "job:completed");
        assert_eq!(EVENT_JOB_FAILED, "job:failed");
        assert_eq!(EVENT_OS_OPEN_REQUESTED, "os-open:requested");
    }

    #[test]
    fn os_open_requested_payload_is_an_empty_versionable_object() {
        let v = serde_json::to_value(OsOpenRequestedEvent {}).expect("ser");
        assert_eq!(v, json!({}));
    }
}
