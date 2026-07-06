//! Structured-folder creation domain (v1 author manifest) — pure, no I/O.
//!
//! The structured folder is an AUTHOR format (`histoire.json` + referenced
//! media), not a machine artifact: unlike the `.rustory` parse
//! (`deny_unknown_fields`), the manifest is parsed TOLERANTLY through
//! `serde_json::Value` — an unknown field never rejects, it produces an
//! `Ambiguous` finding (a typo is flagged, not punished). The analysis is
//! pure: every media probe (existence, regularity, size, magic-byte sniff)
//! is an INPUT provided by the application layer, so the whole matrix is
//! testable without a disk. `validate_canonical` stays the final oracle on
//! the transcoded structure — the folder flow and the editor never disagree
//! on what "canonically valid" means.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::domain::story::{
    canonical_structure_json, content_checksum, normalize_title, validate_canonical,
    validate_title, CanonicalCause, CanonicalNode, CanonicalOption, CanonicalStoryFacts,
    CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION,
};

use super::artifact::MAX_SOURCE_NAME_CHARS;
use super::recognition::{
    folder_import_state, recognition_quality, ImportState, RecognitionAspect, RecognitionCategory,
    RecognitionFinding, RecognitionQuality,
};

/// The exact manifest file name — ONE listed name, no alias (AC2).
pub const STRUCTURED_FOLDER_MANIFEST_NAME: &str = "histoire.json";

/// The supported structured-folder format version (forward guard: anything
/// else blocks, like the `.rustory` envelope).
pub const STRUCTURED_FOLDER_FORMAT_VERSION: u64 = 1;

/// Ceiling on the DISTINCT media files a manifest may reference (anti-DoS:
/// bounds the probe/promotion work). Exceeding it is a `Structure` blocking
/// finding — a typed verdict, never a transport error.
pub const MAX_FOLDER_MEDIA_FILES: usize = 64;

/// Ceiling on the SUM of the retained media byte sizes (anti-DoS: bounds
/// the total acceptance I/O). Exceeding it is a `Structure` blocking finding.
pub const MAX_FOLDER_TOTAL_MEDIA_BYTES: u64 = 256 * 1024 * 1024;

/// Per-node text/label ceilings — mirrors of the editor's write-path bounds
/// (`application::story::node`), re-declared here because the domain never
/// imports the application layer; a test locks the equality.
pub const MAX_FOLDER_NODE_TEXT_CHARS: usize = 65536;
pub const MAX_FOLDER_NODE_LABEL_CHARS: usize = 4096;

/// The two media slots a manifest node can reference. Domain-pure mirror of
/// the infrastructure `MediaKind` (the application layer maps the two).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderMediaKind {
    Image,
    Audio,
}

/// What the application probed for ONE distinct referenced basename. All
/// I/O happens in the caller; the analysis only consumes these facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaProbe {
    /// Present, regular, within the per-file bound, sniffed inside the
    /// closed set. `kind` is what the MAGIC BYTES say — a reference whose
    /// slot disagrees with it is discarded as `Ambiguous` (wrong slot).
    Usable {
        kind: FolderMediaKind,
        byte_size: u64,
    },
    /// Referenced but absent from the folder → `Missing` (discarded).
    Absent,
    /// Present but unusable: symlink / irregular file / oversize / bytes
    /// outside the closed format set → `Ambiguous` (discarded).
    Unusable,
}

/// One media reference retained by the analysis: which node, which slot,
/// which (sober, probed-usable, slot-matching) basename. The accept phase
/// promotes each basename once and wires the asset id into the node's slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedMediaRef {
    pub node_id: String,
    pub kind: FolderMediaKind,
    pub basename: String,
}

/// The creatable content carried by a non-blocked analysis: the title
/// (verbatim, PRE-normalization — storage normalizes, exactly like
/// `create_story`), the transcoded canonical structure WITHOUT asset ids
/// (wired at acceptance, after promotion), and the retained media.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatableStory {
    pub title: String,
    pub structure: CanonicalStructure,
    pub retained_media: Vec<RetainedMediaRef>,
}

/// The full outcome of analyzing a structured folder: the per-aspect
/// findings (exactly one per analyzed aspect), the derived global quality +
/// durable state (folder derivation), the creatable content when not
/// blocked, and the discarded basenames (report detail — never persisted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredFolderAnalysis {
    pub findings: Vec<RecognitionFinding>,
    pub quality: RecognitionQuality,
    pub state: ImportState,
    pub creatable: Option<CreatableStory>,
    /// Distinct referenced basenames discarded by the analysis (absent or
    /// unusable), in first-reference order. Feeds the analysis surface's
    /// summary; the persisted findings stay aggregated pairs.
    pub discarded_media: Vec<String>,
}

impl StructuredFolderAnalysis {
    /// The verdict for a folder whose manifest cannot be read or parsed
    /// (absent, irregular, over the byte bound, malformed JSON): a single
    /// `Envelope` blocking finding — the exact calque of the `.rustory`
    /// `envelope_blocked`, built by the application layer for read
    /// failures and by the parse for malformed bytes.
    pub fn envelope_blocked() -> Self {
        let findings = vec![RecognitionFinding::blocking(RecognitionAspect::Envelope)];
        let quality = recognition_quality(&findings);
        let state = folder_import_state(&findings);
        Self {
            findings,
            quality,
            state,
            creatable: None,
            discarded_media: Vec::new(),
        }
    }
}

/// True iff `name` is a sober basename for the FOLDER provenance row:
/// non-empty, bounded, free of path separators / parent refs / NUL. Same
/// sobriety rules as the `.rustory` source name WITHOUT the extension
/// requirement (a folder has none). The provenance never stores an
/// absolute path (PII).
pub fn is_supported_folder_source_name(name: &str) -> bool {
    is_sober_component(name)
}

