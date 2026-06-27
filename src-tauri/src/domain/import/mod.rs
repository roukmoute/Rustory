//! Local-artifact import domain: the typed recognition verdict and the
//! pure analysis of a `.rustory` v1 artifact. The inverse of
//! `domain::export` — pure, framework-free, zero I/O.

pub mod artifact;
pub mod recognition;

pub use artifact::{
    analyze_components, analyze_rustory_artifact, is_artifact_checksum,
    is_supported_artifact_source_name, ArtifactAnalysis, CanonicalContent, ImportableContent,
};
pub use recognition::{
    import_state, recognition_quality, ImportState, RecognitionAspect, RecognitionCategory,
    RecognitionFinding, RecognitionQuality,
};
