pub mod draft;
pub mod integrity;
pub mod preflight;
pub mod schema;
pub mod validation;

pub use draft::{RecoveryDraft, RecoveryDraftDelta};
pub use integrity::{canonical_structure_json, content_checksum, content_checksum_bytes};
pub use preflight::{
    validate_canonical, Axis, CanonicalBlocker, CanonicalCause, CanonicalStoryFacts, MediaCause,
    Severity,
};
pub use schema::{
    CanonicalNode, CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION, MAX_TITLE_CHARS,
    START_NODE_ID,
};
pub use validation::{map_error, normalize_title, validate_title, StoryTitleError};
