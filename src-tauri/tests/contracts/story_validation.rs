//! Wire-shape contract for the `read_story_validation` command DTO. Pure
//! serialization assertions — the cross-stack mirror is
//! `src/shared/ipc-contracts/story-validation.ts`, kept symmetric by these
//! tests + the runtime guard `isStoryValidationDto`.

use rustory_lib::application::device::check_operation_allowed;
use rustory_lib::application::device::preflight::{
    Blocker, BlockerCause, StoryValidationOutcome, Verdict,
};
use rustory_lib::domain::device::{
    classify_lunii, DeviceProfileClassification, LuniiFirmwareCohort, SupportedOperation,
    UnsupportedReason,
};
use rustory_lib::domain::story::{Axis, CanonicalCause, Severity};
use rustory_lib::ipc::dto::{ReadStoryValidationInputDto, StoryValidationDto};

const VALID_ID: &str = "0123456789abcdef0123456789abcdef";
const STORY_ID: &str = "0197a5d0-0000-7000-8000-000000000000";

fn ready_json(verdict: Verdict, blockers: Vec<Blocker>) -> serde_json::Value {
    let dto = StoryValidationDto::from_outcome(StoryValidationOutcome::Ready {
        device_identifier: VALID_ID.into(),
        story_id: STORY_ID.into(),
        story_title: "Mon histoire".into(),
        verdict,
        blockers,
    });
    serde_json::to_value(&dto).expect("ser")
}

fn canonical(cause: CanonicalCause, severity: Severity) -> Blocker {
    Blocker {
        axis: Axis::Structure,
        cause: BlockerCause::Canonical(cause),
        severity,
    }
}

fn device_profile(reason: UnsupportedReason) -> Blocker {
    Blocker {
        axis: Axis::DeviceProfile,
        cause: BlockerCause::DeviceProfile(reason),
        severity: Severity::Blocking,
    }
}

#[test]
fn no_device_serializes_with_kind_only() {
    let v = serde_json::to_value(StoryValidationDto::NoDevice).expect("ser");
    assert_eq!(v, serde_json::json!({ "kind": "noDevice" }));
    assert_eq!(v.as_object().expect("obj").len(), 1);
}

#[test]
fn presumed_transferable_round_trips_wire_shape() {
    let v = ready_json(Verdict::PresumedTransferable, vec![]);
    assert_eq!(
        v,
        serde_json::json!({
            "kind": "ready",
            "deviceIdentifier": VALID_ID,
            "story": { "id": STORY_ID, "title": "Mon histoire" },
            "verdict": "presumedTransferable",
            "blockers": [],
        })
    );
}

#[test]
fn each_verdict_serializes_in_camel_case() {
    for (verdict, expected) in [
        (Verdict::PresumedTransferable, "presumedTransferable"),
        (Verdict::ToFix, "toFix"),
        (Verdict::Blocked, "blocked"),
    ] {
        let v = ready_json(verdict, vec![]);
        assert_eq!(v["verdict"], expected);
    }
}

#[test]
fn ready_uses_camel_case_only() {
    let v = ready_json(Verdict::PresumedTransferable, vec![]);
    for camel in ["deviceIdentifier", "story", "verdict", "blockers"] {
        assert!(v.get(camel).is_some(), "missing camelCase field: {camel}");
    }
    for snake in ["device_identifier", "story_title"] {
        assert!(v.get(snake).is_none(), "snake_case must not leak: {snake}");
    }
}

#[test]
fn a_canonical_blocker_carries_axis_cause_message_and_user_action() {
    let v = ready_json(
        Verdict::ToFix,
        vec![canonical(CanonicalCause::TitleInvalid, Severity::Fixable)],
    );
    let b = &v["blockers"][0];
    assert_eq!(b["axis"], "structure");
    assert_eq!(b["cause"], "titleInvalid");
    assert!(!b["message"].as_str().expect("message").is_empty());
    let action = b["userAction"].as_str().expect("userAction");
    assert!(
        !action.is_empty(),
        "every blocker must carry a next gesture"
    );
    // camelCase only — no snake_case leak on the blocker either.
    assert!(b.get("user_action").is_none());
}

