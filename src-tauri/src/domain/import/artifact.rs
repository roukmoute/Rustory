//! Pure analysis of a parsed `.rustory` v1 artifact into a recognition
//! verdict (domain layer).
//!
//! Receives an ALREADY-deserialized [`RustoryArtifactV1`] — a malformed
//! file / unknown field is a parse failure the application layer turns
//! into an `Envelope` `Blocking` verdict before reaching this pure
//! function. Reuses the EXACT canonical re-validation a transfer runs
//! (`validate_canonical`, `content_checksum`, `normalize_title`) so the
//! import and the transfer agree on what "canonically valid" means; the
//! only import-specific logic is the format-version guard, the
//! title-normalization ambiguity and the timestamp-shape ambiguity.

use time::format_description::well_known::Iso8601;
use time::OffsetDateTime;

use crate::domain::export::{
    RustoryArtifactV1, RUSTORY_ARTIFACT_EXTENSION, RUSTORY_ARTIFACT_FORMAT_VERSION,
};
use crate::domain::story::{
    canonical_structure_json, content_checksum, normalize_title, validate_canonical,
    CanonicalCause, CanonicalStoryFacts, CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION,
};

use super::recognition::{
    import_state, recognition_quality, ImportState, RecognitionAspect, RecognitionFinding,
    RecognitionQuality,
};

/// Upper bound on a source artifact basename (filesystem-typical). A name
/// beyond this is refused rather than persisted as provenance.
pub const MAX_SOURCE_NAME_CHARS: usize = 255;

/// True iff `name` is a sober BASENAME of a SUPPORTED `.rustory` artifact:
/// non-empty, bounded, free of path separators / parent refs / NUL, with a
/// non-empty stem and the `.rustory` extension (case-insensitive). The
/// explicitly listed format is the authority — never the UI dialog filter
/// alone — and a provenance row never stores an absolute path (PII).
pub fn is_supported_artifact_source_name(name: &str) -> bool {
    if name.is_empty() || name.chars().count() > MAX_SOURCE_NAME_CHARS {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return false;
    }
    if name == "." || name == ".." {
        return false;
    }
    let suffix = format!(".{RUSTORY_ARTIFACT_EXTENSION}");
    name.to_ascii_lowercase()
        .strip_suffix(&suffix)
        .is_some_and(|stem| !stem.is_empty())
}

/// True iff `value` is EXACTLY 64 lowercase hex digits — the shape
/// [`content_checksum_bytes`] emits for an artifact fingerprint. The accept
/// boundary refuses any other shape so a forged provenance never lands.
///
/// [`content_checksum_bytes`]: crate::domain::story::content_checksum_bytes
pub fn is_artifact_checksum(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The validated canonical content carried from the analysis phase to the
/// accept phase (which re-validates it from zero). Present ONLY when the
/// artifact is importable (`quality != Unusable`).
///
/// The timestamps are PRESERVED verbatim from the artifact — a re-openable
/// imported story keeps its history; they are never rewritten to `now`
/// (that would be a silent data loss). `imported_at = now` lives on the
/// provenance row, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportableContent {
    /// The title exactly as carried by the artifact (PRE-normalization).
    /// Kept verbatim so the accept phase can re-derive the
    /// title-normalization ambiguity from zero; the canonical normalization
    /// (`normalize_title`) is applied at storage time, exactly like
    /// `create_story` normalizes a user-typed title.
    pub title: String,
    /// The structure JSON, byte-for-byte from the artifact (never
    /// reserialized — the checksum contract depends on it).
    pub structure_json: String,
    /// The declared checksum, already proven to equal
    /// `SHA-256(structure_json)` by the analysis (carried for the accept
    /// phase to re-prove, never to fabricate).
    pub content_checksum: String,
    pub created_at: String,
    pub updated_at: String,
}

/// The full outcome of analyzing a parsed artifact: the per-aspect
/// findings, the derived global quality + durable import state, and the
/// validated canonical content when importable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactAnalysis {
    pub findings: Vec<RecognitionFinding>,
    pub quality: RecognitionQuality,
    pub state: ImportState,
    pub importable: Option<ImportableContent>,
}

