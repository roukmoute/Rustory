//! Official local-artifact registry: WHICH local artifact types
//! (`.rustory`, structured folder, structured archive…) the official
//! distribution supports, decided line by line — the exact pattern of
//! the device support matrix (`domain::device::support_matrix`) and of
//! the content-source registry (`content_source`). The registry
//! mirrors WORD FOR WORD the `Supported local artifact types` table of
//! `docs/architecture/device-support-profile.md#Local Artifact Import
//! Contract`: a kind lands here when the product speaks about it — NOT
//! when its flow is implemented (the structured archive is a known
//! kind whose reader is deliberately absent, zero-dependency rule).
//!
//! Pure domain: kind in, documented capabilities out, zero I/O.

/// Closed set of KNOWN local artifact kinds — one variant per line of
/// the documented table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalArtifactKind {
    /// The `.rustory` v1 single-file story artifact (import + export).
    RustoryArtifact,
    /// The structured local folder (`histoire.json` + referenced
    /// media) used as a story-creation entry point.
    StructuredFolder,
    /// The structured archive (zip…) — a KNOWN kind whose reader is
    /// deliberately absent (deferred, zero-dependency rule).
    StructuredArchive,
}

/// Every known kind, in the stable rendering order of the documented
/// table. Tripwire: a new enum variant fails the exhaustive `match`
/// below, forcing an explicit registry decision for it.
pub const ALL_LOCAL_ARTIFACT_KINDS: [LocalArtifactKind; 3] = [
    LocalArtifactKind::RustoryArtifact,
    LocalArtifactKind::StructuredFolder,
    LocalArtifactKind::StructuredArchive,
];

impl LocalArtifactKind {
    /// Stable camelCase wire tag (support-profile DTO). Must stay
    /// byte-identical to the TS mirror's closed set.
    pub const fn wire_tag(self) -> &'static str {
        match self {
            Self::RustoryArtifact => "rustoryArtifact",
            Self::StructuredFolder => "structuredFolder",
            Self::StructuredArchive => "structuredArchive",
        }
    }
}

/// The closed capability view of a local artifact line — each field
/// mirrors one word of the documented table's `Status` column
/// (`import + export` / `creation`). Derived from
/// [`LocalArtifactSupport`]: the bundles are the only representable
/// combinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalArtifactCapabilities {
    /// The artifact imports into the library (`Importer une histoire`).
    pub import_artifact: bool,
    /// A library story exports to this artifact.
    pub export_artifact: bool,
    /// The artifact creates a new canonical story.
    pub create_story: bool,
}

impl LocalArtifactCapabilities {
    /// All-false constant: the unambiguous deferred baseline.
    pub const NONE: Self = Self {
        import_artifact: false,
        export_artifact: false,
        create_story: false,
    };

    /// Whether the view offers ANY capability — a line without one is
    /// the documented deferred line, rendered honest, never invented.
    pub fn offers_any(self) -> bool {
        self.import_artifact || self.export_artifact || self.create_story
    }
}

/// Support state of ONE artifact registry line: the CLOSED set of
/// DOCUMENTED capability bundles, or the deferred state WITH its
/// frozen user-facing reason. A partial/unnamed capability combination
/// and a reason-less deferral are both unrepresentable by construction
/// — the registry can never promise an operation the documented bundle
/// does not carry, and a limit can never reach the screen as a bare ✗.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalArtifactSupport {
    /// The documented `.rustory` bundle: import + export.
    ImportAndExport,
    /// The documented structured-folder bundle: story creation.
    StoryCreation,
    /// The line is deferred, with its frozen reason.
    Deferred { reason: &'static str },
}

impl LocalArtifactSupport {
    pub const fn is_available(self) -> bool {
        !matches!(self, Self::Deferred { .. })
    }

    /// The frozen reason of a deferred line — `None` on an available
    /// one (the availability itself replaces it).
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::ImportAndExport | Self::StoryCreation => None,
            Self::Deferred { reason } => Some(reason),
        }
    }

    /// The capability view of the bundle — derived, so it can never
    /// diverge from the bundle the line declares.
    pub const fn capabilities(self) -> LocalArtifactCapabilities {
        match self {
            Self::ImportAndExport => LocalArtifactCapabilities {
                import_artifact: true,
                export_artifact: true,
                create_story: false,
            },
            Self::StoryCreation => LocalArtifactCapabilities {
                import_artifact: false,
                export_artifact: false,
                create_story: true,
            },
            Self::Deferred { .. } => LocalArtifactCapabilities::NONE,
        }
    }
}