#[test]
fn a_device_profile_blocker_carries_the_camel_case_cause() {
    let v = ready_json(
        Verdict::Blocked,
        vec![device_profile(UnsupportedReason::MetadataUnsupported)],
    );
    let b = &v["blockers"][0];
    assert_eq!(b["axis"], "deviceProfile");
    assert_eq!(b["cause"], "metadataUnsupported");
    assert!(!b["message"].as_str().expect("message").is_empty());
    assert!(!b["userAction"].as_str().expect("userAction").is_empty());
}

#[test]
fn every_blocker_cause_serializes_with_a_non_empty_user_action() {
    let blockers = vec![
        canonical(CanonicalCause::TitleInvalid, Severity::Fixable),
        canonical(CanonicalCause::SchemaUnsupported, Severity::Blocking),
        canonical(CanonicalCause::StructureCorrupt, Severity::Blocking),
        canonical(CanonicalCause::ChecksumMismatch, Severity::Blocking),
        device_profile(UnsupportedReason::FirmwareUnsupported),
        device_profile(UnsupportedReason::MetadataUnsupported),
        device_profile(UnsupportedReason::MetadataCorrupt),
        device_profile(UnsupportedReason::FamilyUnknown),
        device_profile(UnsupportedReason::OperationNotAuthorized),
        device_profile(UnsupportedReason::MultipleCandidates),
    ];
    let v = ready_json(Verdict::Blocked, blockers);
    let arr = v["blockers"].as_array().expect("blockers array");
    assert_eq!(arr.len(), 10);
    for b in arr {
        assert!(
            !b["message"].as_str().expect("message").is_empty(),
            "blocker {b} has empty message"
        );
        assert!(
            !b["userAction"].as_str().expect("userAction").is_empty(),
            "blocker {b} has empty userAction"
        );
        // The cause is from the documented closed set (a known camelCase token).
        assert!(b["cause"].is_string());
    }
}

#[test]
fn input_accepts_canonical_camel_case_payload() {
    let dto: ReadStoryValidationInputDto = serde_json::from_value(serde_json::json!({
        "storyId": STORY_ID,
        "deviceIdentifier": VALID_ID,
    }))
    .expect("deser");
    assert_eq!(dto.story_id, STORY_ID);
    assert_eq!(dto.device_identifier, VALID_ID);
}

#[test]
fn input_rejects_unknown_field_no_path_crosses_ipc() {
    let err = serde_json::from_value::<ReadStoryValidationInputDto>(serde_json::json!({
        "storyId": "x",
        "deviceIdentifier": "y",
        "mountPath": "/sneaky",
    }))
    .expect_err("must reject unknown field");
    assert!(err.to_string().contains("mountPath"));
}

#[test]
fn write_capability_is_governed_by_the_cohort_gate_not_the_verdict() {
    // FR34 / AC2: write authorization is decided SOLELY by the capability gate
    // per cohort — never by a validity verdict. Epic 3 wired the gate: V1/V2 are
    // writable, V3 stays refused (reverse-engineering still active). A `présumée
    // transférable` verdict on a V3 device therefore still cannot reach a write.
    for (cohort, version, writable) in [
        (LuniiFirmwareCohort::OrigineV1, 3u8, true),
        (LuniiFirmwareCohort::MidGenV2, 6, true),
        (LuniiFirmwareCohort::V3, 7, false),
    ] {
        let profile = match classify_lunii(version, true, true, "deadbeefdeadbeef") {
            DeviceProfileClassification::Supported(p) => p,
            other => panic!("md v{version} must classify as supported, got {other:?}"),
        };
        assert_eq!(
            profile.firmware_cohort,
            rustory_lib::domain::device::FirmwareCohort::Lunii(cohort)
        );
        let result = check_operation_allowed(&profile, SupportedOperation::WriteStory);
        if writable {
            result.expect("V1/V2 write must be allowed by the gate");
        } else {
            let err = result.expect_err("V3 write must stay refused");
            let v = serde_json::to_value(&err).expect("ser");
            assert_eq!(v["code"], "DEVICE_UNSUPPORTED");
            assert_eq!(v["details"]["operation"], "write_story");
        }
    }
}
