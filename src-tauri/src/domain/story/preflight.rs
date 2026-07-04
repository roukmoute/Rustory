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
//! Scope reminder: the canonical model is schema v3 — an ordered node GRAPH
//! `{ "schemaVersion": 3, "startNodeId": …, "nodes": [<one or more>] }` with
//! per-node option links. The structural check is a GRAPH INVARIANT: at least
//! one node with a sound (non-blank) id (`StructureCorrupt` otherwise), unique
//! node ids (`DuplicateNodeId`), an existing start node (`StartNodeInvalid`) —
//! all Blocking — and resolvable option targets (`BrokenOptionLink`, Fixable:
//! a link whose destination vanished is repairable in the editor, like a media
//! source gone missing; an UNLINKED option — `target: null` — is a normal
//! authoring state and emits nothing). A single empty start node is VALID,
//! never a block. The `Media` axis is not dormant: [`MediaCause`] gives it a
//! real taxonomy that the application layer emits when it resolves a node's
//! media against the asset store. `validate_canonical` itself stays pure (no
//! filesystem access), so it never emits a `Media` blocker — it validates the
//! STRUCTURE; the media detector lives where the bytes do. The `Filesystem`
//! axis is still declared without a detector (filesystem failures surface as
//! transport errors).

use std::collections::HashSet;

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
    /// `structure_json` does not parse, the column `schema_version` disagrees
    /// with the JSON `schemaVersion`, or the graph is unusable (zero nodes, a
    /// blank node id).
    StructureCorrupt,
    /// The recomputed checksum of `structure_json` differs from the stored
    /// `content_checksum` — silent on-disk corruption.
    ChecksumMismatch,
    /// Two nodes of the graph carry the same id — links become ambiguous.
    DuplicateNodeId,
    /// `startNodeId` is blank or references no node — the story has no entry
    /// point.
    StartNodeInvalid,
    /// An option's `target` references a node id absent from the graph.
    /// Repairable (`à corriger`): the link stays visible in the editor so the
    /// user can re-link or remove the option; an unlinked option
    /// (`target: null`) is NOT this cause (a normal authoring state).
    BrokenOptionLink,
}

