use serde::{Deserialize, Serialize};

use crate::application::device::preflight::{
    Blocker, BlockerCause, StoryValidationOutcome, Verdict,
};
use crate::domain::device::UnsupportedReason;
use crate::domain::story::{Axis, CanonicalBlocker, CanonicalCause};

/// Input accepted by the `read_story_validation` Tauri command.
/// `deny_unknown_fields` fails deserialization if the UI ever sends a field
/// ahead of the Rust contract, so the boundary stays authoritative.
///
/// Exactly the two identifiers the UI legitimately holds: the local `story_id`
/// (the selected story) and the opaque hashed `device_identifier` from
/// detection. No path, no pack short id — Rust re-resolves the rest.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadStoryValidationInputDto {
    pub story_id: String,
    pub device_identifier: String,
}

/// The selected local story, identified for the verdict. Title is the local
/// `stories.title` (no recognition needed — the user owns this story).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoryValidationStoryDto {
    pub id: String,
    pub title: String,
}

/// The composed verdict (AC1/AC3). The frontend maps it to a label + chip tone,
/// never recomputes it.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VerdictDto {
    PresumedTransferable,
    ToFix,
    Blocked,
}

/// The two-axis taxonomy (AC1): canonical validity (`structure` / `media` /
/// `filesystem`) vs Lunii compatibility (`deviceProfile`).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BlockerAxisDto {
    Structure,
    Media,
    Filesystem,
    DeviceProfile,
}

/// Closed set of blocker causes spanning both axes (AC2). Never a free-form
/// string — the UI branches on this discriminant, never on the message text.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BlockerCauseDto {
    TitleInvalid,
    SchemaUnsupported,
    StructureCorrupt,
    ChecksumMismatch,
    MetadataUnsupported,
    MetadataCorrupt,
    FamilyUnknown,
    MultipleCandidates,
    FirmwareUnsupported,
    OperationNotAuthorized,
}

/// A single blocker (AC2): a closed `(axis, cause)` pair plus the canonical FR
/// `message` (cause + impact) and `userAction` (next gesture). Both strings are
/// Rust-authoritative and rendered verbatim — never empty.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlockerDto {
    pub axis: BlockerAxisDto,
    pub cause: BlockerCauseDto,
    pub message: String,
    pub user_action: String,
}

/// Wire shape returned by the `read_story_validation` Tauri command.
///
/// Tagged enum on `kind`: `"noDevice"`, `"ready"`. All field names are
/// camelCase. The frontend mirror lives at
/// `src/shared/ipc-contracts/story-validation.ts` — drift is enforced by the
/// contract tests in `src-tauri/tests/contracts/story_validation.rs` AND the
/// runtime guard `isStoryValidationDto`.
///
/// There is no `unsupported` variant: an unreadable / ambiguous / unsupported
/// device profile is a `deviceProfile` blocker inside `ready`, so the canonical
/// axis stays visible alongside (AC1). The OS mount path is never part of this
/// DTO.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StoryValidationDto {
    NoDevice,
    #[serde(rename_all = "camelCase")]
    Ready {
        device_identifier: String,
        story: StoryValidationStoryDto,
        verdict: VerdictDto,
        blockers: Vec<BlockerDto>,
    },
}

impl StoryValidationDto {
    /// Map the application outcome to the wire shape, generating the canonical
    /// FR `message` / `userAction` per cause.
    pub fn from_outcome(outcome: StoryValidationOutcome) -> Self {
        match outcome {
            StoryValidationOutcome::NoDevice => Self::NoDevice,
            StoryValidationOutcome::Ready {
                device_identifier,
                story_id,
                story_title,
                verdict,
                blockers,
            } => Self::Ready {
                device_identifier,
                story: StoryValidationStoryDto {
                    id: story_id,
                    title: story_title,
                },
                verdict: verdict_dto(verdict),
                blockers: blockers.iter().map(blocker_dto).collect(),
            },
        }
    }
}

fn verdict_dto(verdict: Verdict) -> VerdictDto {
    match verdict {
        Verdict::PresumedTransferable => VerdictDto::PresumedTransferable,
        Verdict::ToFix => VerdictDto::ToFix,
        Verdict::Blocked => VerdictDto::Blocked,
    }
}

fn axis_dto(axis: Axis) -> BlockerAxisDto {
    match axis {
        Axis::Structure => BlockerAxisDto::Structure,
        Axis::Media => BlockerAxisDto::Media,
        Axis::Filesystem => BlockerAxisDto::Filesystem,
        Axis::DeviceProfile => BlockerAxisDto::DeviceProfile,
    }
}

fn blocker_dto(blocker: &Blocker) -> BlockerDto {
    let (cause, message, user_action) = cause_copy(&blocker.cause);
    BlockerDto {
        axis: axis_dto(blocker.axis),
        cause,
        message: message.to_string(),
        user_action: user_action.to_string(),
    }
}

