pub mod draft;
pub mod integrity;
pub mod preflight;
pub mod schema;
pub mod validation;

pub use draft::{RecoveryDraft, RecoveryDraftDelta};
pub use integrity::{canonical_structure_json, content_checksum, content_checksum_bytes};
pub use preflight::{
    validate_canonical, Axis, CanonicalBlocker, CanonicalCause, CanonicalStoryFacts, Severity,
};
pub use schema::{
    CanonicalNode, CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION, MAX_TITLE_CHARS,
};
pub use validation::{map_error, normalize_title, validate_title, StoryTitleError};
