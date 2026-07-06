//! Local-artifact import domain: the typed recognition verdict and the
//! pure analysis of a `.rustory` v1 artifact. The inverse of
//! `domain::export` — pure, framework-free, zero I/O.

pub mod artifact;
pub mod recognition;
pub mod structured_folder;

pub use artifact::{
    analyze_components, analyze_rustory_artifact, is_artifact_checksum,
    is_supported_artifact_source_name, ArtifactAnalysis, CanonicalContent, ImportableContent,
};
pub use recognition::{
    folder_import_state, import_state, recognition_quality, ImportState, RecognitionAspect,
    RecognitionCategory, RecognitionFinding, RecognitionQuality,
};
pub use structured_folder::{
    analyze_structured_folder_components, is_sober_media_basename, is_supported_folder_source_name,
    referenced_media, CreatableStory, FolderMediaKind, MediaProbe, RetainedMediaRef,
    StructuredFolderAnalysis, MAX_FOLDER_MEDIA_FILES, MAX_FOLDER_TOTAL_MEDIA_BYTES,
    STRUCTURED_FOLDER_FORMAT_VERSION, STRUCTURED_FOLDER_MANIFEST_NAME,
};