/// True iff `name` is a sober basename for a REFERENCED media file — the
/// same sobriety rules, applied BEFORE any path join so a manifest can
/// never smuggle a traversal into the probe/promotion I/O.
pub fn is_sober_media_basename(name: &str) -> bool {
    is_sober_component(name)
}

fn is_sober_component(name: &str) -> bool {
    if name.is_empty() || name.chars().count() > MAX_SOURCE_NAME_CHARS {
        return false;
    }
    // A blank-after-trim name would render as a dangling provenance /
    // report line while being a real directory entry — refused as sober.
    if name.trim().is_empty() {
        return false;
    }
    // `/` and `\` are path separators; `:` makes a Windows drive-relative
    // path (`c:evil` joins OUTSIDE the chosen folder there — and `:` is
    // illegal in Windows file names anyway, so rejecting it is strictly
    // safe); control characters (NUL included) never belong to a sober
    // name a provenance row or a report line can carry.
    if name
        .chars()
        .any(|c| c == '/' || c == '\\' || c == ':' || c.is_control())
    {
        return false;
    }
    name != "." && name != ".."
}

/// True iff the parsed manifest declares the LISTED format version. The
/// media references of an unlisted format are NEVER probed nor evaluated
/// (AC2: no implicit / partial support) — the verdict blocks on
/// `FormatVersion` without a single media read.
fn passes_format_gate(root: &Value) -> bool {
    root.get("formatVersion").and_then(Value::as_u64) == Some(STRUCTURED_FOLDER_FORMAT_VERSION)
}

/// The DISTINCT sober basenames referenced by the manifest, in first
/// reference order — exactly what the application must probe (and nothing
/// else: a non-sober basename is never returned, so it can never reach a
/// path join; the analysis discards it as `Ambiguous` on its own). An
/// unreadable manifest, one whose declared format is not the listed one,
/// or one referencing MORE distinct files than [`MAX_FOLDER_MEDIA_FILES`]
/// yields an EMPTY list: an unlisted or bounds-breaking manifest never
/// triggers a single media read — the verdict blocks without I/O.
pub fn referenced_media(manifest_bytes: &[u8]) -> Vec<String> {
    let Ok(root) = serde_json::from_slice::<Value>(manifest_bytes) else {
        return Vec::new();
    };
    if !passes_format_gate(&root) {
        return Vec::new();
    }
    let raw_refs = raw_media_refs(&root);
    if exceeds_media_file_bound(&raw_refs) {
        return Vec::new();
    }
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for reference in raw_refs {
        if is_sober_media_basename(&reference.basename) && seen.insert(reference.basename.clone()) {
            ordered.push(reference.basename);
        }
    }
    ordered
}

/// True iff the manifest references more DISTINCT basenames than the
/// anti-DoS bound allows — the probe/promotion work is then never started.
fn exceeds_media_file_bound(raw_refs: &[RawMediaRef]) -> bool {
    let distinct: BTreeSet<&str> = raw_refs.iter().map(|r| r.basename.as_str()).collect();
    distinct.len() > MAX_FOLDER_MEDIA_FILES
}

/// A raw media reference as written in the manifest (possibly non-sober).
struct RawMediaRef {
    node_id: String,
    kind: FolderMediaKind,
    basename: String,
}

/// Extract every (node, slot, basename) media reference from the raw JSON,
/// tolerantly: nodes that are not objects or ids that are not strings are
/// skipped here (the structural extraction reports them; this walk only
/// feeds the media probe list and the media findings).
fn raw_media_refs(root: &Value) -> Vec<RawMediaRef> {
    let mut refs = Vec::new();
    let Some(nodes) = root.get("nodes").and_then(Value::as_array) else {
        return refs;
    };
    for node in nodes {
        let Some(obj) = node.as_object() else {
            continue;
        };
        let Some(id) = obj.get("id").and_then(Value::as_str) else {
            continue;
        };
        for (key, kind) in [
            ("image", FolderMediaKind::Image),
            ("audio", FolderMediaKind::Audio),
        ] {
            if let Some(basename) = obj.get(key).and_then(Value::as_str) {
                // An EMPTY string is an author's "no media here" — treated
                // exactly like the already-tolerated `null` (never pushed
                // as a reference, never reported as discarded: the wire
                // must never carry an empty basename the guards refuse).
                if basename.is_empty() {
                    continue;
                }
                refs.push(RawMediaRef {
                    node_id: id.to_string(),
                    kind,
                    basename: basename.to_string(),
                });
            }
        }
    }
    refs
}

const ROOT_KEYS: [&str; 4] = ["formatVersion", "title", "startNodeId", "nodes"];
const NODE_KEYS: [&str; 6] = ["id", "text", "label", "image", "audio", "options"];
const OPTION_KEYS: [&str; 2] = ["label", "target"];

/// The tolerant structural extraction of the manifest: what could be
/// transcoded, plus the issues found on the way.
struct Extraction {
    format_version_ok: bool,
    /// The manifest title verbatim (`""` when absent or not a string — the
    /// title finding blocks either way through the canonical validation).
    title: String,
    title_present: bool,
    /// At least one unknown field anywhere (root / node / option).
    unknown_field: bool,
    /// The structure is untranscodable or breaks a bound — a real block.
    structure_blocked: bool,
    nodes: Vec<CanonicalNode>,
    start_node_id: Option<String>,
}