/// One line of the official registry: a known kind, the format version
/// the table documents (`None` when the table documents none — no
/// value is ever invented) and the support bundle the distribution
/// decides on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalArtifactLine {
    pub kind: LocalArtifactKind,
    pub format_version: Option<u8>,
    pub support: LocalArtifactSupport,
}

/// THE official local-artifact registry of this distribution —
/// supported line by line, never wholesale (the device support-matrix
/// pattern: every line carries its own justification).
const OFFICIAL_LOCAL_ARTIFACTS: &[LocalArtifactLine] = &[
    // `.rustory` v1 ✅ import + export — the single-file story
    // artifact, inverse of the export flow (`formatVersion == 1`).
    LocalArtifactLine {
        kind: LocalArtifactKind::RustoryArtifact,
        format_version: Some(1),
        support: LocalArtifactSupport::ImportAndExport,
    },
    // Structured folder v1 ✅ creation — `histoire.json` + referenced
    // media, the story-creation entry point (`formatVersion == 1`).
    LocalArtifactLine {
        kind: LocalArtifactKind::StructuredFolder,
        format_version: Some(1),
        support: LocalArtifactSupport::StoryCreation,
    },
    // Structured archive ❌ deferred — no archive reader ships
    // (zero-dependency rule); the line stays VISIBLE with its honest
    // frozen reason, never silently dropped (the not-activated
    // content-source pattern). No format version is documented, none
    // is invented.
    LocalArtifactLine {
        kind: LocalArtifactKind::StructuredArchive,
        format_version: None,
        support: LocalArtifactSupport::Deferred {
            reason: "Lecture d'archives non prise en charge",
        },
    },
];

