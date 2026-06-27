//! Canonical story validation (`preflight`), domain layer.
//!
//! Pure, framework-free re-verification of the LOCAL canonical facts of a
//! story before a transfer: does the stored `structure_json` still parse, does
//! the column `schema_version` agree with the JSON and the supported version,
//! does the stored `content_checksum` still match the bytes, and is the stored
//! title still valid? Each failure becomes a closed `(axis, cause)` blocker
//! with a frozen severity. Reuses the existing integrity / schema / validation
//! helpers — nothing is re-implemented here.
//!
//! Scope reminder: the canonical model is now schema v2 —
//! `{ "schemaVersion": 2, "nodes": [<one current node>] }` with EXACTLY ONE
//! node. A single-node story (even an empty one) is VALID, never a block;
//! zero nodes or more than one node leaves the supported single-node model and
//! is a `StructureCorrupt` block (multi-node authoring is a later phase). The
//! `Media` axis is no longer dormant: [`MediaCause`] gives it a real taxonomy
//! that the application layer emits when it resolves a node's media against the
//! asset store. `validate_canonical` itself stays pure (no filesystem access),
//! so it never emits a `Media` blocker — it validates the STRUCTURE; the media
//! detector lives where the bytes do. The `Filesystem` axis is still declared
//! without a detector (filesystem failures surface as transport errors).

use crate::domain::story::schema::{CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION};
use crate::domain::story::{content_checksum, normalize_title, validate_title};

/// The two-axis taxonomy of AC1. `Structure` / `Media` / `Filesystem` express
/// Rustory canonical validity; `DeviceProfile` expresses Lunii compatibility
/// (populated by the application layer, never by [`validate_canonical`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Structure,
    /// The node-media axis. Has a LIVE taxonomy ([`MediaCause`]) emitted by the
    /// application layer when it resolves a node's media against the asset store
    /// (the node editor is the first living emitter). `validate_canonical` stays
    /// pure and never emits it — the structure check has no filesystem access.
    Media,
    /// Declared for the taxonomy; NO detector (filesystem failures surface as
    /// transport `AppError`s, not verdict blockers). `validate_canonical` never
    /// emits it.
    Filesystem,
    DeviceProfile,
}

/// Severity is a FIXED property of the cause — `Blocking` ⇒ `bloquée`,
/// `Fixable` ⇒ `à corriger` (only when no `Blocking` cause coexists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Fixable,
    Blocking,
}

/// Closed set of canonical-validity causes (axis `Structure`). Each cause owns
/// exactly one severity and, at the IPC layer, exactly one FR message +
/// `userAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalCause {
    /// The stored title no longer passes `validate_title` (defense in depth —
    /// `read_stories` does not re-validate). Repairable by renaming.
    TitleInvalid,
    /// `schema_version` is newer than this build supports (format too recent).
    SchemaUnsupported,
    /// `structure_json` does not parse, or the column `schema_version`
    /// disagrees with the JSON `schemaVersion`.
    StructureCorrupt,
    /// The recomputed checksum of `structure_json` differs from the stored
    /// `content_checksum` — silent on-disk corruption.
    ChecksumMismatch,
}

impl CanonicalCause {
    /// Frozen severity per cause.
    pub const fn severity(self) -> Severity {
        match self {
            Self::TitleInvalid => Severity::Fixable,
            Self::SchemaUnsupported | Self::StructureCorrupt | Self::ChecksumMismatch => {
                Severity::Blocking
            }
        }
    }
}

/// Closed set of node-media causes (axis `Media`). The first LIVING emitter of
/// the `Media` axis: the application layer constructs these when it validates a
/// file at attach time and when it resolves a node's stored media references.
/// Severity follows the AC2 split — `attention requise` (`Fixable`) vs `blocage
/// réel` (`Blocking`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCause {
    /// The chosen file is not a supported media format (sniffed by magic bytes,
    /// never by extension). A real block — the file is never stored.
    UnsupportedFormat,
    /// The chosen file is unreadable or exceeds the byte ceiling. A real block.
    Unreadable,
    /// A node references a stored asset whose source bytes can no longer be
    /// resolved. Repairable (`attention`): the rest of the node stays editable;
    /// the user re-associates or removes the media.
    SourceMissing,
}