fn extract(root: &Value) -> Extraction {
    // A non-object root is treated as an empty object: every required field
    // is then absent and blocks mechanically (formatVersion, title, nodes).
    let empty = serde_json::Map::new();
    let obj = root.as_object().unwrap_or(&empty);
    let mut unknown_field = obj.keys().any(|k| !ROOT_KEYS.contains(&k.as_str()));
    let mut structure_blocked = !root.is_object();

    let format_version_ok =
        obj.get("formatVersion").and_then(Value::as_u64) == Some(STRUCTURED_FOLDER_FORMAT_VERSION);

    let title_value = obj.get("title");
    let title_present = matches!(title_value, Some(Value::String(_)));
    let title = title_value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let mut nodes: Vec<CanonicalNode> = Vec::new();
    match obj.get("nodes").and_then(Value::as_array) {
        None => structure_blocked = true,
        Some(raw_nodes) if raw_nodes.is_empty() => structure_blocked = true,
        Some(raw_nodes) => {
            for raw in raw_nodes {
                let Some(node_obj) = raw.as_object() else {
                    structure_blocked = true;
                    continue;
                };
                if node_obj.keys().any(|k| !NODE_KEYS.contains(&k.as_str())) {
                    unknown_field = true;
                }
                let Some(id) = node_obj.get("id").and_then(Value::as_str) else {
                    structure_blocked = true;
                    continue;
                };
                let text = match optional_string(node_obj.get("text")) {
                    Ok(value) => value,
                    Err(()) => {
                        structure_blocked = true;
                        String::new()
                    }
                };
                let label = match optional_string(node_obj.get("label")) {
                    Ok(value) => value,
                    Err(()) => {
                        structure_blocked = true;
                        String::new()
                    }
                };
                if text.chars().count() > MAX_FOLDER_NODE_TEXT_CHARS
                    || label.chars().count() > MAX_FOLDER_NODE_LABEL_CHARS
                {
                    structure_blocked = true;
                }
                // Media slots: a non-string value is untranscodable; the
                // basenames themselves are the media walk's concern.
                for key in ["image", "audio"] {
                    if matches!(node_obj.get(key), Some(v) if !v.is_string() && !v.is_null()) {
                        structure_blocked = true;
                    }
                }
                let mut options = Vec::new();
                match node_obj.get("options") {
                    None | Some(Value::Null) => {}
                    Some(Value::Array(raw_options)) => {
                        for raw_option in raw_options {
                            let Some(option_obj) = raw_option.as_object() else {
                                structure_blocked = true;
                                continue;
                            };
                            if option_obj
                                .keys()
                                .any(|k| !OPTION_KEYS.contains(&k.as_str()))
                            {
                                unknown_field = true;
                            }
                            let Some(option_label) =
                                option_obj.get("label").and_then(Value::as_str)
                            else {
                                structure_blocked = true;
                                continue;
                            };
                            let target = match option_obj.get("target") {
                                None | Some(Value::Null) => None,
                                Some(Value::String(target)) => Some(target.clone()),
                                Some(_) => {
                                    structure_blocked = true;
                                    None
                                }
                            };
                            options.push(CanonicalOption {
                                label: option_label.to_string(),
                                target,
                            });
                        }
                    }
                    Some(_) => structure_blocked = true,
                }
                nodes.push(CanonicalNode {
                    id: id.to_string(),
                    text,
                    label,
                    image_asset_id: None,
                    audio_asset_id: None,
                    options,
                });
            }
        }
    }

    let start_node_id = match obj.get("startNodeId") {
        // An explicit null is tolerated as "absent" (author format).
        None | Some(Value::Null) => nodes.first().map(|n| n.id.clone()),
        Some(Value::String(declared)) => Some(declared.clone()),
        Some(_) => {
            structure_blocked = true;
            None
        }
    };

    Extraction {
        format_version_ok,
        title,
        title_present,
        unknown_field,
        structure_blocked,
        nodes,
        start_node_id,
    }
}

fn optional_string(value: Option<&Value>) -> Result<String, ()> {
    match value {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(()),
    }
}