/// Map a DOMAIN canonical blocker (`domain::story::CanonicalBlocker`) to the wire
/// `BlockerDto`, reusing the SINGLE canonical FR copy per cause. Shared with the
/// story-preparation flow, where a non-passing `preflight` reports its canonical
/// blockers verbatim — never a second wording for the same cause.
pub fn canonical_blocker_dto(blocker: &CanonicalBlocker) -> BlockerDto {
    let (cause, message, user_action) = canonical_copy(&blocker.cause);
    BlockerDto {
        axis: axis_dto(blocker.axis),
        cause,
        message: message.to_string(),
        user_action: user_action.to_string(),
    }
}

/// Single canonical FR copy per cause — never two wordings for one cause.
/// Mirrors `docs/architecture/ui-states.md#Story Validation / Preflight
/// Contract`.
fn cause_copy(cause: &BlockerCause) -> (BlockerCauseDto, &'static str, &'static str) {
    match cause {
        BlockerCause::Canonical(c) => canonical_copy(c),
        BlockerCause::DeviceProfile(r) => device_profile_copy(r),
    }
}

fn canonical_copy(cause: &CanonicalCause) -> (BlockerCauseDto, &'static str, &'static str) {
    match cause {
        CanonicalCause::TitleInvalid => (
            BlockerCauseDto::TitleInvalid,
            "Le titre enregistré de l'histoire n'est pas valide.",
            "Renomme l'histoire avec un titre valide puis relance la vérification.",
        ),
        CanonicalCause::SchemaUnsupported => (
            BlockerCauseDto::SchemaUnsupported,
            "Cette histoire utilise un format plus récent que celui pris en charge par cette version de Rustory.",
            "Mets à jour Rustory pour transférer cette histoire.",
        ),
        CanonicalCause::StructureCorrupt => (
            BlockerCauseDto::StructureCorrupt,
            "La structure interne de l'histoire est illisible ou incohérente.",
            "Restaure une version saine de l'histoire puis relance la vérification.",
        ),
        CanonicalCause::ChecksumMismatch => (
            BlockerCauseDto::ChecksumMismatch,
            "Les données locales de l'histoire ont changé de façon inattendue (corruption détectée).",
            "Restaure une sauvegarde saine de l'histoire avant de la transférer.",
        ),
    }
}