/// The canonical content of an artifact, decoupled from the on-disk
/// envelope. This is exactly what the accept phase re-validates from zero
/// (it never re-parses the envelope) — so the SAME [`analyze_components`]
/// derives the verdict at analysis time AND re-derives the durable state +
/// attention findings at commit time, with no logic duplicated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalContent {
    /// The title as carried by the artifact (pre-normalization).
    pub title: String,
    pub schema_version: u32,
    pub structure_json: String,
    pub content_checksum: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ArtifactAnalysis {
    /// The verdict for a file that failed to parse as a `.rustory` v1
    /// envelope (malformed JSON, unknown field, missing required field): a
    /// single `Envelope` `Blocking` finding → `Unusable` / `Blocked`, never
    /// importable. Built by the application layer when `serde_json` refuses
    /// the bytes, so a parse failure is a typed verdict, not an `AppError`.
    pub fn envelope_blocked() -> Self {
        Self {
            findings: vec![RecognitionFinding::blocking(RecognitionAspect::Envelope)],
            quality: RecognitionQuality::Unusable,
            state: ImportState::Blocked,
            importable: None,
        }
    }
}

/// The canonical v1 empty structure. Every v1 story carried this EXACT shape
/// (`nodes` was always empty, guaranteed by the type), so an artifact exported
/// before the v2 bump can be upgraded losslessly.
const LEGACY_V1_STRUCTURE: &str = "{\"schemaVersion\":1,\"nodes\":[]}";

/// Bring a LEGACY v1 canonical body up to the current v2 shape so a `.rustory`
/// exported before the v2 bump still imports (backward compatibility). The v1
/// structure was always the empty `nodes` list, so the upgrade injects the same
/// empty starting node the local v1→v2 migration backfills and recomputes the
/// checksum — lossless. Anything other than the exact canonical v1 empty body
/// is left untouched (a genuinely corrupt / non-canonical v1 stays blocked).
fn upgrade_legacy_v1(
    schema_version: u32,
    structure_json: &str,
    content_checksum_in: &str,
) -> (u32, String, String) {
    if schema_version == 1 && structure_json == LEGACY_V1_STRUCTURE {
        let json = canonical_structure_json(&CanonicalStructure::minimal());
        let checksum = content_checksum(&json);
        (CANONICAL_STORY_SCHEMA_VERSION, json, checksum)
    } else {
        (
            schema_version,
            structure_json.to_string(),
            content_checksum_in.to_string(),
        )
    }
}

/// Analyze a parsed `.rustory` v1 artifact. Pure: no I/O, deterministic on
/// its input. A legacy v1 canonical body is upgraded to v2 (lossless — the v1
/// structure was always empty) so older artifacts remain importable, then
/// delegates to [`analyze_components`] with the envelope's declared format
/// version.
pub fn analyze_rustory_artifact(artifact: &RustoryArtifactV1) -> ArtifactAnalysis {
    let story = &artifact.story;
    let (schema_version, structure_json, content_checksum) = upgrade_legacy_v1(
        story.schema_version,
        &story.structure_json,
        &story.content_checksum,
    );
    analyze_components(
        artifact.rustory_artifact.format_version,
        &CanonicalContent {
            title: story.title.clone(),
            schema_version,
            structure_json,
            content_checksum,
            created_at: story.created_at.clone(),
            updated_at: story.updated_at.clone(),
        },
    )
}