/// Analyze a structured folder from its manifest bytes + the media probes
/// (all I/O done by the caller — see [`referenced_media`] for what to
/// probe). Pure and deterministic. Produces exactly one finding per aspect
/// of the folder matrix (`Envelope`, `FormatVersion`, `Title`, `Structure`,
/// `Media`) when the manifest parses; a malformed manifest is the single
/// `Envelope` blocking verdict (calque of the `.rustory` flow).
pub fn analyze_structured_folder_components(
    manifest_bytes: &[u8],
    probes: &BTreeMap<String, MediaProbe>,
) -> StructuredFolderAnalysis {
    let Ok(root) = serde_json::from_slice::<Value>(manifest_bytes) else {
        return StructuredFolderAnalysis::envelope_blocked();
    };

    let extraction = extract(&root);
    let mut findings = vec![RecognitionFinding::recognized(RecognitionAspect::Envelope)];

    findings.push(if extraction.format_version_ok {
        RecognitionFinding::recognized(RecognitionAspect::FormatVersion)
    } else {
        RecognitionFinding::blocking(RecognitionAspect::FormatVersion)
    });

    // Media walk: decide each REFERENCE (a probed file may match one slot
    // and not another — same basename referenced as image AND audio), and
    // collect the retained refs + the discarded basenames. The whole media
    // aspect is evaluated ONLY when the declared format is the LISTED one:
    // an unlisted format is a `FormatVersion` block whose media were never
    // read (AC2 — no implicit / partial support), so its verdict carries no
    // `Media` finding at all.
    let mut media_missing = false;
    let mut media_ambiguous = false;
    let mut retained_media = Vec::new();
    let mut discarded_media = Vec::new();
    let mut discarded_seen = BTreeSet::new();
    let mut retained_sizes: BTreeMap<&str, u64> = BTreeMap::new();
    let mut structure_blocked = extraction.structure_blocked;
    let raw_refs = raw_media_refs(&root);
    // More distinct referenced files than the anti-DoS bound: a Structure
    // block decided WITHOUT evaluating a single media (mirrors
    // `referenced_media`, which exposed nothing to probe) — no `Media`
    // finding may be asserted for work that was never done.
    let media_bound_exceeded = exceeds_media_file_bound(&raw_refs);
    if media_bound_exceeded {
        structure_blocked = true;
    }
    let media_analyzed = extraction.format_version_ok && !media_bound_exceeded;
    if media_analyzed {
        for reference in &raw_refs {
            if !is_sober_media_basename(&reference.basename) {
                media_ambiguous = true;
                if discarded_seen.insert(reference.basename.clone()) {
                    discarded_media.push(reference.basename.clone());
                }
                continue;
            }
            // A sober basename missing from the probe map is a caller bug —
            // fail safe as "absent" (the media is discarded, never invented).
            let probe = probes
                .get(&reference.basename)
                .copied()
                .unwrap_or(MediaProbe::Absent);
            match probe {
                MediaProbe::Usable { kind, byte_size } if kind == reference.kind => {
                    retained_sizes.insert(reference.basename.as_str(), byte_size);
                    retained_media.push(RetainedMediaRef {
                        node_id: reference.node_id.clone(),
                        kind: reference.kind,
                        basename: reference.basename.clone(),
                    });
                }
                MediaProbe::Usable { .. } => {
                    // Wrong slot (e.g. a PNG referenced as audio): discarded.
                    media_ambiguous = true;
                    if discarded_seen.insert(reference.basename.clone()) {
                        discarded_media.push(reference.basename.clone());
                    }
                }
                MediaProbe::Absent => {
                    media_missing = true;
                    if discarded_seen.insert(reference.basename.clone()) {
                        discarded_media.push(reference.basename.clone());
                    }
                }
                MediaProbe::Unusable => {
                    media_ambiguous = true;
                    if discarded_seen.insert(reference.basename.clone()) {
                        discarded_media.push(reference.basename.clone());
                    }
                }
            }
        }

        // Anti-DoS total bound (Structure blocking, never a transport
        // error): the sum of the retained media byte sizes.
        if retained_sizes.values().sum::<u64>() > MAX_FOLDER_TOTAL_MEDIA_BYTES {
            structure_blocked = true;
        }
    }

    // Canonical oracle over the transcoded structure — the same
    // `validate_canonical` a transfer and the editor run. Only meaningful
    // when the extraction produced a transcodable shape.
    //
    // The TITLE is validated INDEPENDENTLY of the structural state: a
    // blocked structure must never mask an invalid title (the AC1 report
    // names EVERYTHING that is wrong, not just the first blocker found).
    let mut broken_option_link = false;
    let mut title_blocked =
        !extraction.title_present || validate_title(&normalize_title(&extraction.title)).is_err();
    let structure = CanonicalStructure {
        schema_version: CANONICAL_STORY_SCHEMA_VERSION,
        start_node_id: extraction.start_node_id.clone().unwrap_or_default(),
        nodes: extraction.nodes.clone(),
    };
    if !structure_blocked {
        let structure_json = canonical_structure_json(&structure);
        let facts = CanonicalStoryFacts {
            title: normalize_title(&extraction.title),
            schema_version: CANONICAL_STORY_SCHEMA_VERSION,
            structure_json: structure_json.clone(),
            content_checksum: content_checksum(&structure_json),
        };
        for blocker in validate_canonical(&facts) {
            match blocker.cause {
                CanonicalCause::BrokenOptionLink => broken_option_link = true,
                CanonicalCause::TitleInvalid => title_blocked = true,
                // Schema/checksum causes are unreachable by construction
                // (constant version, checksum computed from the same
                // bytes); any other cause is a structural block.
                _ => structure_blocked = true,
            }
        }
    }

    findings.push(if title_blocked {
        RecognitionFinding::blocking(RecognitionAspect::Title)
    } else if extraction.title != normalize_title(&extraction.title) {
        RecognitionFinding::ambiguous(RecognitionAspect::Title)
    } else {
        RecognitionFinding::recognized(RecognitionAspect::Title)
    });

    findings.push(if structure_blocked {
        RecognitionFinding::blocking(RecognitionAspect::Structure)
    } else if extraction.unknown_field || broken_option_link {
        RecognitionFinding::ambiguous(RecognitionAspect::Structure)
    } else {
        RecognitionFinding::recognized(RecognitionAspect::Structure)
    });

    // The `Media` aspect exists on the verdict ONLY when it was actually
    // analyzed (listed format, in-bounds reference count) — never asserted
    // for work that was never done.
    if media_analyzed {
        findings.push(if media_missing {
            RecognitionFinding {
                aspect: RecognitionAspect::Media,
                category: RecognitionCategory::Missing,
            }
        } else if media_ambiguous {
            RecognitionFinding::ambiguous(RecognitionAspect::Media)
        } else {
            RecognitionFinding::recognized(RecognitionAspect::Media)
        });
    }

    let quality = recognition_quality(&findings);
    let state = folder_import_state(&findings);
    let creatable = if quality == RecognitionQuality::Unusable {
        None
    } else {
        Some(CreatableStory {
            title: extraction.title,
            structure,
            retained_media,
        })
    };

    StructuredFolderAnalysis {
        findings,
        quality,
        state,
        creatable,
        discarded_media,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(manifest: &str, probes: &[(&str, MediaProbe)]) -> StructuredFolderAnalysis {
        let map: BTreeMap<String, MediaProbe> = probes
            .iter()
            .map(|(name, probe)| (name.to_string(), *probe))
            .collect();
        analyze_structured_folder_components(manifest.as_bytes(), &map)
    }

    fn category_of(
        analysis: &StructuredFolderAnalysis,
        aspect: RecognitionAspect,
    ) -> RecognitionCategory {
        analysis
            .findings
            .iter()
            .find(|f| f.aspect == aspect)
            .unwrap_or_else(|| panic!("a finding must exist for {aspect:?}"))
            .category
    }

    const CLEAN_MANIFEST: &str = r#"{
        "formatVersion": 1,
        "title": "Le voyage de Nour",
        "startNodeId": "debut",
        "nodes": [
            {
                "id": "debut",
                "text": "Il était une fois…",
                "label": "Départ",
                "options": [
                    { "label": "Aller à la mer", "target": "mer" },
                    { "label": "Attendre", "target": null }
                ]
            },
            { "id": "mer", "text": "…", "options": [] }
        ]
    }"#;

    fn usable_image() -> MediaProbe {
        MediaProbe::Usable {
            kind: FolderMediaKind::Image,
            byte_size: 1024,
        }
    }

    fn usable_audio() -> MediaProbe {
        MediaProbe::Usable {
            kind: FolderMediaKind::Audio,
            byte_size: 2048,
        }
    }

    // ---- Clean path -------------------------------------------------------

    #[test]
    fn a_clean_manifest_is_recognized_and_creatable() {
        let analysis = analyze(CLEAN_MANIFEST, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        assert_eq!(analysis.state, ImportState::Recognized);
        assert!(analysis
            .findings
            .iter()
            .all(|f| f.category == RecognitionCategory::Recognized));
        let creatable = analysis.creatable.expect("clean is creatable");
        assert_eq!(creatable.title, "Le voyage de Nour");
        assert_eq!(creatable.structure.schema_version, 3);
        assert_eq!(creatable.structure.start_node_id, "debut");
        assert_eq!(creatable.structure.nodes.len(), 2);
        assert_eq!(creatable.structure.nodes[0].options.len(), 2);
        assert!(creatable.retained_media.is_empty());
        assert!(analysis.discarded_media.is_empty());
    }

    #[test]
    fn produces_exactly_one_finding_per_folder_aspect() {
        let analysis = analyze(CLEAN_MANIFEST, &[]);
        for aspect in [
            RecognitionAspect::Envelope,
            RecognitionAspect::FormatVersion,
            RecognitionAspect::Title,
            RecognitionAspect::Structure,
            RecognitionAspect::Media,
        ] {
            let count = analysis
                .findings
                .iter()
                .filter(|f| f.aspect == aspect)
                .count();
            assert_eq!(count, 1, "exactly one finding for {aspect:?}");
        }
        assert_eq!(analysis.findings.len(), 5);
    }

    #[test]
    fn start_node_id_defaults_to_the_first_node() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Sans départ déclaré",
            "nodes": [
                { "id": "premier", "text": "…" },
                { "id": "second" }
            ]
        }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(creatable.structure.start_node_id, "premier");
    }

    #[test]
    fn optional_node_fields_default_cleanly() {
        // text / label / options absent → empty defaults, no finding.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Minimal",
            "nodes": [ { "id": "seul" } ]
        }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(creatable.structure.nodes[0].text, "");
        assert_eq!(creatable.structure.nodes[0].label, "");
        assert!(creatable.structure.nodes[0].options.is_empty());
    }

    // ---- Envelope ---------------------------------------------------------

    #[test]
    fn malformed_json_is_the_single_envelope_blocked_verdict() {
        let analysis = analyze("{ this is not json", &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(analysis.state, ImportState::Blocked);
        assert_eq!(analysis.findings.len(), 1);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Envelope),
            RecognitionCategory::Blocking
        );
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn a_non_object_root_blocks_on_the_required_fields() {
        // Valid JSON that is not an object: every required field is absent.
        let analysis = analyze("42", &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Envelope),
            RecognitionCategory::Recognized,
            "the JSON itself parsed"
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::FormatVersion),
            RecognitionCategory::Blocking
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Blocking
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
    }

    // ---- FormatVersion ----------------------------------------------------

    #[test]
    fn rejects_a_missing_format_version() {
        let manifest = r#"{ "title": "Sans version", "nodes": [ { "id": "n1" } ] }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::FormatVersion),
            RecognitionCategory::Blocking
        );
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn rejects_format_version_zero_and_a_future_one() {
        // The exact calque of `rejects_deserialization_of_format_version_zero`:
        // the forward/backward guard blocks at the ANALYSIS level.
        for version in ["0", "2"] {
            let manifest = format!(
                r#"{{ "formatVersion": {version}, "title": "V", "nodes": [ {{ "id": "n1" }} ] }}"#
            );
            let analysis = analyze(&manifest, &[]);
            assert_eq!(analysis.quality, RecognitionQuality::Unusable);
            assert_eq!(
                category_of(&analysis, RecognitionAspect::FormatVersion),
                RecognitionCategory::Blocking,
                "formatVersion {version} must block"
            );
        }
    }

    #[test]
    fn rejects_a_non_integer_format_version() {
        let manifest = r#"{ "formatVersion": "1", "title": "V", "nodes": [ { "id": "n1" } ] }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::FormatVersion),
            RecognitionCategory::Blocking,
            "a string formatVersion is not the declared integer contract"
        );
    }

    // ---- Title ------------------------------------------------------------

    #[test]
    fn a_missing_title_blocks() {
        let manifest = r#"{ "formatVersion": 1, "nodes": [ { "id": "n1" } ] }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn a_blank_title_blocks() {
        let manifest = r#"{ "formatVersion": 1, "title": "   ", "nodes": [ { "id": "n1" } ] }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn a_normalizable_title_is_ambiguous_but_creatable() {
        let manifest =
            r#"{ "formatVersion": 1, "title": "  Espaces  ", "nodes": [ { "id": "n1" } ] }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Partial);
        assert_eq!(analysis.state, ImportState::NeedsReview);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Ambiguous
        );
        // The creatable carries the VERBATIM title; storage normalizes.
        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(creatable.title, "  Espaces  ");
    }

    // ---- Structure --------------------------------------------------------

    #[test]
    fn missing_or_empty_nodes_block() {
        for manifest in [
            r#"{ "formatVersion": 1, "title": "T" }"#,
            r#"{ "formatVersion": 1, "title": "T", "nodes": [] }"#,
            r#"{ "formatVersion": 1, "title": "T", "nodes": "not-a-list" }"#,
        ] {
            let analysis = analyze(manifest, &[]);
            assert_eq!(analysis.quality, RecognitionQuality::Unusable);
            assert_eq!(
                category_of(&analysis, RecognitionAspect::Structure),
                RecognitionCategory::Blocking,
                "manifest {manifest} must block on structure"
            );
        }
    }

    #[test]
    fn a_duplicate_node_id_blocks() {
        // The UX `dupliqué`: a Blocking Structure finding through the
        // canonical oracle (DuplicateNodeId), never a category of its own.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Doublon",
            "nodes": [ { "id": "n1" }, { "id": "n1" } ]
        }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn a_declared_but_unknown_start_node_blocks() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Départ fantôme",
            "startNodeId": "ghost",
            "nodes": [ { "id": "n1" } ]
        }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn a_missing_node_id_blocks() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Nœud sans id",
            "nodes": [ { "text": "orphelin" } ]
        }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn a_wrongly_typed_known_field_blocks() {
        // The tolerance covers UNKNOWN fields, never a known field carrying
        // an untranscodable type.
        for manifest in [
            r#"{ "formatVersion": 1, "title": "T", "nodes": [ { "id": "n1", "text": 42 } ] }"#,
            r#"{ "formatVersion": 1, "title": "T", "nodes": [ { "id": "n1", "image": 7 } ] }"#,
            r#"{ "formatVersion": 1, "title": "T", "nodes": [ { "id": "n1", "options": 3 } ] }"#,
            r#"{ "formatVersion": 1, "title": "T", "nodes": [ { "id": "n1", "options": [ { "target": "x" } ] } ] }"#,
            r#"{ "formatVersion": 1, "title": "T", "startNodeId": 9, "nodes": [ { "id": "n1" } ] }"#,
        ] {
            let analysis = analyze(manifest, &[]);
            assert_eq!(
                category_of(&analysis, RecognitionAspect::Structure),
                RecognitionCategory::Blocking,
                "manifest {manifest} must block on structure"
            );
        }
    }

    #[test]
    fn an_unknown_field_is_ambiguous_never_a_rejection() {
        // The DELIBERATE difference with the `.rustory` machine artifact:
        // an author typo is flagged, not punished.
        for manifest in [
            r#"{ "formatVersion": 1, "title": "T", "nodes": [ { "id": "n1" } ], "surprise": 1 }"#,
            r#"{ "formatVersion": 1, "title": "T", "nodes": [ { "id": "n1", "imge": "typo.png" } ] }"#,
            r#"{ "formatVersion": 1, "title": "T", "nodes": [ { "id": "n1", "options": [ { "label": "Go", "targt": "n1" } ] } ] }"#,
        ] {
            let analysis = analyze(manifest, &[]);
            assert_eq!(analysis.quality, RecognitionQuality::Partial, "{manifest}");
            assert_eq!(analysis.state, ImportState::NeedsReview);
            assert_eq!(
                category_of(&analysis, RecognitionAspect::Structure),
                RecognitionCategory::Ambiguous
            );
            assert!(analysis.creatable.is_some(), "still creatable");
        }
    }

    #[test]
    fn a_broken_option_link_is_ambiguous_and_preserved() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Lien cassé",
            "nodes": [
                { "id": "n1", "options": [ { "label": "Perdu", "target": "ghost" } ] }
            ]
        }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Partial);
        assert_eq!(analysis.state, ImportState::NeedsReview);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Ambiguous
        );
        // The broken link is PRESERVED (repairable in the editor).
        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(
            creatable.structure.nodes[0].options[0].target.as_deref(),
            Some("ghost")
        );
    }

    #[test]
    fn an_oversize_node_text_blocks_as_an_anti_dos_bound() {
        let big = "x".repeat(MAX_FOLDER_NODE_TEXT_CHARS + 1);
        let manifest = format!(
            r#"{{ "formatVersion": 1, "title": "T", "nodes": [ {{ "id": "n1", "text": "{big}" }} ] }}"#
        );
        let analysis = analyze(&manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn folder_text_bounds_mirror_the_editor_write_bounds() {
        // The manifest must not create what the editor would refuse to save.
        assert_eq!(
            MAX_FOLDER_NODE_TEXT_CHARS,
            crate::application::story::node::MAX_NODE_TEXT_CHARS
        );
        assert_eq!(
            MAX_FOLDER_NODE_LABEL_CHARS,
            crate::application::story::node::MAX_NODE_LABEL_CHARS
        );
    }

    #[test]
    fn too_many_distinct_media_references_block_without_a_single_probe() {
        let refs: Vec<String> = (0..=MAX_FOLDER_MEDIA_FILES)
            .map(|i| format!(r#"{{ "id": "n{i}", "image": "img{i}.png" }}"#))
            .collect();
        let manifest = format!(
            r#"{{ "formatVersion": 1, "title": "Trop", "nodes": [ {} ] }}"#,
            refs.join(",")
        );
        let analysis = analyze(&manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking,
            "more than MAX_FOLDER_MEDIA_FILES distinct files is an anti-DoS block"
        );
        // The bound short-circuits BEFORE any media work: nothing exposed
        // to probe (zero I/O possible), no `Media` finding asserted, no
        // discarded list built from unprobed references.
        assert!(
            referenced_media(manifest.as_bytes()).is_empty(),
            "a bounds-breaking manifest exposes nothing to probe"
        );
        assert!(
            !analysis
                .findings
                .iter()
                .any(|f| f.aspect == RecognitionAspect::Media),
            "no Media finding may be asserted for unprobed references"
        );
        assert!(analysis.discarded_media.is_empty());
    }

    #[test]
    fn an_empty_media_reference_means_no_media_like_null() {
        // An author's `"image": ""` is "no media here" — the exact calque
        // of the tolerated `null`: no reference, no finding, no discarded
        // entry (the wire must never carry an empty basename).
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Sans image déclarée",
            "nodes": [ { "id": "n1", "image": "", "audio": "" } ]
        }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        assert_eq!(analysis.state, ImportState::Recognized);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Recognized
        );
        assert!(analysis.discarded_media.is_empty());
        let creatable = analysis.creatable.expect("creatable");
        assert!(creatable.retained_media.is_empty());
        assert!(referenced_media(manifest.as_bytes()).is_empty());
    }

    #[test]
    fn an_oversize_media_total_blocks() {
        let half = MAX_FOLDER_TOTAL_MEDIA_BYTES / 2 + 1;
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Total",
            "nodes": [
                { "id": "n1", "image": "a.png" },
                { "id": "n2", "image": "b.png" }
            ]
        }"#;
        let probe = MediaProbe::Usable {
            kind: FolderMediaKind::Image,
            byte_size: half,
        };
        let analysis = analyze(manifest, &[("a.png", probe), ("b.png", probe)]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking,
            "the retained total beyond MAX_FOLDER_TOTAL_MEDIA_BYTES is an anti-DoS block"
        );
    }

    // ---- Media ------------------------------------------------------------

    #[test]
    fn usable_referenced_media_are_retained_and_recognized() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Médias",
            "nodes": [
                { "id": "n1", "image": "couverture.png", "audio": "intro.mp3" }
            ]
        }"#;
        let analysis = analyze(
            manifest,
            &[
                ("couverture.png", usable_image()),
                ("intro.mp3", usable_audio()),
            ],
        );
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Recognized
        );
        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(creatable.retained_media.len(), 2);
        assert_eq!(creatable.retained_media[0].basename, "couverture.png");
        assert_eq!(creatable.retained_media[0].kind, FolderMediaKind::Image);
        assert_eq!(creatable.retained_media[1].basename, "intro.mp3");
        assert_eq!(creatable.retained_media[1].kind, FolderMediaKind::Audio);
    }

    #[test]
    fn an_absent_referenced_media_is_missing_and_the_state_is_partial() {
        // THE positive twin of the `.rustory` negative test: the folder
        // flow DOES emit `Missing` and the durable `Partial` state.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Média manquant",
            "nodes": [ { "id": "n1", "image": "fantome.png" } ]
        }"#;
        let analysis = analyze(manifest, &[("fantome.png", MediaProbe::Absent)]);
        assert_eq!(analysis.quality, RecognitionQuality::Partial);
        assert_eq!(analysis.state, ImportState::Partial);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Missing
        );
        // The story is still creatable — the slot is born empty.
        let creatable = analysis.creatable.expect("creatable");
        assert!(creatable.retained_media.is_empty());
        assert_eq!(analysis.discarded_media, vec!["fantome.png".to_string()]);
    }

    #[test]
    fn an_unusable_media_is_ambiguous_needs_review_and_discarded() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Média illisible",
            "nodes": [ { "id": "n1", "audio": "casse.mp3" } ]
        }"#;
        let analysis = analyze(manifest, &[("casse.mp3", MediaProbe::Unusable)]);
        assert_eq!(analysis.quality, RecognitionQuality::Partial);
        assert_eq!(analysis.state, ImportState::NeedsReview);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Ambiguous
        );
        assert_eq!(analysis.discarded_media, vec!["casse.mp3".to_string()]);
    }

    #[test]
    fn a_wrong_slot_media_is_ambiguous_and_discarded() {
        // A PNG referenced as audio: the file is fine, the REFERENCE is not.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Mauvais emplacement",
            "nodes": [ { "id": "n1", "audio": "image.png" } ]
        }"#;
        let analysis = analyze(manifest, &[("image.png", usable_image())]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Ambiguous
        );
        let creatable = analysis.creatable.expect("creatable");
        assert!(creatable.retained_media.is_empty());
    }

    #[test]
    fn the_same_file_can_serve_one_slot_and_be_discarded_on_the_other() {
        // "a.png" referenced as image (retained) AND as audio (wrong slot,
        // discarded): the decision is PER REFERENCE, the probe per file.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Double usage",
            "nodes": [
                { "id": "n1", "image": "a.png" },
                { "id": "n2", "audio": "a.png" }
            ]
        }"#;
        let analysis = analyze(manifest, &[("a.png", usable_image())]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Ambiguous
        );
        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(creatable.retained_media.len(), 1);
        assert_eq!(creatable.retained_media[0].node_id, "n1");
        assert_eq!(creatable.retained_media[0].kind, FolderMediaKind::Image);
    }

    #[test]
    fn a_non_sober_media_basename_is_ambiguous_and_never_probed() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Traversée",
            "nodes": [ { "id": "n1", "image": "../evil.png" } ]
        }"#;
        // No probe provided on purpose: the name must never reach the
        // probe list (referenced_media excludes it).
        let analysis = analyze(manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Ambiguous
        );
        assert_eq!(analysis.discarded_media, vec!["../evil.png".to_string()]);
        assert!(referenced_media(manifest.as_bytes()).is_empty());
    }

    #[test]
    fn missing_dominates_ambiguous_on_the_media_finding() {
        // One absent + one unusable: the single Media finding names the
        // MISSING content (drives the `partial` durable state).
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Mixte",
            "nodes": [
                { "id": "n1", "image": "absente.png", "audio": "cassee.mp3" }
            ]
        }"#;
        let analysis = analyze(
            manifest,
            &[
                ("absente.png", MediaProbe::Absent),
                ("cassee.mp3", MediaProbe::Unusable),
            ],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Missing
        );
        assert_eq!(analysis.state, ImportState::Partial);
        assert_eq!(analysis.discarded_media.len(), 2);
    }

    #[test]
    fn a_discarded_media_never_prevents_the_creation() {
        // AC1's "à corriger": the node is born with the empty slot.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Créable quand même",
            "nodes": [ { "id": "n1", "image": "absente.png", "text": "Contenu" } ]
        }"#;
        let analysis = analyze(manifest, &[("absente.png", MediaProbe::Absent)]);
        let creatable = analysis.creatable.expect("a missing media never blocks");
        assert!(creatable.structure.nodes[0].image_asset_id.is_none());
        assert_eq!(creatable.structure.nodes[0].text, "Contenu");
    }

    // ---- referenced_media ---------------------------------------------------

    #[test]
    fn referenced_media_returns_distinct_sober_basenames_in_order() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Refs",
            "nodes": [
                { "id": "n1", "image": "b.png", "audio": "a.mp3" },
                { "id": "n2", "image": "b.png", "audio": "../evil" }
            ]
        }"#;
        assert_eq!(
            referenced_media(manifest.as_bytes()),
            vec!["b.png".to_string(), "a.mp3".to_string()]
        );
    }

    #[test]
    fn referenced_media_is_empty_on_an_unreadable_manifest() {
        assert!(referenced_media(b"not json").is_empty());
        assert!(referenced_media(b"{}").is_empty());
    }

    #[test]
    fn an_unlisted_format_never_exposes_its_media_references() {
        // AC2: the media of an UNLISTED format are never probed (no I/O)
        // and never asserted — the verdict blocks on FormatVersion and
        // carries NO Media finding at all.
        let manifest = r#"{
            "formatVersion": 2,
            "title": "Futur",
            "nodes": [ { "id": "n1", "image": "jamais-lue.png" } ]
        }"#;
        assert!(
            referenced_media(manifest.as_bytes()).is_empty(),
            "an unlisted format exposes nothing to probe"
        );
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::FormatVersion),
            RecognitionCategory::Blocking
        );
        assert!(
            !analysis
                .findings
                .iter()
                .any(|f| f.aspect == RecognitionAspect::Media),
            "no Media finding may be asserted for an unanalyzed aspect"
        );
        assert!(analysis.discarded_media.is_empty());
    }

    #[test]
    fn a_blocked_structure_never_masks_an_invalid_title() {
        // The AC1 report names EVERYTHING to fix: a blank title must block
        // even when the structure is already blocking on its own.
        let manifest = r#"{ "formatVersion": 1, "title": "   ", "nodes": [] }"#;
        let analysis = analyze(manifest, &[]);
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Blocking,
            "the blank title must be reported alongside the blocked structure"
        );
    }

    // ---- Sober names ---------------------------------------------------------

    #[test]
    fn folder_source_name_accepts_sober_names_without_extension() {
        assert!(is_supported_folder_source_name("mon-dossier"));
        assert!(is_supported_folder_source_name("Histoire de Nour"));
        assert!(is_supported_folder_source_name("dossier.v1"));
    }

    #[test]
    fn folder_source_name_refuses_paths_and_pii() {
        assert!(!is_supported_folder_source_name(""));
        assert!(!is_supported_folder_source_name("/home/user/dossier"));
        assert!(!is_supported_folder_source_name("dir\\dossier"));
        assert!(!is_supported_folder_source_name("."));
        assert!(!is_supported_folder_source_name(".."));
        assert!(!is_supported_folder_source_name("nul\0"));
        assert!(!is_supported_folder_source_name(&"a".repeat(300)));
    }

    #[test]
    fn media_basename_sobriety_mirrors_the_same_rules() {
        assert!(is_sober_media_basename("couverture.png"));
        assert!(is_sober_media_basename("intro"));
        assert!(!is_sober_media_basename(""));
        assert!(!is_sober_media_basename("../evil.png"));
        assert!(!is_sober_media_basename("a/b.png"));
        assert!(!is_sober_media_basename(".."));
    }

    #[test]
    fn sobriety_refuses_drive_relative_blank_and_control_names() {
        // `c:evil.png` joins as a drive-relative path OUTSIDE the chosen
        // folder on Windows (and `:` is illegal in Windows names anyway).
        assert!(!is_sober_media_basename("c:evil.png"));
        assert!(!is_supported_folder_source_name("c:dossier"));
        // Blank-after-trim names would render as dangling report lines.
        assert!(!is_sober_media_basename(" "));
        assert!(!is_sober_media_basename("   "));
        assert!(!is_supported_folder_source_name("  "));
        // Control characters (newline, tab, NUL) never belong to a sober
        // provenance / report name.
        assert!(!is_sober_media_basename("a\nb.png"));
        assert!(!is_sober_media_basename("a\tb"));
        assert!(!is_supported_folder_source_name("nul\0"));
        // Inner spaces stay fine (a normal author name).
        assert!(is_sober_media_basename("ma couverture.png"));
    }

    // ---- The positive twin locking the folder emitters ---------------------

    #[test]
    fn the_folder_flow_emits_missing_and_partial_unlike_the_rustory_flow() {
        // The `.rustory` negative test (`never_emits_declared_but_unsupported_
        // categories_or_states`) keeps its guarantee for ITS flow; this
        // positive twin proves the folder flow is the real emitter.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Jumeau positif",
            "nodes": [ { "id": "n1", "image": "absente.png" } ]
        }"#;
        let analysis = analyze(manifest, &[("absente.png", MediaProbe::Absent)]);
        assert!(analysis
            .findings
            .iter()
            .any(|f| f.category == RecognitionCategory::Missing));
        assert_eq!(analysis.state, ImportState::Partial);
    }
}
