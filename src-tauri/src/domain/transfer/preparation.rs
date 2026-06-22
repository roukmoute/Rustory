//! Story-preparation domain (pure, framework-free).
//!
//! Describes the artifacts a transfer of a local story WOULD need and the
//! closed outcomes of assembling them. Strictly independent of
//! `infrastructure/`, `application/` and `tauri::*`: the infrastructure layer
//! enumerates/reads the bytes, the application layer orchestrates the job, and
//! the IPC layer maps these types to wire DTOs.
//!
//! Decision reminders (see `docs/architecture/ui-states.md#Story Preparation
//! Contract`): preparation is a LOCAL operation producing DERIVED artifacts —
//! it never writes to the device and is orthogonal to the `WriteStory` gate.
//! The descriptor is EPHEMERAL and re-derivable (no persistence in MVP). In MVP
//! there is NO media transcoding: the substance is the observable phase
//! progression plus a genuine enumerate + integrity re-check producing the
//! descriptor below.

use crate::domain::story::{CanonicalBlocker, Severity};

/// Version stamped on every produced [`TransferArtifactDescriptor`]. Bumping it
/// invalidates any cached/derived artifact keyed on it (architecture cache
/// strategy). It stays `1` for the whole MVP — no real consumer caches yet.
pub const PREPARATION_PIPELINE_VERSION: u32 = 1;

/// The observable phases of the transfer state machine emitted by the
/// preparation flow. `Transfer` and `Verify` belong to the machine too but are
/// OUT OF SCOPE here (a later write / verification step owns them) — they are
/// deliberately NOT represented so no caller can emit false coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparationPhase {
    /// Re-runs the read-only validation + authoritative device re-scan before
    /// any assembly. Maps to the `en vérification` UI label.
    Preflight,
    /// Local artifact assembly + integrity re-check. Maps to `en préparation`.
    Prepare,
}

impl PreparationPhase {
    /// Stable camelCase wire/log tag. The UI maps it to a French label; this
    /// token never reaches the user verbatim.
    pub const fn wire_tag(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Prepare => "prepare",
        }
    }
}

/// Kind of a single prepared artifact. Internal to the descriptor — never
/// serialized to the wire (the `prepared` DTO exposes only the cohort + story).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedArtifactKind {
    /// The canonical structure of a native minimal story (the only artifact a
    /// native story needs).
    CanonicalStructure,
    /// One file of an imported raw pack (already in device format).
    PackFile,
}

/// One artifact a transfer would carry: a REFERENCE plus its verified size and
/// checksum — never the duplicated bytes. `relative_ref` is the artifact's
/// path relative to its owning store (e.g. `structure.json` for a native story,
/// or the pack-relative path for an imported file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedArtifact {
    pub kind: PreparedArtifactKind,
    pub relative_ref: String,
    pub byte_len: u64,
    pub checksum: String,
}

/// The ephemeral descriptor of what a transfer would need: references +
/// checksums, stamped with the pipeline version and the target cohort the
/// later write step will re-verify. Never persisted in MVP, re-derived on
/// demand (freshness over caching).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferArtifactDescriptor {
    pub story_id: String,
    pub target_cohort: String,
    pub pipeline_version: u32,
    pub artifacts: Vec<PreparedArtifact>,
    pub aggregate_checksum: String,
}

/// Closed set of functional preparation-failure causes. A functional failure is
/// the terminal `retryable` state of the job (NOT an `AppError`); each cause
/// maps to one canonical FR `message` + `userAction` at the IPC layer, and to a
/// fixed severity below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparationFailureCause {
    /// The read-only preflight verdict is not `présumée transférable` (a fixable
    /// or blocking validation issue exists). The offending blockers are reported.
    PreflightNotPassing,
    /// A required artifact (a pack file, the pack folder) is absent.
    ArtifactMissing,
    /// An artifact's integrity check failed (re-checksum disagrees with the
    /// recorded value, or its structure is no longer valid).
    ArtifactCorrupt,
    /// The live re-scan no longer resolves to the requested device during the
    /// preflight (unplugged / swapped / unreadable).
    DeviceChanged,
    /// The wall-clock budget was exhausted, or the operation was interrupted
    /// (window close). The local draft is preserved either way.
    Interrupted,
}

