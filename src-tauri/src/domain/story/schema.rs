use serde::{Deserialize, Serialize};

/// Current version of the canonical story model serialized to disk.
///
/// Incrementing this constant MUST be paired with a matching SQL migration
/// that describes how prior `schema_version` records are either migrated
/// or explicitly refused, per the architecture's migration rules. Version 2
/// gives a story a single editable **current node** (text, metadata and
/// optional media references); version 1 had an always-empty node list.
pub const CANONICAL_STORY_SCHEMA_VERSION: u32 = 2;

/// Upper bound on the number of Unicode scalar values allowed in a story
/// title. Aligned with the frontend ergonomic validation so the two sides
/// reject the same inputs.
pub const MAX_TITLE_CHARS: usize = 120;

/// Stable id of the single starting node produced by [`CanonicalStructure::minimal`]
/// and backfilled by the v1→v2 migration.
///
/// Node ids only need to be unique WITHIN a story; at this stage a story
/// carries EXACTLY ONE node, so a constant keeps the canonical bytes
/// deterministic (hence a stable `content_checksum`) and keeps the current
/// node clearly identified across a long edit session — no identity drift.
pub const START_NODE_ID: &str = "n1";

/// Root canonical structure persisted in `stories.structure_json`. The v2
/// shape carries exactly one current node (`nodes` holds a single entry).
/// `deny_unknown_fields` makes a stray root key fail the parse rather than
/// being silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalStructure {
    pub schema_version: u32,
    pub nodes: Vec<CanonicalNode>,
}

/// The current node of a story: a stable `id`, the narrative `text`, a
/// human-readable `label` (metadata), and optional references to a stored
/// image / audio asset BY ASSET ID. The media BYTES never live in the
/// canonical JSON — only the reference does; the bytes are owned by Rust
/// infrastructure (see the node-media store) and `content_checksum` covers
/// these reference strings exactly like any other canonical byte.
///
/// `deny_unknown_fields` keeps the node shape under Rust authority: a drifted
/// payload (a stray key, the old empty `{}` v1 node) fails to parse rather
/// than silently degrading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalNode {
    pub id: String,
    pub text: String,
    pub label: String,
    pub image_asset_id: Option<String>,
    pub audio_asset_id: Option<String>,
}

impl CanonicalNode {
    /// The empty starting node: a stable id, no text, no label, no media.
    /// A freshly created or migrated story opens on this node — an empty
    /// node is a valid starting state, never an error.
    pub fn start() -> Self {
        Self {
            id: START_NODE_ID.to_string(),
            text: String::new(),
            label: String::new(),
            image_asset_id: None,
            audio_asset_id: None,
        }
    }
}

impl CanonicalStructure {
    /// The minimal canonical structure for a brand-new story: schema v2 with
    /// exactly one empty starting node. Deterministic — every call produces
    /// byte-identical JSON, so the v1→v2 migration can re-stamp every legacy
    /// row to a single, precomputed checksum.
    pub fn minimal() -> Self {
        Self {
            schema_version: CANONICAL_STORY_SCHEMA_VERSION,
            nodes: vec![CanonicalNode::start()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::story::canonical_structure_json;

    #[test]
    fn minimal_is_schema_v2_with_a_single_empty_start_node() {
        let s = CanonicalStructure::minimal();
        assert_eq!(s.schema_version, 2);
        assert_eq!(s.nodes.len(), 1);
        let node = &s.nodes[0];
        assert_eq!(node.id, START_NODE_ID);
        assert_eq!(node.text, "");
        assert_eq!(node.label, "");
        assert!(node.image_asset_id.is_none());
        assert!(node.audio_asset_id.is_none());
    }

    #[test]
    fn minimal_serializes_to_the_stable_v2_shape() {
        // This EXACT byte string is the one the migration re-stamps onto every
        // legacy row; its SHA-256 is embedded in `0007_*.sql`. A drift here
        // (field rename, order, omitted null) would silently invalidate the
        // migration's precomputed checksum.
        assert_eq!(
            canonical_structure_json(&CanonicalStructure::minimal()),
            "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}",
        );
    }

    #[test]
    fn node_round_trips_with_media_references() {
        let node = CanonicalNode {
            id: "n1".into(),
            text: "Il était une fois".into(),
            label: "Début".into(),
            image_asset_id: Some("asset-img".into()),
            audio_asset_id: Some("asset-aud".into()),
        };
        let structure = CanonicalStructure {
            schema_version: CANONICAL_STORY_SCHEMA_VERSION,
            nodes: vec![node.clone()],
        };
        let json = canonical_structure_json(&structure);
        let back: CanonicalStructure = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(back.nodes[0], node);
    }

    #[test]
    fn rejects_unknown_root_key() {
        let err = serde_json::from_str::<CanonicalStructure>(
            "{\"schemaVersion\":2,\"nodes\":[],\"extra\":1}",
        )
        .expect_err("unknown root key must fail");
        assert!(err.to_string().contains("extra"));
    }

    #[test]
    fn rejects_unknown_node_key() {
        let err = serde_json::from_str::<CanonicalStructure>(
            "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"x\":1}]}",
        )
        .expect_err("unknown node key must fail");
        assert!(err.to_string().contains('x') || err.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_legacy_empty_node_shape() {
        // The v1 placeholder node was `{}`; the typed v2 node requires
        // id/text/label, so the old shape no longer parses.
        serde_json::from_str::<CanonicalStructure>("{\"schemaVersion\":2,\"nodes\":[{}]}")
            .expect_err("empty node object must fail to parse");
    }
}
