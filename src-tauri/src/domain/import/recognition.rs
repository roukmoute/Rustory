//! Local-artifact import recognition (domain layer).
//!
//! Pure, framework-free classification of an analyzed local artifact into
//! a typed recognition verdict — the import counterpart of `preflight.rs`.
//! A "partially usable" or "functionally blocked" artifact is a RESULT
//! STATE here, never an `AppError`: only transport failures (unreadable
//! file, failed DB write) are errors, surfaced by the application layer.
//!
//! Two flows share this taxonomy, each with its OWN aspect set and state
//! derivation (each contract is documented separately in the support
//! profile): the `.rustory` v1 file import (`artifact.rs`) and the
//! structured-folder creation (`structured_folder.rs`). The [`Missing`]
//! finding category and the [`Partial`] import state are emitted by the
//! FOLDER flow only — the `.rustory` flow still never emits them, and a
//! negative test locks that, mirroring the `Axis::Filesystem`
//! declared-but-unemitted axis in `preflight.rs`. [`Resolved`] is emitted
//! by the write-path review resolution only, never at analysis time.
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

/// The aspect of the analyzed input a single finding refers to. The
/// `.rustory` flow analyzes `Envelope` / `FormatVersion` / `SchemaVersion`
/// / `Structure` / `Integrity` / `Title` / `Timestamps`; the
/// structured-folder flow analyzes `Envelope` / `FormatVersion` / `Title`
/// / `Structure` / `Media` (an author manifest has no declared schema, no
/// checksum, no timestamps); the RSS ingestion flow analyzes `Envelope` /
/// `FormatVersion` / `Source` / `Title` / `Structure` (+ `Media` when the
/// chosen item references an enclosure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionAspect {
    Envelope,
    FormatVersion,
    SchemaVersion,
    Structure,
    Integrity,
    Title,
    Timestamps,
    /// The referenced media files of a structured folder, or the remote
    /// enclosure of an ingested RSS item (never downloaded — `Missing`).
    /// Never emitted by the `.rustory` flow.
    Media,
    /// The external source an ingestion came from (an RSS feed). Emitted
    /// by the RSS ingestion flow ONLY, and always as `Ambiguous`: external
    /// content that was not reread by a human is never "clean", so this
    /// nominal finding structurally floors the durable state at
    /// `needs_review`.
    Source,
}

/// The recognition category of a single finding (UI: `reconnu` /
/// `ambiguïté` / `information manquante` / `blocage réel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionCategory {
    /// Understood and accepted.
    Recognized,
    /// Usable but adjusted / not fully trusted (e.g. a normalized title).
    Ambiguous,
    /// An expected aspect is absent. Emitted by the structured-folder flow
    /// (a referenced media absent from the folder); the `.rustory` flow
    /// never emits it.
    Missing,
    /// Makes the artifact unusable as-is.
    Blocking,
}

/// Durable per-story import state (calque of the Transfer State Contract;
/// UI chips `reconnu` / `partiel` / `à revoir`, reserved for this flow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportState {
    Recognized,
    /// "Some content is usable, some is not": emitted by the
    /// structured-folder flow when a referenced media is absent
    /// ([`folder_import_state`]); never emitted by the `.rustory` flow
    /// (which uses [`NeedsReview`] for its single-story ambiguities).
    ///
    /// [`NeedsReview`]: ImportState::NeedsReview
    Partial,
    NeedsReview,
    Blocked,
    /// Emitted by the write-path review resolution ONLY
    /// (`application::story::review`) — never at analysis time.
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
/// some not) is the folder flow's mapping ([`folder_import_state`]).
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

/// The STRUCTURED-FOLDER state derivation (the `.rustory` one above is
/// untouched): any `Blocking` → `Blocked` (nothing is created); else any
/// `Missing` (a referenced media absent — some content is usable, some is
/// not) → [`Partial`], its first real emitter; else any `Ambiguous` →
/// [`NeedsReview`]; else [`Recognized`].
///
/// [`Partial`]: ImportState::Partial
/// [`NeedsReview`]: ImportState::NeedsReview
/// [`Recognized`]: ImportState::Recognized
pub fn folder_import_state(findings: &[RecognitionFinding]) -> ImportState {
    match recognition_quality(findings) {
        RecognitionQuality::Clean => ImportState::Recognized,
        RecognitionQuality::Unusable => ImportState::Blocked,
        RecognitionQuality::Partial => {
            if findings
                .iter()
                .any(|f| f.category == RecognitionCategory::Missing)
            {
                ImportState::Partial
            } else {
                ImportState::NeedsReview
            }
        }
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

    fn missing(aspect: RecognitionAspect) -> RecognitionFinding {
        RecognitionFinding {
            aspect,
            category: RecognitionCategory::Missing,
        }
    }

    #[test]
    fn folder_state_maps_a_missing_media_to_partial() {
        // The folder flow is the FIRST real emitter of the `Partial` state:
        // a referenced media absent from the folder → some content is
        // usable, some is not.
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            missing(RecognitionAspect::Media),
        ];
        assert_eq!(recognition_quality(&findings), RecognitionQuality::Partial);
        assert_eq!(folder_import_state(&findings), ImportState::Partial);
    }

    #[test]
    fn folder_state_maps_an_ambiguity_alone_to_needs_review() {
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::ambiguous(RecognitionAspect::Structure),
        ];
        assert_eq!(folder_import_state(&findings), ImportState::NeedsReview);
    }

    #[test]
    fn folder_state_missing_dominates_an_ambiguity() {
        // Missing + Ambiguous → the durable state names the missing content
        // (`partial`), not just "review".
        let findings = [
            RecognitionFinding::ambiguous(RecognitionAspect::Title),
            missing(RecognitionAspect::Media),
        ];
        assert_eq!(folder_import_state(&findings), ImportState::Partial);
    }

    #[test]
    fn folder_state_a_blocker_dominates_everything() {
        let findings = [
            missing(RecognitionAspect::Media),
            RecognitionFinding::blocking(RecognitionAspect::Structure),
        ];
        assert_eq!(folder_import_state(&findings), ImportState::Blocked);
    }

    #[test]
    fn folder_state_clean_is_recognized() {
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::recognized(RecognitionAspect::Media),
        ];
        assert_eq!(folder_import_state(&findings), ImportState::Recognized);
    }
}