/// Classify a `.rustory` artifact from its declared format version + its
/// canonical content. Produces exactly one finding per aspect, derives the
/// global quality + state, and carries the validated canonical content when
/// importable. The accept phase calls this with the supported constant as
/// `format_version` (the envelope was already proven at analysis time) to
/// re-derive the durable state + attention findings from the SAME logic.
pub fn analyze_components(format_version: u32, content: &CanonicalContent) -> ArtifactAnalysis {
    let mut findings = Vec::with_capacity(7);

    // Envelope: by the time we hold `CanonicalContent` the on-disk envelope
    // parsed (the application layer maps a parse failure / unknown field to
    // an `Envelope` `Blocking` verdict before reaching here), so the
    // envelope is always recognized at this layer.
    findings.push(RecognitionFinding::recognized(RecognitionAspect::Envelope));

    // Format version: the bytes parsed as the V1 wire shape, but the
    // DECLARED version must be exactly the supported constant. A
    // `formatVersion` other than 1 is a forward/backward-compat block —
    // refused at the ANALYSIS level (the envelope parses; the analysis
    // blocks it), the live guard the export-side wire-shape test backs.
    findings.push(if format_version == RUSTORY_ARTIFACT_FORMAT_VERSION {
        RecognitionFinding::recognized(RecognitionAspect::FormatVersion)
    } else {
        RecognitionFinding::blocking(RecognitionAspect::FormatVersion)
    });

    // Canonical schema / structure / integrity / title: reuse the SAME
    // re-validation a transfer runs. Each canonical cause maps to exactly
    // one import aspect, so the import and the transfer never disagree on
    // what "canonically valid" means.
    let facts = CanonicalStoryFacts {
        title: content.title.clone(),
        schema_version: content.schema_version,
        structure_json: content.structure_json.clone(),
        content_checksum: content.content_checksum.clone(),
    };
    let blockers = validate_canonical(&facts);
    let blocked = |cause: CanonicalCause| blockers.iter().any(|b| b.cause == cause);

    findings.push(aspect_finding(
        RecognitionAspect::SchemaVersion,
        blocked(CanonicalCause::SchemaUnsupported),
    ));
    findings.push(aspect_finding(
        RecognitionAspect::Structure,
        blocked(CanonicalCause::StructureCorrupt),
    ));
    findings.push(aspect_finding(
        RecognitionAspect::Integrity,
        blocked(CanonicalCause::ChecksumMismatch),
    ));

    // Title: a blocking invalidity dominates; otherwise a value that
    // differs from its normalization is an ambiguity (the import will
    // store the normalized form — surfaced so the user knows). The title
    // is OUTSIDE the `content_checksum` digest, so a normalized title
    // never diverges the integrity check — the two are independent.
    if blocked(CanonicalCause::TitleInvalid) {
        findings.push(RecognitionFinding::blocking(RecognitionAspect::Title));
    } else if content.title != normalize_title(&content.title) {
        findings.push(RecognitionFinding::ambiguous(RecognitionAspect::Title));
    } else {
        findings.push(RecognitionFinding::recognized(RecognitionAspect::Title));
    }

    // Timestamps: PRESERVED as-is. A value off the canonical ISO-8601 UTC
    // millisecond shape (a hand-edited artifact) is an ambiguity, NEVER a
    // block and NEVER rewritten — fidelity beats silent normalization.
    let timestamps_ok =
        is_canonical_timestamp(&content.created_at) && is_canonical_timestamp(&content.updated_at);
    findings.push(if timestamps_ok {
        RecognitionFinding::recognized(RecognitionAspect::Timestamps)
    } else {
        RecognitionFinding::ambiguous(RecognitionAspect::Timestamps)
    });

    let quality = recognition_quality(&findings);
    let state = import_state(quality);
    let importable = if quality == RecognitionQuality::Unusable {
        None
    } else {
        Some(ImportableContent {
            // PRE-normalization title verbatim — the accept phase normalizes
            // it at storage and re-derives the normalization ambiguity from
            // this original value.
            title: content.title.clone(),
            structure_json: content.structure_json.clone(),
            content_checksum: content.content_checksum.clone(),
            created_at: content.created_at.clone(),
            updated_at: content.updated_at.clone(),
        })
    };

    ArtifactAnalysis {
        findings,
        quality,
        state,
        importable,
    }
}

fn aspect_finding(aspect: RecognitionAspect, is_blocked: bool) -> RecognitionFinding {
    if is_blocked {
        RecognitionFinding::blocking(aspect)
    } else {
        RecognitionFinding::recognized(aspect)
    }
}

/// Validate the canonical `YYYY-MM-DDTHH:MM:SS.sssZ` ISO-8601 UTC
/// millisecond timestamp. TWO layers: the exact byte SHAPE (the form
/// `now_iso_ms` emits and the export carries) AND a real ISO-8601 parse, so
/// a string that looks canonical but encodes an impossible instant
/// (`9999-99-99T99:99:99.999Z`) is rejected — preserved verbatim and tagged
/// `Ambiguous`, never silently accepted as `Recognized`.
fn is_canonical_timestamp(ts: &str) -> bool {
    has_canonical_timestamp_shape(ts) && OffsetDateTime::parse(ts, &Iso8601::DEFAULT).is_ok()
}