fn device_profile_copy(
    reason: &UnsupportedReason,
) -> (BlockerCauseDto, &'static str, &'static str) {
    match reason {
        UnsupportedReason::MetadataUnsupported => (
            BlockerCauseDto::MetadataUnsupported,
            "Le profil de la Lunii connectée n'est pas pris en charge.",
            "Consulte le profil de support pour voir les Lunii compatibles.",
        ),
        UnsupportedReason::MetadataCorrupt => (
            BlockerCauseDto::MetadataCorrupt,
            "Les marqueurs de la Lunii connectée sont incomplets ou illisibles.",
            "Rebranche la Lunii puis relance la vérification.",
        ),
        UnsupportedReason::FamilyUnknown => (
            BlockerCauseDto::FamilyUnknown,
            "La famille de l'appareil connecté n'est pas reconnue.",
            "Branche une Lunii prise en charge puis relance la vérification.",
        ),
        UnsupportedReason::MultipleCandidates => (
            BlockerCauseDto::MultipleCandidates,
            "Plusieurs Lunii compatibles sont connectées en même temps.",
            "Ne garde qu'une seule Lunii branchée puis relance la vérification.",
        ),
        UnsupportedReason::FirmwareUnsupported => (
            BlockerCauseDto::FirmwareUnsupported,
            "Le firmware de la Lunii connectée n'est pas pris en charge.",
            "Consulte le profil de support pour les firmwares compatibles.",
        ),
        UnsupportedReason::OperationNotAuthorized => (
            BlockerCauseDto::OperationNotAuthorized,
            "Le profil détecté n'autorise pas la lecture de la bibliothèque de l'appareil.",
            "Consulte le profil de support pour comprendre ce qui est permis.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::device::preflight::Blocker as AppBlocker;
    use crate::domain::story::Severity;
    use serde_json::json;

    const VALID_ID: &str = "0123456789abcdef0123456789abcdef";

    fn ready(verdict: Verdict, blockers: Vec<AppBlocker>) -> StoryValidationOutcome {
        StoryValidationOutcome::Ready {
            device_identifier: VALID_ID.into(),
            story_id: "0197a5d0-0000-7000-8000-000000000000".into(),
            story_title: "Mon histoire".into(),
            verdict,
            blockers,
        }
    }

    #[test]
    fn no_device_serializes_with_single_kind_key() {
        let v = serde_json::to_value(StoryValidationDto::NoDevice).expect("ser");
        assert_eq!(v, json!({ "kind": "noDevice" }));
        assert_eq!(v.as_object().expect("obj").len(), 1);
    }

    #[test]
    fn presumed_transferable_round_trips_with_camel_case_fields() {
        let dto = StoryValidationDto::from_outcome(ready(Verdict::PresumedTransferable, vec![]));
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "ready");
        assert_eq!(v["deviceIdentifier"], VALID_ID);
        assert_eq!(v["story"]["id"], "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(v["story"]["title"], "Mon histoire");
        assert_eq!(v["verdict"], "presumedTransferable");
        assert_eq!(v["blockers"], json!([]));
        // No snake_case leak.
        assert!(v.get("device_identifier").is_none());
        assert!(v.get("story_title").is_none());
    }

    #[test]
    fn verdict_variants_serialize_in_camel_case() {
        for (verdict, expected) in [
            (Verdict::PresumedTransferable, "presumedTransferable"),
            (Verdict::ToFix, "toFix"),
            (Verdict::Blocked, "blocked"),
        ] {
            let dto = StoryValidationDto::from_outcome(ready(verdict, vec![]));
            let v = serde_json::to_value(&dto).expect("ser");
            assert_eq!(v["verdict"], expected);
        }
    }

    #[test]
    fn canonical_blocker_carries_axis_cause_and_non_empty_copy() {
        let blocker = AppBlocker {
            axis: Axis::Structure,
            cause: BlockerCause::Canonical(CanonicalCause::ChecksumMismatch),
            severity: Severity::Blocking,
        };
        let dto = StoryValidationDto::from_outcome(ready(Verdict::Blocked, vec![blocker]));
        let v = serde_json::to_value(&dto).expect("ser");
        let b = &v["blockers"][0];
        assert_eq!(b["axis"], "structure");
        assert_eq!(b["cause"], "checksumMismatch");
        assert!(!b["message"].as_str().expect("message").is_empty());
        assert!(!b["userAction"].as_str().expect("userAction").is_empty());
        // camelCase only.
        assert!(b.get("user_action").is_none());
    }

    #[test]
    fn device_profile_blocker_maps_reason_to_camel_case_cause() {
        let blocker = AppBlocker {
            axis: Axis::DeviceProfile,
            cause: BlockerCause::DeviceProfile(UnsupportedReason::MultipleCandidates),
            severity: Severity::Blocking,
        };
        let dto = StoryValidationDto::from_outcome(ready(Verdict::Blocked, vec![blocker]));
        let v = serde_json::to_value(&dto).expect("ser");
        let b = &v["blockers"][0];
        assert_eq!(b["axis"], "deviceProfile");
        assert_eq!(b["cause"], "multipleCandidates");
        assert!(!b["message"].as_str().expect("message").is_empty());
        assert!(!b["userAction"].as_str().expect("userAction").is_empty());
    }

    #[test]
    fn every_blocker_cause_has_non_empty_message_and_user_action() {
        let canonical = [
            CanonicalCause::TitleInvalid,
            CanonicalCause::SchemaUnsupported,
            CanonicalCause::StructureCorrupt,
            CanonicalCause::ChecksumMismatch,
        ];
        for c in canonical {
            let (_, message, action) = canonical_copy(&c);
            assert!(!message.is_empty(), "{c:?} message empty");
            assert!(!action.is_empty(), "{c:?} userAction empty");
        }
        let reasons = [
            UnsupportedReason::FirmwareUnsupported,
            UnsupportedReason::MetadataUnsupported,
            UnsupportedReason::MetadataCorrupt,
            UnsupportedReason::FamilyUnknown,
            UnsupportedReason::OperationNotAuthorized,
            UnsupportedReason::MultipleCandidates,
        ];
        for r in reasons {
            let (_, message, action) = device_profile_copy(&r);
            assert!(!message.is_empty(), "{r:?} message empty");
            assert!(!action.is_empty(), "{r:?} userAction empty");
        }
    }

    #[test]
    fn input_accepts_canonical_camel_case_payload() {
        let dto: ReadStoryValidationInputDto = serde_json::from_value(json!({
            "storyId": "0197a5d0-0000-7000-8000-000000000000",
            "deviceIdentifier": VALID_ID,
        }))
        .expect("deser");
        assert_eq!(dto.story_id, "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(dto.device_identifier, VALID_ID);
    }

    #[test]
    fn input_rejects_snake_case_field() {
        let err = serde_json::from_value::<ReadStoryValidationInputDto>(json!({
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
        let err = serde_json::from_value::<ReadStoryValidationInputDto>(json!({
            "storyId": "x",
            "deviceIdentifier": "y",
            "mountPath": "/sneaky",
        }))
        .expect_err("must reject unknown field — no path crosses IPC");
        assert!(err.to_string().contains("mountPath"));
    }
}