/// The official registry, as a borrowed slice: the support-profile
/// wire serializes it line by line.
pub fn official_local_artifacts() -> &'static [LocalArtifactLine] {
    OFFICIAL_LOCAL_ARTIFACTS
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== The official registry — one line = one test, mirroring the
    // documented `Supported local artifact types` table. =====

    #[test]
    fn official_registry_supports_rustory_artifact_for_import_and_export() {
        let line = official_local_artifacts()
            .iter()
            .find(|line| line.kind == LocalArtifactKind::RustoryArtifact)
            .expect("rustory line");
        assert_eq!(line.format_version, Some(1));
        assert_eq!(line.support, LocalArtifactSupport::ImportAndExport);
        let capabilities = line.support.capabilities();
        assert!(capabilities.import_artifact);
        assert!(capabilities.export_artifact);
        assert!(!capabilities.create_story);
    }

    #[test]
    fn official_registry_supports_structured_folder_for_creation() {
        let line = official_local_artifacts()
            .iter()
            .find(|line| line.kind == LocalArtifactKind::StructuredFolder)
            .expect("folder line");
        assert_eq!(line.format_version, Some(1));
        assert_eq!(line.support, LocalArtifactSupport::StoryCreation);
        let capabilities = line.support.capabilities();
        assert!(!capabilities.import_artifact);
        assert!(!capabilities.export_artifact);
        assert!(capabilities.create_story);
    }

    #[test]
    fn official_registry_defers_the_structured_archive_with_a_non_empty_reason() {
        // Documents the CURRENT distribution state: the archive kind is
        // KNOWN (its label and honest reason render on the support
        // profile) but no capability ships — no archive reader exists
        // (zero-dependency rule). A capability appearing one day is an
        // announced re-scope of this test.
        let line = official_local_artifacts()
            .iter()
            .find(|line| line.kind == LocalArtifactKind::StructuredArchive)
            .expect("archive line");
        assert_eq!(line.format_version, None, "no version is ever invented");
        assert!(!line.support.is_available());
        let reason = line.support.reason().expect("deferred carries a reason");
        assert_eq!(reason, "Lecture d'archives non prise en charge");
        assert_eq!(line.support.capabilities(), LocalArtifactCapabilities::NONE);
    }

    #[test]
    fn official_registry_carries_every_known_kind_exactly_once() {
        for kind in ALL_LOCAL_ARTIFACT_KINDS {
            let lines = official_local_artifacts()
                .iter()
                .filter(|line| line.kind == kind)
                .count();
            assert_eq!(lines, 1, "kind {kind:?} must have exactly one line");
        }
        assert_eq!(
            official_local_artifacts().len(),
            ALL_LOCAL_ARTIFACT_KINDS.len(),
            "no line may carry an unknown kind"
        );
    }

    #[test]
    fn official_registry_preserves_the_documented_rendering_order() {
        let kinds: Vec<LocalArtifactKind> = official_local_artifacts()
            .iter()
            .map(|line| line.kind)
            .collect();
        assert_eq!(kinds, ALL_LOCAL_ARTIFACT_KINDS.to_vec());
    }

    // ===== Support bundles — the closed representable combinations =====

    #[test]
    fn each_bundle_derives_exactly_its_documented_capabilities() {
        // The bundles are a CLOSED sum: a partial combination (e.g. an
        // import-only `.rustory`) is unrepresentable — there is no
        // variant to build it from, so the registry can never promise
        // an operation the documented bundle does not carry.
        assert_eq!(
            LocalArtifactSupport::ImportAndExport.capabilities(),
            LocalArtifactCapabilities {
                import_artifact: true,
                export_artifact: true,
                create_story: false,
            }
        );
        assert_eq!(
            LocalArtifactSupport::StoryCreation.capabilities(),
            LocalArtifactCapabilities {
                import_artifact: false,
                export_artifact: false,
                create_story: true,
            }
        );
        assert_eq!(
            LocalArtifactSupport::Deferred { reason: "why" }.capabilities(),
            LocalArtifactCapabilities::NONE
        );
    }

    #[test]
    fn availability_and_reason_are_coherent_by_construction() {
        assert!(LocalArtifactSupport::ImportAndExport.is_available());
        assert_eq!(LocalArtifactSupport::ImportAndExport.reason(), None);
        assert!(LocalArtifactSupport::StoryCreation.is_available());
        assert_eq!(LocalArtifactSupport::StoryCreation.reason(), None);
        let deferred = LocalArtifactSupport::Deferred { reason: "why" };
        assert!(!deferred.is_available());
        assert_eq!(deferred.reason(), Some("why"));
    }

    #[test]
    fn none_capabilities_offer_nothing() {
        assert!(!LocalArtifactCapabilities::NONE.offers_any());
    }

    #[test]
    fn offers_any_is_true_when_any_single_capability_is_set() {
        for capabilities in [
            LocalArtifactCapabilities {
                import_artifact: true,
                ..LocalArtifactCapabilities::NONE
            },
            LocalArtifactCapabilities {
                export_artifact: true,
                ..LocalArtifactCapabilities::NONE
            },
            LocalArtifactCapabilities {
                create_story: true,
                ..LocalArtifactCapabilities::NONE
            },
        ] {
            assert!(capabilities.offers_any(), "{capabilities:?}");
        }
    }

    // ===== Wire tags — stable, distinct, exhaustive =====

    #[test]
    fn kind_wire_tags_are_stable() {
        // Exhaustive by construction: iterating the ALL_ tripwire array.
        let tags: Vec<&str> = ALL_LOCAL_ARTIFACT_KINDS
            .iter()
            .map(|kind| kind.wire_tag())
            .collect();
        assert_eq!(
            tags,
            vec!["rustoryArtifact", "structuredFolder", "structuredArchive"]
        );
    }

    #[test]
    fn wire_tags_are_pairwise_distinct() {
        for (i, a) in ALL_LOCAL_ARTIFACT_KINDS.iter().enumerate() {
            for b in &ALL_LOCAL_ARTIFACT_KINDS[i + 1..] {
                assert_ne!(a.wire_tag(), b.wire_tag());
            }
        }
    }

    #[test]
    fn all_local_artifact_kinds_tripwire_is_exhaustive() {
        // Compile-time tripwire: adding a kind variant breaks this
        // exhaustive match, forcing ALL_LOCAL_ARTIFACT_KINDS (and the
        // official registry, through the exactly-once test above) to
        // absorb the newcomer explicitly.
        for kind in ALL_LOCAL_ARTIFACT_KINDS {
            match kind {
                LocalArtifactKind::RustoryArtifact
                | LocalArtifactKind::StructuredFolder
                | LocalArtifactKind::StructuredArchive => {}
            }
        }
    }
}