impl MediaCause {
    /// Frozen severity per media cause.
    pub const fn severity(self) -> Severity {
        match self {
            Self::UnsupportedFormat | Self::Unreadable => Severity::Blocking,
            Self::SourceMissing => Severity::Fixable,
        }
    }
}

/// A single canonical-validity blocker. `axis` is always `Structure` for the
/// MVP causes; the field is kept so the application layer composes a uniform
/// blocker list across the two axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalBlocker {
    pub axis: Axis,
    pub cause: CanonicalCause,
    pub severity: Severity,
}

impl CanonicalBlocker {
    fn structure(cause: CanonicalCause) -> Self {
        Self {
            axis: Axis::Structure,
            cause,
            severity: cause.severity(),
        }
    }
}

/// The LOCAL facts read from the `stories` row — the exact stored values, never
/// recomputed outside the DB read.
#[derive(Debug, Clone)]
pub struct CanonicalStoryFacts {
    pub title: String,
    pub schema_version: u32,
    pub structure_json: String,
    pub content_checksum: String,
}

/// Re-verify a story's canonical validity. Returns the (possibly empty) list of
/// structural blockers. An empty list ⇒ canonically valid (an empty-`nodes`
/// story included).
pub fn validate_canonical(facts: &CanonicalStoryFacts) -> Vec<CanonicalBlocker> {
    let mut blockers = Vec::new();

    // 1. Parse `structure_json`. An unreadable structure is a hard block; the
    //    schema/checksum checks below would be meaningless on garbage, so we
    //    record `StructureCorrupt` and stop — but still re-guard the title,
    //    which is independent of the structure, so a fixable title issue is not
    //    masked by a corrupt structure.
    let structure: CanonicalStructure = match serde_json::from_str(&facts.structure_json) {
        Ok(s) => s,
        Err(_) => {
            blockers.push(CanonicalBlocker::structure(
                CanonicalCause::StructureCorrupt,
            ));
            push_title_blocker(&mut blockers, &facts.title);
            return blockers;
        }
    };

    // 2. Schema coherence. A column↔JSON disagreement is ALWAYS a corruption
    //    signal, whatever the two values are, so it is tested FIRST: only once
    //    the column and the JSON AGREE on a version does "format too recent"
    //    (a version newer than this build supports) become meaningful. A
    //    disagreement where the JSON is newer than the current version is still
    //    corruption (`StructureCorrupt`), not "format too recent"
    //    (`SchemaUnsupported`) — the latter must mean a genuinely newer, self
    //    consistent artifact the user should update Rustory for, not a
    //    tampered/diverging row. An agreed version below the current one is also
    //    corruption.
    let mut structure_flagged_corrupt = false;
    if facts.schema_version != structure.schema_version {
        blockers.push(CanonicalBlocker::structure(
            CanonicalCause::StructureCorrupt,
        ));
        structure_flagged_corrupt = true;
    } else if structure.schema_version > CANONICAL_STORY_SCHEMA_VERSION {
        blockers.push(CanonicalBlocker::structure(
            CanonicalCause::SchemaUnsupported,
        ));
    } else if structure.schema_version != CANONICAL_STORY_SCHEMA_VERSION {
        blockers.push(CanonicalBlocker::structure(
            CanonicalCause::StructureCorrupt,
        ));
        structure_flagged_corrupt = true;
    }

    // 2b. Shape coherence with the current model. In the v2 canonical form a
    //     story carries EXACTLY ONE current node. Zero nodes (nothing to edit)
    //     or more than one node (multi-node authoring is a later phase) leaves
    //     the supported single-node model — a structural incoherence, not
    //     "format too recent" (the version IS the current one) — so it is a
    //     `StructureCorrupt` block. The single node must also be structurally
    //     sound (a non-empty stable id, needed to target writes and keep the
    //     current node identified). Only checked for the current version (a
    //     future schema may legitimately carry several nodes) and only when the
    //     structure was not already flagged corrupt (avoid a duplicate blocker).
    if structure.schema_version == CANONICAL_STORY_SCHEMA_VERSION && !structure_flagged_corrupt {
        let single_sound_node =
            structure.nodes.len() == 1 && !structure.nodes[0].id.trim().is_empty();
        if !single_sound_node {
            blockers.push(CanonicalBlocker::structure(
                CanonicalCause::StructureCorrupt,
            ));
        }
    }

    // 3. Integrity: recompute the checksum over `structure_json` ALONE (the
    //    title never enters the digest — see `create_story` / `update_story`)
    //    and compare to the stored value. A mismatch is silent corruption.
    if content_checksum(&facts.structure_json) != facts.content_checksum {
        blockers.push(CanonicalBlocker::structure(
            CanonicalCause::ChecksumMismatch,
        ));
    }

    // 4. Title re-guard (defense in depth). Fixable: the user can rename.
    push_title_blocker(&mut blockers, &facts.title);

    // NOTE: a single EMPTY node (no text, no media) is VALID in v2 — we
    // deliberately emit NO "story without content" block (that would make
    // `présumée transférable` unreachable). The check above only rejects a
    // node COUNT other than one, which leaves the single-node model rather
    // than being a legitimately empty starting node.

    blockers
}