/// The exact byte shape of the canonical timestamp — pins the form (24
/// chars, fixed separators, ASCII digits) independently of the value parse,
/// so a non-canonical FORM (e.g. missing milliseconds, space instead of
/// `T`) is rejected even when `time` would tolerate it.
fn has_canonical_timestamp_shape(ts: &str) -> bool {
    let bytes = ts.as_bytes();
    if bytes.len() != 24 {
        return false;
    }
    for (index, expected) in [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'.'),
        (23, b'Z'),
    ] {
        if bytes[index] != expected {
            return false;
        }
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::export::{ArtifactEnvelopeV1, ExportedStoryV1};
    use crate::domain::import::recognition::RecognitionCategory;
    use crate::domain::story::content_checksum;

    const CANONICAL_STRUCTURE: &str = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";

    /// A pristine artifact as Rustory's own export would produce it: a
    /// normalized title, canonical timestamps, a checksum over the
    /// structure JSON.
    fn clean_artifact() -> RustoryArtifactV1 {
        RustoryArtifactV1 {
            rustory_artifact: ArtifactEnvelopeV1 {
                format_version: RUSTORY_ARTIFACT_FORMAT_VERSION,
                exported_at: "2026-06-27T10:00:00.000Z".into(),
                exported_by: "rustory/0.1.0".into(),
            },
            story: ExportedStoryV1 {
                schema_version: 2,
                title: "Le Soleil Couchant".into(),
                structure_json: CANONICAL_STRUCTURE.into(),
                content_checksum: content_checksum(CANONICAL_STRUCTURE),
                created_at: "2026-06-20T10:00:00.000Z".into(),
                updated_at: "2026-06-24T14:15:00.000Z".into(),
            },
        }
    }

    fn category_of(analysis: &ArtifactAnalysis, aspect: RecognitionAspect) -> RecognitionCategory {
        analysis
            .findings
            .iter()
            .find(|f| f.aspect == aspect)
            .unwrap_or_else(|| panic!("a finding must exist for {aspect:?}"))
            .category
    }

    #[test]
    fn produces_exactly_one_finding_per_aspect() {
        let analysis = analyze_rustory_artifact(&clean_artifact());
        for aspect in [
            RecognitionAspect::Envelope,
            RecognitionAspect::FormatVersion,
            RecognitionAspect::SchemaVersion,
            RecognitionAspect::Structure,
            RecognitionAspect::Integrity,
            RecognitionAspect::Title,
            RecognitionAspect::Timestamps,
        ] {
            let count = analysis
                .findings
                .iter()
                .filter(|f| f.aspect == aspect)
                .count();
            assert_eq!(count, 1, "exactly one finding for {aspect:?}");
        }
        assert_eq!(analysis.findings.len(), 7);
    }

    #[test]
    fn clean_artifact_is_recognized_and_importable() {
        let analysis = analyze_rustory_artifact(&clean_artifact());
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        assert_eq!(analysis.state, ImportState::Recognized);
        assert!(analysis
            .findings
            .iter()
            .all(|f| f.category == RecognitionCategory::Recognized));
        let content = analysis.importable.expect("a clean artifact is importable");
        assert_eq!(content.title, "Le Soleil Couchant");
        assert_eq!(content.structure_json, CANONICAL_STRUCTURE);
        assert_eq!(
            content.content_checksum,
            content_checksum(CANONICAL_STRUCTURE)
        );
        // Timestamps are PRESERVED verbatim.
        assert_eq!(content.created_at, "2026-06-20T10:00:00.000Z");
        assert_eq!(content.updated_at, "2026-06-24T14:15:00.000Z");
    }

    #[test]
    fn a_legacy_v1_artifact_is_upgraded_and_imports_clean() {
        // An artifact exported BEFORE the v2 bump (a v1 empty body) must remain
        // importable: it is upgraded losslessly to the v2 starting node, exactly
        // like the local v1→v2 migration backfills it.
        let mut artifact = clean_artifact();
        artifact.story.schema_version = 1;
        artifact.story.structure_json = LEGACY_V1_STRUCTURE.into();
        artifact.story.content_checksum = content_checksum(LEGACY_V1_STRUCTURE);

        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(
            analysis.quality,
            RecognitionQuality::Clean,
            "a legacy v1 artifact imports clean after the upgrade"
        );
        let content = analysis.importable.expect("importable after upgrade");
        let expected = canonical_structure_json(&CanonicalStructure::minimal());
        assert_eq!(content.structure_json, expected);
        assert_eq!(content.content_checksum, content_checksum(&expected));
    }

    #[test]
    fn a_non_canonical_v1_structure_is_not_upgraded() {
        // Only the EXACT canonical v1 empty body is upgraded — a v1 with an
        // unexpected structure is genuinely corrupt and stays blocked.
        let mut artifact = clean_artifact();
        let tampered = "{\"schemaVersion\":1,\"nodes\":[{}]}";
        artifact.story.schema_version = 1;
        artifact.story.structure_json = tampered.into();
        artifact.story.content_checksum = content_checksum(tampered);
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
    }

    #[test]
    fn a_non_normalized_title_is_partial_needs_review_but_importable() {
        let mut artifact = clean_artifact();
        artifact.story.title = "  Le Soleil Couchant  ".into(); // leading/trailing spaces
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Partial);
        assert_eq!(analysis.state, ImportState::NeedsReview);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Ambiguous
        );
        // Importable: the title is carried VERBATIM (pre-normalization), so
        // the accept phase can re-derive the ambiguity; storage normalizes it.
        let content = analysis.importable.expect("partial is still importable");
        assert_eq!(content.title, "  Le Soleil Couchant  ");
    }

    #[test]
    fn a_non_canonical_timestamp_is_partial_needs_review_and_preserved() {
        let mut artifact = clean_artifact();
        artifact.story.updated_at = "2026-06-24T14:15:00Z".into(); // no milliseconds
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Partial);
        assert_eq!(analysis.state, ImportState::NeedsReview);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Timestamps),
            RecognitionCategory::Ambiguous
        );
        // The malformed timestamp is PRESERVED, never rewritten.
        let content = analysis.importable.expect("still importable");
        assert_eq!(content.updated_at, "2026-06-24T14:15:00Z");
    }

    #[test]
    fn a_diverging_checksum_is_unusable_blocked_not_importable() {
        let mut artifact = clean_artifact();
        artifact.story.content_checksum = "0".repeat(64); // wrong digest
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(analysis.state, ImportState::Blocked);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Integrity),
            RecognitionCategory::Blocking
        );
        assert!(
            analysis.importable.is_none(),
            "a corrupt artifact is not importable"
        );
    }

    #[test]
    fn a_non_canonical_structure_is_unusable_blocked() {
        let mut artifact = clean_artifact();
        let tampered = "{\"schemaVersion\":2,\"nodes\":[]}"; // zero nodes leaves the single-node v2 model
        artifact.story.structure_json = tampered.into();
        artifact.story.content_checksum = content_checksum(tampered);
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn a_schema_above_supported_is_unusable_blocked_on_schema_version() {
        let mut artifact = clean_artifact();
        let future = "{\"schemaVersion\":3,\"nodes\":[]}";
        artifact.story.schema_version = 3;
        artifact.story.structure_json = future.into();
        artifact.story.content_checksum = content_checksum(future);
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::SchemaVersion),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn an_empty_title_is_unusable_blocked_on_title() {
        let mut artifact = clean_artifact();
        artifact.story.title = "   ".into();
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Blocking
        );
    }

    /// Activates the forward-compatibility guard the export-side
    /// `#[ignore]`d wire-shape test only documented: the refusal is at the
    /// ANALYSIS level (the V1 envelope parses, the analysis blocks it).
    #[test]
    fn rejects_format_version_zero_at_analysis() {
        let mut artifact = clean_artifact();
        artifact.rustory_artifact.format_version = 0;
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(analysis.state, ImportState::Blocked);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::FormatVersion),
            RecognitionCategory::Blocking
        );
        assert!(analysis.importable.is_none());
    }

    #[test]
    fn rejects_a_future_format_version_too() {
        let mut artifact = clean_artifact();
        artifact.rustory_artifact.format_version = 2;
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::FormatVersion),
            RecognitionCategory::Blocking
        );
    }

    /// The `.rustory` flow (single story, `nodes: []`) NEVER emits the
    /// `Missing` finding category nor the `Partial` / `Resolved` import
    /// states — they are DECLARED for the deferred structured multi-element
    /// import. Mirrors the negative test on `Axis::Media` / `Axis::Filesystem`.
    #[test]
    fn never_emits_declared_but_unsupported_categories_or_states() {
        let candidates = {
            let mut bad_title = clean_artifact();
            bad_title.story.title = "  spaced  ".into();
            let mut bad_ts = clean_artifact();
            bad_ts.story.updated_at = "not-a-date".into();
            let mut bad_checksum = clean_artifact();
            bad_checksum.story.content_checksum = "f".repeat(64);
            let mut bad_format = clean_artifact();
            bad_format.rustory_artifact.format_version = 9;
            [
                clean_artifact(),
                bad_title,
                bad_ts,
                bad_checksum,
                bad_format,
            ]
        };
        for artifact in &candidates {
            let analysis = analyze_rustory_artifact(artifact);
            assert!(
                analysis
                    .findings
                    .iter()
                    .all(|f| f.category != RecognitionCategory::Missing),
                "the .rustory flow never emits a Missing finding"
            );
            assert_ne!(
                analysis.state,
                ImportState::Partial,
                "the .rustory flow never emits the Partial state"
            );
            assert_ne!(
                analysis.state,
                ImportState::Resolved,
                "the .rustory flow never emits the Resolved state"
            );
        }
    }

    #[test]
    fn canonical_timestamp_shape_validator() {
        assert!(is_canonical_timestamp("2026-06-27T10:00:00.000Z"));
        assert!(!is_canonical_timestamp("2026-06-27T10:00:00Z")); // no ms
        assert!(!is_canonical_timestamp("2026-06-27 10:00:00.000Z")); // space, not T
        assert!(!is_canonical_timestamp("2026-06-27T10:00:00.000")); // no Z
        assert!(!is_canonical_timestamp("")); // empty
        assert!(!is_canonical_timestamp("xxxx-06-27T10:00:00.000Z")); // non-digit
    }

    #[test]
    fn an_impossible_date_with_a_canonical_shape_is_rejected() {
        // The shape is canonical (24 chars, right separators, digits) but the
        // instant is impossible — the real ISO-8601 parse must reject it.
        assert!(!is_canonical_timestamp("9999-99-99T99:99:99.999Z"));
        assert!(!is_canonical_timestamp("2026-13-01T10:00:00.000Z")); // month 13
        assert!(!is_canonical_timestamp("2026-02-30T10:00:00.000Z")); // 30 Feb
    }

    #[test]
    fn an_impossible_date_is_ambiguous_needs_review_and_preserved() {
        let mut artifact = clean_artifact();
        artifact.story.created_at = "9999-99-99T99:99:99.999Z".into();
        let analysis = analyze_rustory_artifact(&artifact);
        assert_eq!(analysis.quality, RecognitionQuality::Partial);
        assert_eq!(analysis.state, ImportState::NeedsReview);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Timestamps),
            RecognitionCategory::Ambiguous
        );
        // The impossible date is PRESERVED verbatim, never rewritten.
        let content = analysis.importable.expect("still importable");
        assert_eq!(content.created_at, "9999-99-99T99:99:99.999Z");
    }

    #[test]
    fn supported_artifact_source_name_accepts_a_sober_rustory_basename() {
        assert!(is_supported_artifact_source_name("histoire.rustory"));
        assert!(is_supported_artifact_source_name("Mon Histoire.RUSTORY")); // case-insensitive
        assert!(is_supported_artifact_source_name("a.txt.rustory"));
    }

    #[test]
    fn supported_artifact_source_name_refuses_non_rustory_paths_and_pii() {
        assert!(!is_supported_artifact_source_name("histoire.txt")); // wrong extension
        assert!(!is_supported_artifact_source_name(
            "/home/u/histoire.rustory"
        )); // absolute path
        assert!(!is_supported_artifact_source_name("dir\\histoire.rustory")); // backslash
        assert!(!is_supported_artifact_source_name(".rustory")); // empty stem
        assert!(!is_supported_artifact_source_name("..")); // parent ref
        assert!(!is_supported_artifact_source_name("")); // empty
        assert!(!is_supported_artifact_source_name(&format!(
            "{}.rustory",
            "a".repeat(300)
        ))); // oversize
    }

    #[test]
    fn artifact_checksum_validator() {
        assert!(is_artifact_checksum(&"a".repeat(64)));
        assert!(is_artifact_checksum(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_artifact_checksum(&"a".repeat(63))); // too short
        assert!(!is_artifact_checksum(&"A".repeat(64))); // uppercase
        assert!(!is_artifact_checksum(&"g".repeat(64))); // non-hex
        assert!(!is_artifact_checksum("")); // empty
    }
}
