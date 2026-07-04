use serde::{Deserialize, Serialize};

/// Current version of the canonical story model serialized to disk.
///
/// Incrementing this constant MUST be paired with a matching migration
/// that describes how prior `schema_version` records are either migrated
/// or explicitly refused, per the architecture's migration rules. Version 3
/// turns the story into an ordered node GRAPH: one or more nodes, an explicit
/// start node (`startNodeId`), and per-node option links toward other nodes.
/// Version 2 carried exactly one current node; version 1 had an always-empty
/// node list.
pub const CANONICAL_STORY_SCHEMA_VERSION: u32 = 3;

/// Upper bound on the number of Unicode scalar values allowed in a story
/// title. Aligned with the frontend ergonomic validation so the two sides
/// reject the same inputs.
pub const MAX_TITLE_CHARS: usize = 120;

/// Stable id of the starting node produced by [`CanonicalStructure::minimal`]
/// and used when a brand-new story is created.
///
/// Node ids only need to be unique WITHIN a story. New node ids are generated
/// by Rust as the smallest free `n<k>` (n2, n3, …), so a fresh story keeps
/// deterministic canonical bytes (hence a stable `content_checksum`). The
/// start node of an EXISTING story is designated by `startNodeId`, never by
/// this constant (a migrated or imported story may start on another id).
pub const START_NODE_ID: &str = "n1";

/// Root canonical structure persisted in `stories.structure_json`. The v3
/// shape is a flat ordered node graph: `nodes` holds ONE OR MORE nodes (their
/// order is the display/navigation order), and `start_node_id` designates the
/// entry point (it must reference an existing node). `deny_unknown_fields`
/// makes a stray root key fail the parse rather than being silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalStructure {
    pub schema_version: u32,
    pub start_node_id: String,
    pub nodes: Vec<CanonicalNode>,
}

/// A node of the story graph: a stable `id`, the narrative `text`, a
/// human-readable `label` (metadata), optional references to a stored
/// image / audio asset BY ASSET ID, and the node's `options` (the choices it
/// offers, each possibly linking to another node). The media BYTES never live
/// in the canonical JSON — only the reference does; the bytes are owned by
/// Rust infrastructure (see the node-media store) and `content_checksum`
/// covers these reference strings exactly like any other canonical byte.
///
/// `deny_unknown_fields` keeps the node shape under Rust authority: a drifted
/// payload (a stray key, an older node shape) fails to parse rather than
/// silently degrading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalNode {
    pub id: String,
    pub text: String,
    pub label: String,
    pub image_asset_id: Option<String>,
    pub audio_asset_id: Option<String>,
    pub options: Vec<CanonicalOption>,
}

/// A choice a node offers. `target` is the destination node id: `None` means
/// the option is not linked yet (a normal authoring state); `Some(id)` with an
/// id present in `nodes[]` is a live link; `Some(id)` with an ABSENT id is a
/// broken link — persistable, surfaced as repairable (`à corriger`), never
/// silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalOption {
    pub label: String,
    pub target: Option<String>,
}

impl CanonicalNode {
    /// The empty starting node: a stable id, no text, no label, no media, no
    /// options. A freshly created or migrated story opens on this node — an
    /// empty node is a valid starting state, never an error.
    pub fn start() -> Self {
        Self {
            id: START_NODE_ID.to_string(),
            text: String::new(),
            label: String::new(),
            image_asset_id: None,
            audio_asset_id: None,
            options: Vec::new(),
        }
    }
}

impl CanonicalStructure {
    /// The minimal canonical structure for a brand-new story: schema v3 with
    /// exactly one empty starting node designated as the start. Deterministic —
    /// every call produces byte-identical JSON.
    pub fn minimal() -> Self {
        Self {
            schema_version: CANONICAL_STORY_SCHEMA_VERSION,
            start_node_id: START_NODE_ID.to_string(),
            nodes: vec![CanonicalNode::start()],
        }
    }
}

/// READ-ONLY shape of a legacy v2 canonical structure — the exact v2 layout
/// (no `startNodeId`, no node `options`), with `deny_unknown_fields` so a
/// drifted payload fails to parse instead of being silently coerced. The live
/// [`CanonicalStructure`] CANNOT read v2 bytes (missing required fields);
/// every v2 consumer (the schema migration, the artifact import upgrade)
/// parses through this dedicated type, then promotes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyStructureV2 {
    pub schema_version: u32,
    pub nodes: Vec<LegacyNodeV2>,
}