impl PreparationFailureCause {
    /// Stable snake_case wire/log tag — the closed identifier support greps on,
    /// never a localized message.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::PreflightNotPassing => "preflight_not_passing",
            Self::ArtifactMissing => "artifact_missing",
            Self::ArtifactCorrupt => "artifact_corrupt",
            Self::DeviceChanged => "device_changed",
            Self::Interrupted => "interrupted",
        }
    }

    /// Frozen severity per cause (reuses the canonical-validity vocabulary). It
    /// does NOT change the UI rendering — every preparation failure surfaces as
    /// `échec récupérable` with `Relancer` — but it labels the cause for traces
    /// and keeps the cause→severity mapping under test. `Blocking` marks a data
    /// integrity problem (a fresh transfer is needed once the data is restored);
    /// `Fixable` marks a problem the user can clear with a direct gesture
    /// (repair validation, re-plug the device, retry).
    pub const fn severity(self) -> Severity {
        match self {
            Self::PreflightNotPassing | Self::DeviceChanged | Self::Interrupted => {
                Severity::Fixable
            }
            Self::ArtifactMissing | Self::ArtifactCorrupt => Severity::Blocking,
        }
    }

    /// Single canonical FR copy per cause: `(message, userAction)`. The SAME
    /// pair feeds the `job:failed` event (application layer) AND the `retryable`
    /// preparation DTO (IPC layer) — never two wordings for one cause. The UI
    /// renders both strings verbatim and adds the `Relancer` gesture.
    pub const fn copy(self) -> (&'static str, &'static str) {
        match self {
            Self::PreflightNotPassing => (
                "La préparation ne peut pas démarrer : l'histoire n'a pas passé la vérification.",
                "Corrige les points signalés puis relance la préparation.",
            ),
            Self::ArtifactMissing => (
                "Préparation impossible : un fichier nécessaire au transfert est introuvable.",
                "Vérifie l'histoire locale puis relance la préparation.",
            ),
            Self::ArtifactCorrupt => (
                "Préparation impossible : un fichier nécessaire au transfert est altéré ou illisible.",
                "Restaure une version saine de l'histoire puis relance la préparation.",
            ),
            Self::DeviceChanged => (
                "Préparation interrompue : l'appareil connecté a changé.",
                "Rebranche la Lunii souhaitée puis relance la préparation.",
            ),
            Self::Interrupted => (
                "Préparation interrompue avant la fin.",
                "Relance la préparation.",
            ),
        }
    }
}

/// Decide whether a read-only preflight authorises the prepare phase.
///
/// Fail-closed: the prepare phase proceeds ONLY from a fully-clear preflight —
/// the `présumée transférable` verdict, which in MVP means NO canonical blocker
/// at all. Any fixable (`à corriger`) or blocking (`bloquée`) issue stops at
/// [`PreparationFailureCause::PreflightNotPassing`]; `Préparer` never runs a
/// best-effort preparation on a story that did not pass validation. An
/// empty-`nodes` story is canonically valid (the v1 form), so it has no blocker
/// and proceeds.
pub fn gate_prepare(
    canonical_blockers: &[CanonicalBlocker],
) -> Result<(), PreparationFailureCause> {
    if canonical_blockers.is_empty() {
        Ok(())
    } else {
        Err(PreparationFailureCause::PreflightNotPassing)
    }
}

/// Sanity-check an assembled descriptor before declaring the story `prepared`.
///
/// A coherent descriptor carries the current pipeline version, at least one
/// artifact (even a native minimal story has its canonical structure), a
/// non-empty aggregate checksum, and a non-empty reference + checksum on every
/// artifact. An incoherent descriptor is treated as
/// [`PreparationFailureCause::ArtifactCorrupt`] — never silently declared ready.
pub fn ensure_descriptor_coherent(
    descriptor: &TransferArtifactDescriptor,
) -> Result<(), PreparationFailureCause> {
    let coherent = descriptor.pipeline_version == PREPARATION_PIPELINE_VERSION
        && !descriptor.artifacts.is_empty()
        && !descriptor.aggregate_checksum.is_empty()
        && descriptor
            .artifacts
            .iter()
            .all(|a| !a.relative_ref.is_empty() && !a.checksum.is_empty());
    if coherent {
        Ok(())
    } else {
        Err(PreparationFailureCause::ArtifactCorrupt)
    }
}