impl CanonicalCause {
    /// Frozen severity per cause.
    pub const fn severity(self) -> Severity {
        match self {
            Self::TitleInvalid | Self::BrokenOptionLink => Severity::Fixable,
            Self::SchemaUnsupported
            | Self::StructureCorrupt
            | Self::ChecksumMismatch
            | Self::DuplicateNodeId
            | Self::StartNodeInvalid => Severity::Blocking,
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
/// structural blockers. An empty list ⇒ canonically valid (a minimal
/// single-start-node story included).
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

    // 2b. Graph invariant, for the current version only (a future schema may
    //     relax it) and only when the structure was not already flagged
    //     corrupt (avoid piling blockers onto an incoherent version). Each
    //     cause is reported AT MOST ONCE — the blocker list is a story-level
    //     verdict; per-node localization is the projection's concern.
    //
    //     - `StructureCorrupt` (Blocking): zero nodes, or a blank node id —
    //       the graph is unusable / writes cannot be targeted.
    //     - `DuplicateNodeId` (Blocking): two nodes share an id — links become
    //       ambiguous.
    //     - `StartNodeInvalid` (Blocking): `startNodeId` blank or absent from
    //       `nodes[]` — no entry point.
    //     - `BrokenOptionLink` (Fixable): an option targets an id absent from
    //       the graph — repairable in the editor (the exact analogue of a
    //       media source gone missing). `target: null` (unlinked) emits
    //       NOTHING: not linked yet ≠ points at a ghost. Self-reference is a
    //       legitimate narrative loop and emits nothing either.
    if structure.schema_version == CANONICAL_STORY_SCHEMA_VERSION && !structure_flagged_corrupt {
        let has_blank_id = structure.nodes.iter().any(|n| n.id.trim().is_empty());
        if structure.nodes.is_empty() || has_blank_id {
            blockers.push(CanonicalBlocker::structure(
                CanonicalCause::StructureCorrupt,
            ));
        }

        let mut seen_ids: HashSet<&str> = HashSet::new();
        let mut has_duplicate = false;
        for node in &structure.nodes {
            if !seen_ids.insert(node.id.as_str()) {
                has_duplicate = true;
            }
        }
        if has_duplicate {
            blockers.push(CanonicalBlocker::structure(CanonicalCause::DuplicateNodeId));
        }

        if structure.start_node_id.trim().is_empty()
            || !seen_ids.contains(structure.start_node_id.as_str())
        {
            blockers.push(CanonicalBlocker::structure(
                CanonicalCause::StartNodeInvalid,
            ));
        }

        let has_broken_link = structure
            .nodes
            .iter()
            .flat_map(|n| n.options.iter())
            .any(|o| {
                o.target
                    .as_deref()
                    .is_some_and(|target| !seen_ids.contains(target))
            });
        if has_broken_link {
            blockers.push(CanonicalBlocker::structure(
                CanonicalCause::BrokenOptionLink,
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

    // NOTE: a single EMPTY start node (no text, no media, no options) is VALID
    // in v3 — we deliberately emit NO "story without content" block (that
    // would make `présumée transférable` unreachable). The graph invariant
    // above only rejects an unusable graph, never a legitimately empty
    // starting node.

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

    const HEALTHY_JSON: &str = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";

    fn healthy_facts() -> CanonicalStoryFacts {
        CanonicalStoryFacts {
            title: "Mon histoire".into(),
            schema_version: 3,
            structure_json: HEALTHY_JSON.into(),
            content_checksum: content_checksum(HEALTHY_JSON),
        }
    }

    fn facts_for(json: &str) -> CanonicalStoryFacts {
        CanonicalStoryFacts {
            title: "Mon histoire".into(),
            schema_version: 3,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        }
    }

    #[test]
    fn healthy_story_has_no_blockers() {
        assert!(validate_canonical(&healthy_facts()).is_empty());
    }

    #[test]
    fn single_empty_start_node_is_valid_never_a_block() {
        // The minimal v3 form is a single empty start node; it must produce
        // zero blockers (otherwise `présumée transférable` would be
        // unreachable for a brand-new or migrated story).
        let facts = healthy_facts();
        assert_eq!(facts.structure_json, HEALTHY_JSON);
        assert!(validate_canonical(&facts).is_empty());
    }

    #[test]
    fn healthy_multi_node_graph_with_links_has_no_blockers() {
        // Two nodes, a linked option, an unlinked option and a self-reference:
        // a perfectly sound authoring state.
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"Bonjour\",\"label\":\"Début\",\"imageAssetId\":\"img\",\"audioAssetId\":\"aud\",\"options\":[{\"label\":\"Continuer\",\"target\":\"n2\"},{\"label\":\"Rester\",\"target\":\"n1\"}]},{\"id\":\"n2\",\"text\":\"Suite\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[{\"label\":\"Réfléchir\",\"target\":null}]}]}";
        assert!(validate_canonical(&facts_for(json)).is_empty());
    }

    #[test]
    fn start_on_a_non_first_node_is_valid() {
        // `startNodeId` designates the entry point; it does not have to be the
        // first node of the list (display order and entry point are distinct).
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n2\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]},{\"id\":\"n2\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
        assert!(validate_canonical(&facts_for(json)).is_empty());
    }

    #[test]
    fn zero_nodes_is_structure_corrupt_and_start_invalid() {
        // An empty graph has nothing to edit AND no reachable entry point —
        // both invariants report, each exactly once.
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[]}";
        let blockers = validate_canonical(&facts_for(json));
        assert_eq!(blockers.len(), 2);
        assert!(blockers.iter().any(
            |b| b.cause == CanonicalCause::StructureCorrupt && b.severity == Severity::Blocking
        ));
        assert!(blockers.iter().any(
            |b| b.cause == CanonicalCause::StartNodeInvalid && b.severity == Severity::Blocking
        ));
    }

    #[test]
    fn blank_node_id_is_structure_corrupt() {
        // Every node needs a stable, non-blank id to target writes and keep
        // links meaningful.
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]},{\"id\":\"   \",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
        let blockers = validate_canonical(&facts_for(json));
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::StructureCorrupt);
        assert_eq!(blockers[0].severity, Severity::Blocking);
    }

    #[test]
    fn duplicate_node_ids_are_a_blocking_duplicate_blocker() {
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]},{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
        let blockers = validate_canonical(&facts_for(json));
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::DuplicateNodeId);
        assert_eq!(blockers[0].severity, Severity::Blocking);
    }

