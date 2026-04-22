use serde::{Deserialize, Serialize};

/// Current version of the canonical story model serialized to disk.
///
/// Incrementing this constant MUST be paired with a matching SQL migration
/// that describes how prior `schema_version` records are either migrated
/// or explicitly refused, per the architecture's migration rules.
pub const CANONICAL_STORY_SCHEMA_VERSION: u32 = 1;

/// Upper bound on the number of Unicode scalar values allowed in a story
/// title. Aligned with the frontend ergonomic validation so the two sides
/// reject the same inputs.
pub const MAX_TITLE_CHARS: usize = 120;

/// Root canonical structure persisted in `stories.structure_json`. The
/// minimum shape for the initial release contains no nodes — later
/// releases extend this with seasons/episodes/nodes as they become part
/// of the supported editor surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalStructure {
    pub schema_version: u32,
    pub nodes: Vec<CanonicalNode>,
}

/// Placeholder node type. Kept as a unit struct with no fields so the
/// canonical JSON shape `{"schemaVersion":1,"nodes":[]}` is guaranteed by
/// the type system to remain empty at this stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalNode {}

impl CanonicalStructure {
    pub fn minimal() -> Self {
        Self {
            schema_version: CANONICAL_STORY_SCHEMA_VERSION,
            nodes: Vec::new(),
        }
    }
}