fn push_title_blocker(blockers: &mut Vec<CanonicalBlocker>, title: &str) {
    if validate_title(&normalize_title(title)).is_err() {
        blockers.push(CanonicalBlocker::structure(CanonicalCause::TitleInvalid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEALTHY_JSON: &str = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";

    fn healthy_facts() -> CanonicalStoryFacts {
        CanonicalStoryFacts {
            title: "Mon histoire".into(),
            schema_version: 2,
            structure_json: HEALTHY_JSON.into(),
            content_checksum: content_checksum(HEALTHY_JSON),
        }
    }

    #[test]
    fn healthy_story_has_no_blockers() {
        assert!(validate_canonical(&healthy_facts()).is_empty());
    }

    #[test]
    fn single_empty_node_is_valid_never_a_block() {
        // The v2 canonical form is a single empty starting node; it must
        // produce zero blockers (otherwise `présumée transférable` would be
        // unreachable for a brand-new or migrated story).
        let facts = healthy_facts();
        assert_eq!(facts.structure_json, HEALTHY_JSON);
        assert!(validate_canonical(&facts).is_empty());
    }

    #[test]
    fn filled_single_node_with_media_is_valid() {
        // A node carrying text, a label and media references is canonically
        // valid — the structure check does not resolve the media (no FS here).
        let json = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"Bonjour\",\"label\":\"Début\",\"imageAssetId\":\"img\",\"audioAssetId\":\"aud\"}]}";
        let facts = CanonicalStoryFacts {
            title: "Pleine".into(),
            schema_version: 2,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        assert!(validate_canonical(&facts).is_empty());
    }

    #[test]
    fn zero_nodes_in_v2_is_structure_corrupt() {
        // v2 needs exactly one current node; an empty list has nothing to edit
        // and leaves the supported model.
        let json = "{\"schemaVersion\":2,\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Sans nœud".into(),
            schema_version: 2,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        let blockers = validate_canonical(&facts);
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::StructureCorrupt);
        assert_eq!(blockers[0].severity, Severity::Blocking);
    }

    #[test]
    fn multiple_nodes_in_v2_is_structure_corrupt() {
        // Two nodes is multi-node authoring — out of scope for the single-node
        // model, hence a structural incoherence even with a coherent checksum.
        let json = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null},{\"id\":\"n2\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";
        let facts = CanonicalStoryFacts {
            title: "Multi".into(),
            schema_version: 2,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        let blockers = validate_canonical(&facts);
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::StructureCorrupt);
    }

    #[test]
    fn blank_node_id_in_v2_is_structure_corrupt() {
        // The current node needs a stable, non-blank id to target writes and
        // stay identified.
        let json = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"   \",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";
        let facts = CanonicalStoryFacts {
            title: "Id vide".into(),
            schema_version: 2,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        let blockers = validate_canonical(&facts);
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::StructureCorrupt);
    }

    #[test]
    fn wrong_node_count_does_not_double_report_structure_corrupt() {
        // A column↔JSON schema disagreement already flags StructureCorrupt; an
        // unexpected node shape on top must not push a SECOND identical blocker.
        let json = "{\"schemaVersion\":0,\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Incohérente".into(),
            schema_version: 2,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        let corrupt = validate_canonical(&facts)
            .into_iter()
            .filter(|b| b.cause == CanonicalCause::StructureCorrupt)
            .count();
        assert_eq!(corrupt, 1, "StructureCorrupt must be reported exactly once");
    }

    #[test]
    fn media_cause_severities_split_attention_from_blocking() {
        assert_eq!(MediaCause::UnsupportedFormat.severity(), Severity::Blocking);
        assert_eq!(MediaCause::Unreadable.severity(), Severity::Blocking);
        assert_eq!(MediaCause::SourceMissing.severity(), Severity::Fixable);
    }

    #[test]
    fn checksum_mismatch_is_a_blocking_blocker() {
        let mut facts = healthy_facts();
        facts.content_checksum = "0".repeat(64);
        let blockers = validate_canonical(&facts);
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::ChecksumMismatch);
        assert_eq!(blockers[0].axis, Axis::Structure);
        assert_eq!(blockers[0].severity, Severity::Blocking);
    }

    #[test]
    fn unreadable_structure_json_is_structure_corrupt() {
        let mut facts = healthy_facts();
        facts.structure_json = "{ this is not json".into();
        // The stored checksum no longer matches either, but `StructureCorrupt`
        // dominates and is reported alone (the title stays valid).
        let blockers = validate_canonical(&facts);
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::StructureCorrupt);
        assert_eq!(blockers[0].severity, Severity::Blocking);
    }

    #[test]
    fn schema_version_above_supported_is_schema_unsupported() {
        // A self-consistent v3 (column == JSON == 3, both above the current 2)
        // is "format too recent" — the only path that means "update Rustory".
        let json = "{\"schemaVersion\":3,\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Format trop récent".into(),
            schema_version: 3,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        let blockers = validate_canonical(&facts);
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::SchemaUnsupported);
        assert_eq!(blockers[0].severity, Severity::Blocking);
    }

    #[test]
    fn column_json_schema_disagreement_is_structure_corrupt() {
        // The column says v2, the JSON says v0 — incoherent → corruption, not
        // "format too recent".
        let json = "{\"schemaVersion\":0,\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Incohérente".into(),
            schema_version: 2,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        let blockers = validate_canonical(&facts);
        assert!(blockers
            .iter()
            .any(|b| b.cause == CanonicalCause::StructureCorrupt));
        assert!(blockers
            .iter()
            .all(|b| b.cause != CanonicalCause::SchemaUnsupported));
    }

    #[test]
    fn column_json_disagreement_with_newer_json_is_structure_corrupt_not_unsupported() {
        // The column says v2 but the JSON says v3: a DISAGREEMENT is always
        // corruption, never "format too recent". `SchemaUnsupported` must be
        // reserved for a self-consistent newer artifact (column == JSON > current),
        // whose `userAction` is "update Rustory" — not a diverging/tampered row.
        let json = "{\"schemaVersion\":3,\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Désaccord vers le récent".into(),
            schema_version: 2,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        let blockers = validate_canonical(&facts);
        assert!(blockers
            .iter()
            .any(|b| b.cause == CanonicalCause::StructureCorrupt));
        assert!(
            blockers
                .iter()
                .all(|b| b.cause != CanonicalCause::SchemaUnsupported),
            "a column↔JSON disagreement must not be labelled SchemaUnsupported"
        );
    }

    #[test]
    fn empty_title_is_a_fixable_title_blocker() {
        let mut facts = healthy_facts();
        facts.title = "   ".into();
        let blockers = validate_canonical(&facts);
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::TitleInvalid);
        assert_eq!(blockers[0].severity, Severity::Fixable);
    }

    #[test]
    fn control_char_title_is_a_fixable_title_blocker() {
        let mut facts = healthy_facts();
        facts.title = "Ligne1\nLigne2".into();
        let blockers = validate_canonical(&facts);
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::TitleInvalid);
        assert_eq!(blockers[0].severity, Severity::Fixable);
    }

    #[test]
    fn a_corrupt_structure_still_surfaces_a_fixable_title() {
        // StructureCorrupt does not mask an independent (fixable) title issue.
        let facts = CanonicalStoryFacts {
            title: String::new(),
            schema_version: 2,
            structure_json: "not json".into(),
            content_checksum: "0".repeat(64),
        };
        let blockers = validate_canonical(&facts);
        assert!(blockers
            .iter()
            .any(|b| b.cause == CanonicalCause::StructureCorrupt));
        assert!(blockers
            .iter()
            .any(|b| b.cause == CanonicalCause::TitleInvalid));
    }
}
