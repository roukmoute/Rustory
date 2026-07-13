//! Local-artifact import domain: the typed recognition verdict and the
//! pure analysis of a `.rustory` v1 artifact. The inverse of
//! `domain::export` — pure, framework-free, zero I/O.

pub mod artifact;
pub mod content_source;
pub mod recognition;
pub mod rss;
pub mod structured_folder;

pub use artifact::{
    analyze_components, analyze_rustory_artifact, is_artifact_checksum,
    is_supported_artifact_source_name, ArtifactAnalysis, CanonicalContent, ImportableContent,
};
pub use content_source::{
    content_source_activation, official_content_sources, ContentSourceActivation,
    ContentSourceKind, ContentSourceLine, ALL_CONTENT_SOURCE_ACTIVATIONS, ALL_CONTENT_SOURCE_KINDS,
};
pub use recognition::{
    folder_import_state, import_state, recognition_quality, ImportState, RecognitionAspect,
    RecognitionCategory, RecognitionFinding, RecognitionQuality,
};
pub use rss::{
    clean_rss_text, feed_url_host, is_supported_feed_url, parse_rss, resolve_rss_item,
    rss_import_state, rss_item_findings, rss_item_fingerprint, rss_item_ref, RssAnalysis, RssItem,
    RssItemRef, MAX_RSS_ITEMS, MAX_RSS_ITEM_TEXT_CHARS, MAX_RSS_URL_CHARS, MAX_RSS_XML_DEPTH,
    RSS_FALLBACK_TITLE_PREFIX, RSS_SOURCE_FORMAT_VERSION,
};
pub use structured_folder::{
    analyze_structured_folder_components, is_sober_media_basename, is_supported_folder_source_name,
    referenced_media, CreatableStory, FolderMediaKind, MediaProbe, RetainedMediaRef,
    StructuredFolderAnalysis, MAX_FOLDER_MEDIA_FILES, MAX_FOLDER_TOTAL_MEDIA_BYTES,
    STRUCTURED_FOLDER_FORMAT_VERSION, STRUCTURED_FOLDER_MANIFEST_NAME,
};
