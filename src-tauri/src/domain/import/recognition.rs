//! Local-artifact import recognition (domain layer).
//!
//! Pure, framework-free classification of an analyzed local artifact into
//! a typed recognition verdict — the import counterpart of `preflight.rs`.
//! A "partially usable" or "functionally blocked" artifact is a RESULT
//! STATE here, never an `AppError`: only transport failures (unreadable
//! file, failed DB write) are errors, surfaced by the application layer.
//!
//! Scope reminder: the only supported artifact in this iteration is the
//! `.rustory` v1 file (a single story, `nodes: []`). The [`Missing`]
//! finding category and the [`Partial`] / [`Resolved`] import states are
//! DECLARED for the deferred structured multi-element import but have NO
//! emitter in the `.rustory` flow — a negative test locks that, mirroring
//! the `Axis::Media` / `Axis::Filesystem` declared-but-unemitted axes in
//! `preflight.rs`.
//!
//! [`Missing`]: RecognitionCategory::Missing
//! [`Partial`]: ImportState::Partial
//! [`Resolved`]: ImportState::Resolved

/// Global recognition quality of an analyzed artifact (UI: `Propre` /
/// `Partiellement exploitable` / `Inexploitable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionQuality {
    /// Every aspect recognized — imports with no marker.
    Clean,
    /// Importable, but one or more aspects need attention (a durable
    /// marker + an on-demand report).
    Partial,
    /// A real blocker prevents a safe import — nothing is added.
    Unusable,
}

/// The aspect of the artifact a single finding refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionAspect {
    Envelope,
    FormatVersion,
    SchemaVersion,
    Structure,
    Integrity,
    Title,
    Timestamps,
}

/// The recognition category of a single finding (UI: `reconnu` /
/// `ambiguïté` / `information manquante` / `blocage réel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionCategory {
    /// Understood and accepted.
    Recognized,
    /// Usable but adjusted / not fully trusted (e.g. a normalized title).
    Ambiguous,
    /// An expected aspect is absent. DECLARED for structured imports; the
    /// `.rustory` flow never emits it.
    Missing,
    /// Makes the artifact unusable as-is.
    Blocking,
}

/// Durable per-story import state (calque of the Transfer State Contract;
/// UI chips `reconnu` / `partiel` / `à revoir`, reserved for this flow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportState {
    Recognized,
    /// DECLARED for the deferred structured multi-element import; never
    /// emitted by the `.rustory` flow (which uses [`NeedsReview`] for its
    /// single-story ambiguities).
    ///
    /// [`NeedsReview`]: ImportState::NeedsReview
    Partial,
    NeedsReview,
    Blocked,
    /// DECLARED for guided repair; not emitted in this iteration.
    Resolved,
}

/// A single recognition finding: a closed `(aspect, category)` pair. The
/// IPC layer maps the pair to exactly one canonical FR message + impact —
/// the UI branches on this discriminant, never on free-form text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecognitionFinding {
    pub aspect: RecognitionAspect,
    pub category: RecognitionCategory,
}

impl RecognitionFinding {
    pub fn recognized(aspect: RecognitionAspect) -> Self {
        Self {
            aspect,
            category: RecognitionCategory::Recognized,
        }
    }

    pub fn ambiguous(aspect: RecognitionAspect) -> Self {
        Self {
            aspect,
            category: RecognitionCategory::Ambiguous,
        }
    }

    pub fn blocking(aspect: RecognitionAspect) -> Self {
        Self {
            aspect,
            category: RecognitionCategory::Blocking,
        }
    }
}

/// Derive the global quality from the set of per-aspect findings: any
/// blocking finding makes the artifact `Unusable`; otherwise any ambiguity
/// (or a declared `Missing`) makes it `Partial`; otherwise it is `Clean`.
pub fn recognition_quality(findings: &[RecognitionFinding]) -> RecognitionQuality {
    if findings
        .iter()
        .any(|f| f.category == RecognitionCategory::Blocking)
    {
        RecognitionQuality::Unusable
    } else if findings.iter().any(|f| {
        matches!(
            f.category,
            RecognitionCategory::Ambiguous | RecognitionCategory::Missing
        )
    }) {
        RecognitionQuality::Partial
    } else {
        RecognitionQuality::Clean
    }
}

/// Map the global quality to the durable per-story import state. For a
/// `.rustory` artifact the `Partial` quality always means "review the
/// adjusted aspects" → [`NeedsReview`]; [`Partial`] (some elements usable,
/// some not) is reserved for the deferred multi-element import.
///
/// [`NeedsReview`]: ImportState::NeedsReview
/// [`Partial`]: ImportState::Partial
pub fn import_state(quality: RecognitionQuality) -> ImportState {
    match quality {
        RecognitionQuality::Clean => ImportState::Recognized,
        RecognitionQuality::Partial => ImportState::NeedsReview,
        RecognitionQuality::Unusable => ImportState::Blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_recognized_is_clean() {
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::recognized(RecognitionAspect::Title),
        ];
        assert_eq!(recognition_quality(&findings), RecognitionQuality::Clean);
        assert_eq!(
            import_state(recognition_quality(&findings)),
            ImportState::Recognized
        );
    }

    #[test]
    fn an_ambiguity_makes_it_partial_needs_review() {
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::ambiguous(RecognitionAspect::Title),
        ];
        assert_eq!(recognition_quality(&findings), RecognitionQuality::Partial);
        assert_eq!(
            import_state(recognition_quality(&findings)),
            ImportState::NeedsReview
        );
    }

    #[test]
    fn a_blocker_dominates_an_ambiguity_and_is_unusable_blocked() {
        let findings = [
            RecognitionFinding::ambiguous(RecognitionAspect::Title),
            RecognitionFinding::blocking(RecognitionAspect::Integrity),
        ];
        assert_eq!(recognition_quality(&findings), RecognitionQuality::Unusable);
        assert_eq!(
            import_state(recognition_quality(&findings)),
            ImportState::Blocked
        );
    }

    #[test]
    fn empty_findings_is_clean() {
        // Defensive: no findings at all is a (vacuous) clean verdict.
        assert_eq!(recognition_quality(&[]), RecognitionQuality::Clean);
    }
}