/// Verify a freshly-assembled descriptor against the integrity baseline the
/// import (or the canonical store) recorded for it.
///
/// The descriptor's `aggregate_checksum` is recomputed from the artifacts on
/// disk; `expected_aggregate` is the value recorded when the artifacts were
/// first acquired (`story_imports.pack_checksum` for an imported pack, the
/// canonical `content_checksum` for a native story). A mismatch is silent
/// on-disk corruption → [`PreparationFailureCause::ArtifactCorrupt`]. Keeping
/// this comparison a pure function lets the assembler stay a plain producer and
/// the corruption path stay unit-testable without duplicating the hash.
pub fn verify_aggregate(
    descriptor: &TransferArtifactDescriptor,
    expected_aggregate: &str,
) -> Result<(), PreparationFailureCause> {
    if descriptor.aggregate_checksum == expected_aggregate {
        Ok(())
    } else {
        Err(PreparationFailureCause::ArtifactCorrupt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::story::{Axis, CanonicalCause};

    fn blocker(cause: CanonicalCause) -> CanonicalBlocker {
        CanonicalBlocker {
            axis: Axis::Structure,
            cause,
            severity: cause.severity(),
        }
    }

    fn native_descriptor() -> TransferArtifactDescriptor {
        TransferArtifactDescriptor {
            story_id: "0197a5d0-0000-7000-8000-000000000000".into(),
            target_cohort: "origine_v1".into(),
            pipeline_version: PREPARATION_PIPELINE_VERSION,
            artifacts: vec![PreparedArtifact {
                kind: PreparedArtifactKind::CanonicalStructure,
                relative_ref: "structure.json".into(),
                byte_len: 30,
                checksum: "a".repeat(64),
            }],
            aggregate_checksum: "a".repeat(64),
        }
    }

    #[test]
    fn passing_preflight_authorises_prepare() {
        assert!(gate_prepare(&[]).is_ok());
    }

    #[test]
    fn to_fix_preflight_blocks_prepare_with_preflight_not_passing() {
        // A purely fixable issue (invalid title) still stops preparation.
        let err = gate_prepare(&[blocker(CanonicalCause::TitleInvalid)])
            .expect_err("a fixable blocker must stop prepare");
        assert_eq!(err, PreparationFailureCause::PreflightNotPassing);
    }

    #[test]
    fn blocked_preflight_blocks_prepare_with_preflight_not_passing() {
        let err = gate_prepare(&[blocker(CanonicalCause::ChecksumMismatch)])
            .expect_err("a blocking issue must stop prepare");
        assert_eq!(err, PreparationFailureCause::PreflightNotPassing);
    }

    #[test]
    fn native_minimal_descriptor_is_coherent() {
        // A native minimal story (structure only) yields a coherent descriptor —
        // an empty `nodes` is valid, never a blocker.
        assert!(ensure_descriptor_coherent(&native_descriptor()).is_ok());
    }

    #[test]
    fn empty_artifact_set_is_incoherent() {
        let mut d = native_descriptor();
        d.artifacts.clear();
        assert_eq!(
            ensure_descriptor_coherent(&d).expect_err("no artifact must be incoherent"),
            PreparationFailureCause::ArtifactCorrupt
        );
    }

    #[test]
    fn stale_pipeline_version_is_incoherent() {
        let mut d = native_descriptor();
        d.pipeline_version = PREPARATION_PIPELINE_VERSION + 1;
        assert_eq!(
            ensure_descriptor_coherent(&d)
                .expect_err("a stale pipeline version must be incoherent"),
            PreparationFailureCause::ArtifactCorrupt
        );
    }

    #[test]
    fn an_artifact_without_checksum_is_incoherent() {
        let mut d = native_descriptor();
        d.artifacts[0].checksum = String::new();
        assert!(ensure_descriptor_coherent(&d).is_err());
    }

    #[test]
    fn failure_cause_severity_mapping_is_frozen() {
        assert_eq!(
            PreparationFailureCause::PreflightNotPassing.severity(),
            Severity::Fixable
        );
        assert_eq!(
            PreparationFailureCause::DeviceChanged.severity(),
            Severity::Fixable
        );
        assert_eq!(
            PreparationFailureCause::Interrupted.severity(),
            Severity::Fixable
        );
        assert_eq!(
            PreparationFailureCause::ArtifactMissing.severity(),
            Severity::Blocking
        );
        assert_eq!(
            PreparationFailureCause::ArtifactCorrupt.severity(),
            Severity::Blocking
        );
    }

    #[test]
    fn every_failure_cause_has_non_empty_copy() {
        for cause in [
            PreparationFailureCause::PreflightNotPassing,
            PreparationFailureCause::ArtifactMissing,
            PreparationFailureCause::ArtifactCorrupt,
            PreparationFailureCause::DeviceChanged,
            PreparationFailureCause::Interrupted,
        ] {
            let (message, action) = cause.copy();
            assert!(!message.is_empty(), "{cause:?} message empty");
            assert!(!action.is_empty(), "{cause:?} userAction empty");
        }
    }

    #[test]
    fn failure_cause_diagnostic_tags_are_stable_and_distinct() {
        let tags = [
            PreparationFailureCause::PreflightNotPassing.diagnostic_tag(),
            PreparationFailureCause::ArtifactMissing.diagnostic_tag(),
            PreparationFailureCause::ArtifactCorrupt.diagnostic_tag(),
            PreparationFailureCause::DeviceChanged.diagnostic_tag(),
            PreparationFailureCause::Interrupted.diagnostic_tag(),
        ];
        let mut unique = tags.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), tags.len(), "tags must be distinct");
        assert!(tags.iter().all(|t| !t.is_empty()));
    }

    #[test]
    fn phase_wire_tags_are_stable() {
        assert_eq!(PreparationPhase::Preflight.wire_tag(), "preflight");
        assert_eq!(PreparationPhase::Prepare.wire_tag(), "prepare");
    }

    #[test]
    fn pipeline_version_is_one_in_mvp() {
        assert_eq!(PREPARATION_PIPELINE_VERSION, 1);
    }

    #[test]
    fn verify_aggregate_passes_on_match_and_fails_corrupt_on_mismatch() {
        let d = native_descriptor();
        assert!(verify_aggregate(&d, &d.aggregate_checksum).is_ok());
        assert_eq!(
            verify_aggregate(&d, &"b".repeat(64)).expect_err("mismatch must be corrupt"),
            PreparationFailureCause::ArtifactCorrupt
        );
    }
}