    #[test]
    fn duplicate_ids_reported_once_even_when_tripled() {
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]},{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]},{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
        let dup = validate_canonical(&facts_for(json))
            .into_iter()
            .filter(|b| b.cause == CanonicalCause::DuplicateNodeId)
            .count();
        assert_eq!(dup, 1, "DuplicateNodeId must be reported exactly once");
    }

    #[test]
    fn start_node_absent_from_graph_is_start_node_invalid() {
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"nZ\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
        let blockers = validate_canonical(&facts_for(json));
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::StartNodeInvalid);
        assert_eq!(blockers[0].severity, Severity::Blocking);
    }

    #[test]
    fn blank_start_node_id_is_start_node_invalid() {
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"  \",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
        let blockers = validate_canonical(&facts_for(json));
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::StartNodeInvalid);
    }

    #[test]
    fn broken_option_link_is_a_fixable_blocker() {
        // An option pointing at a vanished node is repairable in the editor —
        // it must NOT block writes (deleting a referenced node stays possible)
        // nor unmount the projection.
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[{\"label\":\"Aller\",\"target\":\"nGone\"}]}]}";
        let blockers = validate_canonical(&facts_for(json));
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].cause, CanonicalCause::BrokenOptionLink);
        assert_eq!(blockers[0].severity, Severity::Fixable);
        assert_eq!(blockers[0].axis, Axis::Structure);
    }

    #[test]
    fn broken_links_reported_once_even_when_several() {
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[{\"label\":\"A\",\"target\":\"ghost1\"},{\"label\":\"B\",\"target\":\"ghost2\"}]}]}";
        let broken = validate_canonical(&facts_for(json))
            .into_iter()
            .filter(|b| b.cause == CanonicalCause::BrokenOptionLink)
            .count();
        assert_eq!(broken, 1, "BrokenOptionLink must be reported exactly once");
    }

    #[test]
    fn unlinked_option_emits_nothing() {
        // `target: null` is a normal authoring state (not linked yet), never a
        // blocker — unlinked ≠ broken.
        let json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[{\"label\":\"Plus tard\",\"target\":null}]}]}";
        assert!(validate_canonical(&facts_for(json)).is_empty());
    }

    #[test]
    fn graph_invariant_not_piled_onto_a_version_disagreement() {
        // A column↔JSON schema disagreement already flags StructureCorrupt;
        // the graph checks are skipped so blockers do not pile up on an
        // incoherent version.
        let json = "{\"schemaVersion\":0,\"startNodeId\":\"\",\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Incohérente".into(),
            schema_version: 3,
            structure_json: json.into(),
            content_checksum: content_checksum(json),
        };
        let blockers = validate_canonical(&facts);
        let corrupt = blockers
            .iter()
            .filter(|b| b.cause == CanonicalCause::StructureCorrupt)
            .count();
        assert_eq!(corrupt, 1, "StructureCorrupt must be reported exactly once");
        assert!(blockers
            .iter()
            .all(|b| b.cause != CanonicalCause::StartNodeInvalid));
    }

    #[test]
    fn media_cause_severities_split_attention_from_blocking() {
        assert_eq!(MediaCause::UnsupportedFormat.severity(), Severity::Blocking);
        assert_eq!(MediaCause::Unreadable.severity(), Severity::Blocking);
        assert_eq!(MediaCause::SourceMissing.severity(), Severity::Fixable);
    }

    #[test]
    fn new_structure_cause_severities_are_frozen() {
        assert_eq!(
            CanonicalCause::DuplicateNodeId.severity(),
            Severity::Blocking
        );
        assert_eq!(
            CanonicalCause::StartNodeInvalid.severity(),
            Severity::Blocking
        );
        assert_eq!(
            CanonicalCause::BrokenOptionLink.severity(),
            Severity::Fixable
        );
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
        // A self-consistent v4 (column == JSON == 4, both above the current 3)
        // is "format too recent" — the only path that means "update Rustory".
        let json = "{\"schemaVersion\":4,\"startNodeId\":\"n1\",\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Format trop récent".into(),
            schema_version: 4,
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
        // The column says v3, the JSON says v0 — incoherent → corruption, not
        // "format too recent".
        let json = "{\"schemaVersion\":0,\"startNodeId\":\"n1\",\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Incohérente".into(),
            schema_version: 3,
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
        // The column says v3 but the JSON says v4: a DISAGREEMENT is always
        // corruption, never "format too recent". `SchemaUnsupported` must be
        // reserved for a self-consistent newer artifact (column == JSON > current),
        // whose `userAction` is "update Rustory" — not a diverging/tampered row.
        let json = "{\"schemaVersion\":4,\"startNodeId\":\"n1\",\"nodes\":[]}";
        let facts = CanonicalStoryFacts {
            title: "Désaccord vers le récent".into(),
            schema_version: 3,
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
            schema_version: 3,
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