/// READ-ONLY shape of a legacy v2 node (no `options` field).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyNodeV2 {
    pub id: String,
    pub text: String,
    pub label: String,
    pub image_asset_id: Option<String>,
    pub audio_asset_id: Option<String>,
}

impl LegacyStructureV2 {
    /// Promote a parsed v2 structure to the current v3 shape, LOSSLESSLY: the
    /// node content (id, text, label, media references) is carried
    /// byte-for-byte, the single node's id becomes `startNodeId` (NOT
    /// necessarily "n1" — an imported artifact may use another id), and the
    /// node gains empty `options`. Returns `None` — the caller fails closed —
    /// when the payload is not a HEALTHY v2: a `schemaVersion` other than 2
    /// (a column↔JSON or envelope disagreement is corruption, never silently
    /// re-labelled) or a node count other than one (the v2 model carried
    /// EXACTLY one node; promoting a forged multi-node or empty v2 would
    /// silently repair — or invent a start for — a corrupt payload).
    pub fn promote_to_v3(&self) -> Option<CanonicalStructure> {
        if self.schema_version != 2 || self.nodes.len() != 1 {
            return None;
        }
        let start_node_id = self.nodes[0].id.clone();
        Some(CanonicalStructure {
            schema_version: CANONICAL_STORY_SCHEMA_VERSION,
            start_node_id,
            nodes: self
                .nodes
                .iter()
                .map(|n| CanonicalNode {
                    id: n.id.clone(),
                    text: n.text.clone(),
                    label: n.label.clone(),
                    image_asset_id: n.image_asset_id.clone(),
                    audio_asset_id: n.audio_asset_id.clone(),
                    options: Vec::new(),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::story::canonical_structure_json;

    #[test]
    fn minimal_is_schema_v3_with_a_single_empty_start_node() {
        let s = CanonicalStructure::minimal();
        assert_eq!(s.schema_version, 3);
        assert_eq!(s.start_node_id, START_NODE_ID);
        assert_eq!(s.nodes.len(), 1);
        let node = &s.nodes[0];
        assert_eq!(node.id, START_NODE_ID);
        assert_eq!(node.text, "");
        assert_eq!(node.label, "");
        assert!(node.image_asset_id.is_none());
        assert!(node.audio_asset_id.is_none());
        assert!(node.options.is_empty());
    }

    #[test]
    fn minimal_serializes_to_the_stable_v3_shape() {
        // This EXACT byte string is the canonical minimal form: field order is
        // frozen by the struct declaration order (root: schemaVersion,
        // startNodeId, nodes; node: id, text, label, imageAssetId,
        // audioAssetId, options; option: label, target). A drift here (field
        // rename, order, omitted null) would silently change every fresh
        // story's `content_checksum`.
        assert_eq!(
            canonical_structure_json(&CanonicalStructure::minimal()),
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}",
        );
    }

    #[test]
    fn option_serializes_label_then_target() {
        // The option field order (label, target) is part of the frozen byte
        // shape — checked on both the linked and unlinked variants.
        let node = CanonicalNode {
            id: "n1".into(),
            text: String::new(),
            label: String::new(),
            image_asset_id: None,
            audio_asset_id: None,
            options: vec![
                CanonicalOption {
                    label: "Continuer".into(),
                    target: Some("n2".into()),
                },
                CanonicalOption {
                    label: "Attendre".into(),
                    target: None,
                },
            ],
        };
        let json = serde_json::to_string(&node).expect("serialize node");
        assert!(json.contains(
            "\"options\":[{\"label\":\"Continuer\",\"target\":\"n2\"},{\"label\":\"Attendre\",\"target\":null}]"
        ));
    }

    #[test]
    fn multi_node_graph_round_trips() {
        let structure = CanonicalStructure {
            schema_version: CANONICAL_STORY_SCHEMA_VERSION,
            start_node_id: "n1".into(),
            nodes: vec![
                CanonicalNode {
                    id: "n1".into(),
                    text: "Il était une fois".into(),
                    label: "Début".into(),
                    image_asset_id: Some("asset-img".into()),
                    audio_asset_id: Some("asset-aud".into()),
                    options: vec![CanonicalOption {
                        label: "Entrer".into(),
                        target: Some("n2".into()),
                    }],
                },
                CanonicalNode {
                    id: "n2".into(),
                    text: "La suite".into(),
                    label: String::new(),
                    image_asset_id: None,
                    audio_asset_id: None,
                    options: vec![],
                },
            ],
        };
        let json = canonical_structure_json(&structure);
        let back: CanonicalStructure = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(back, structure);
    }

    #[test]
    fn rejects_unknown_root_key() {
        let err = serde_json::from_str::<CanonicalStructure>(
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[],\"extra\":1}",
        )
        .expect_err("unknown root key must fail");
        assert!(err.to_string().contains("extra"));
    }

    #[test]
    fn rejects_unknown_node_key() {
        let err = serde_json::from_str::<CanonicalStructure>(
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[],\"x\":1}]}",
        )
        .expect_err("unknown node key must fail");
        assert!(err.to_string().contains('x') || err.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_unknown_option_key() {
        let err = serde_json::from_str::<CanonicalStructure>(
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[{\"label\":\"Go\",\"target\":null,\"y\":1}]}]}",
        )
        .expect_err("unknown option key must fail");
        assert!(err.to_string().contains('y') || err.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_legacy_v2_node_shape() {
        // The v2 node had no `options` field; the typed v3 node requires it,
        // so the old shape no longer parses (the migration re-stamps it).
        serde_json::from_str::<CanonicalStructure>(
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}",
        )
        .expect_err("v2 node shape (no options) must fail to parse");
    }

    #[test]
    fn rejects_legacy_v2_root_shape() {
        // The v2 root had no `startNodeId`; the v3 root requires it.
        serde_json::from_str::<CanonicalStructure>(
            "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}",
        )
        .expect_err("v2 root shape (no startNodeId) must fail to parse");
    }

    #[test]
    fn legacy_v2_parses_and_promotes_losslessly_to_v3() {
        let legacy: LegacyStructureV2 = serde_json::from_str(
            "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"nX\",\"text\":\"Bonjour\",\"label\":\"Début\",\"imageAssetId\":\"img\",\"audioAssetId\":null}]}",
        )
        .expect("v2 bytes parse through the legacy type");
        let v3 = legacy.promote_to_v3().expect("healthy v2 promotes");
        assert_eq!(v3.schema_version, 3);
        // The start node keeps the ORIGINAL id — never forced back to "n1".
        assert_eq!(v3.start_node_id, "nX");
        assert_eq!(v3.nodes.len(), 1);
        assert_eq!(v3.nodes[0].id, "nX");
        assert_eq!(v3.nodes[0].text, "Bonjour");
        assert_eq!(v3.nodes[0].label, "Début");
        assert_eq!(v3.nodes[0].image_asset_id.as_deref(), Some("img"));
        assert!(v3.nodes[0].audio_asset_id.is_none());
        assert!(v3.nodes[0].options.is_empty());
    }

    #[test]
    fn legacy_v2_with_unknown_field_fails_to_parse() {
        serde_json::from_str::<LegacyStructureV2>(
            "{\"schemaVersion\":2,\"nodes\":[],\"startNodeId\":\"n1\"}",
        )
        .expect_err("a v3 field on a v2 payload must fail the legacy parse");
    }

    #[test]
    fn legacy_v2_promotion_fails_closed_on_any_unhealthy_shape() {
        let wrong_version = LegacyStructureV2 {
            schema_version: 1,
            nodes: vec![],
        };
        assert!(wrong_version.promote_to_v3().is_none());

        let empty: LegacyStructureV2 =
            serde_json::from_str("{\"schemaVersion\":2,\"nodes\":[]}").expect("parse");
        assert!(
            empty.promote_to_v3().is_none(),
            "no node ⇒ no definable start node ⇒ never invented"
        );

        // A forged multi-node v2 never existed as a healthy payload (the v2
        // model carried EXACTLY one node) — promoting it would silently
        // repair a corrupt structure.
        let multi: LegacyStructureV2 = serde_json::from_str(
            "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null},{\"id\":\"n2\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}",
        )
        .expect("parse");
        assert!(multi.promote_to_v3().is_none());
    }
}
