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
/// bounds the probe/promotion work). Exceeding it is a `MediaCount` blocking
/// finding — a typed verdict that names the number, never a transport error
/// — reached on the references alone: no media is read. An honest long
/// story reaches it (33 nodes with a picture and a sound each), which is
/// why its refusal says what it is.
pub const MAX_FOLDER_MEDIA_FILES: usize = 64;

/// Ceiling on ONE media file of a structured folder: the local-attach
/// ceiling of the node-media store — the bound of a media added by hand in
/// the editor — re-declared here because the domain never imports the
/// infrastructure layer (the folder flow asserts the equality at compile
/// time). A media over it is present but left out: a `MediaSize` ambiguity.
pub const MAX_FOLDER_MEDIA_BYTES: u64 = 32 * 1024 * 1024;

/// Ceiling on the SUM of the retained media byte sizes (anti-DoS: bounds
/// the total acceptance I/O). Exceeding it is a `MediaSize` blocking
/// finding — a typed verdict that names the size, never a transport error.
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
    /// Present and regular, but over the per-file bound: never loaded, and
    /// discarded for its SIZE (`MediaSize`). `kind` is what its magic bytes
    /// say (`None`: outside the closed set) — only the header is read, so
    /// the `Media` aspect still judges its format and its slot like any
    /// other file's: a file both too large and of an unknown format is
    /// reported for both.
    Oversize { kind: Option<FolderMediaKind> },
    /// Present but unusable: symlink / irregular file / bytes outside the
    /// closed format set → `Ambiguous` (discarded).
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
    /// Distinct referenced basenames discarded by the analysis (absent,
    /// unusable or over the per-file bound), in first-reference order.
    /// Feeds the analysis surface's summary; the persisted findings stay
    /// aggregated pairs.
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
    if exceeds_media_file_bound(&raw_refs, MAX_FOLDER_MEDIA_FILES) {
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

/// True iff `references` name more DISTINCT basenames than `max_files`, the
/// reference bound of their source — the probe/promotion work is then never
/// started. ONE count for the folder and the archive flows, and for the two
/// places of each that must agree on it: the list of what to probe and the
/// verdict. A file referenced by several nodes counts once.
pub(super) fn exceeds_media_file_bound(references: &[MediaReference], max_files: usize) -> bool {
    let distinct: BTreeSet<&str> = references.iter().map(|r| r.basename.as_str()).collect();
    distinct.len() > max_files
}

/// A media reference as written in an author manifest or a pack descriptor:
/// which node, which slot, which basename — possibly non-sober (the walk
/// discards such a name without ever probing it).
pub(super) struct MediaReference {
    pub(super) node_id: String,
    pub(super) kind: FolderMediaKind,
    pub(super) basename: String,
}

/// What walking the media references of a source decided: the references
/// the creation keeps, the basenames it leaves out, and why. ONE walk for
/// the folder and the archive flows, so the two can never disagree on what
/// a probe means.
pub(super) struct MediaWalk {
    /// At least one referenced media is absent from the source.
    pub(super) missing: bool,
    /// At least one referenced media is there but unusable: bytes outside
    /// the closed format set, wrong slot, irregular file, non-sober name.
    pub(super) ambiguous: bool,
    /// At least one referenced media is there but over the per-file bound.
    pub(super) oversize: bool,
    /// The references kept, in reference order.
    pub(super) retained: Vec<RetainedMediaRef>,
    /// The DISTINCT basenames left out, in first-reference order.
    pub(super) discarded: Vec<String>,
    /// Sum of the byte sizes of the DISTINCT retained basenames.
    pub(super) retained_bytes: u64,
}

impl MediaWalk {
    /// The single `Media` finding of a verdict: a missing media dominates an
    /// unusable one (it drives the durable `partial` state).
    pub(super) fn media_finding(&self) -> RecognitionFinding {
        if self.missing {
            RecognitionFinding {
                aspect: RecognitionAspect::Media,
                category: RecognitionCategory::Missing,
                message: None,
            }
        } else if self.ambiguous {
            RecognitionFinding::ambiguous(RecognitionAspect::Media)
        } else {
            RecognitionFinding::recognized(RecognitionAspect::Media)
        }
    }

    /// The single `MediaSize` finding of a verdict, against the source's
    /// aggregate bound: the retained media together over it block (nothing
    /// is created — and that dominates); else a media left out for its own
    /// size is an ambiguity; else the sizes are recognized.
    pub(super) fn media_size_finding(&self, max_total_bytes: u64) -> RecognitionFinding {
        if self.retained_bytes > max_total_bytes {
            RecognitionFinding::blocking(RecognitionAspect::MediaSize)
        } else if self.oversize {
            RecognitionFinding::ambiguous(RecognitionAspect::MediaSize)
        } else {
            RecognitionFinding::recognized(RecognitionAspect::MediaSize)
        }
    }
}

/// Decide each media REFERENCE against the probe of its file (a probed file
/// may match one slot and not another — the same basename referenced as
/// image AND audio). A sober basename missing from the probe map is a caller
/// bug — it fails safe as "absent" (the media is discarded, never invented).
///
/// Size and format are judged INDEPENDENTLY: a file over the per-file bound
/// is left out for its size, and its format and slot are still held against
/// it when they are wrong — neither reason hides the other.
fn walk_media_references(
    references: &[MediaReference],
    probes: &BTreeMap<String, MediaProbe>,
) -> MediaWalk {
    let mut walk = MediaWalk {
        missing: false,
        ambiguous: false,
        oversize: false,
        retained: Vec::new(),
        discarded: Vec::new(),
        retained_bytes: 0,
    };
    let mut discarded_seen = BTreeSet::new();
    let mut retained_sizes: BTreeMap<&str, u64> = BTreeMap::new();
    for reference in references {
        let probe = if is_sober_media_basename(&reference.basename) {
            probes
                .get(&reference.basename)
                .copied()
                .unwrap_or(MediaProbe::Absent)
        } else {
            // A non-sober name is never probed: discarded as unusable.
            MediaProbe::Unusable
        };
        match probe {
            MediaProbe::Usable { kind, byte_size } if kind == reference.kind => {
                retained_sizes.insert(reference.basename.as_str(), byte_size);
                walk.retained.push(RetainedMediaRef {
                    node_id: reference.node_id.clone(),
                    kind: reference.kind,
                    basename: reference.basename.clone(),
                });
                continue;
            }
            // Wrong slot (e.g. a PNG referenced as audio), or unusable.
            MediaProbe::Usable { .. } | MediaProbe::Unusable => walk.ambiguous = true,
            MediaProbe::Absent => walk.missing = true,
            MediaProbe::Oversize { kind } => {
                walk.oversize = true;
                if kind != Some(reference.kind) {
                    walk.ambiguous = true;
                }
            }
        }
        if discarded_seen.insert(reference.basename.as_str()) {
            walk.discarded.push(reference.basename.clone());
        }
    }
    walk.retained_bytes = retained_sizes.values().sum();
    walk
}

/// What the media references of a source come to once held against its
/// reference bound. ONE judgement for the folder and the archive flows, so
/// the two can never disagree on what crossing the bound means.
pub(super) enum MediaJudgement {
    /// More DISTINCT referenced files than the bound — decided on the
    /// references alone. No media is read, so the verdict says nothing of
    /// their presence, of their format or of their size: only that they are
    /// too many.
    TooMany,
    /// Within the bound: each reference decided against its probe.
    Walked(MediaWalk),
}

impl MediaJudgement {
    /// The media findings of a verdict, against the source's aggregate byte
    /// bound: the single `MediaCount` block of a source that references too
    /// many media, else the `Media` and `MediaSize` findings of the media
    /// that were read. Never both — no finding is asserted for work that
    /// was never done.
    pub(super) fn findings(&self, max_total_bytes: u64) -> Vec<RecognitionFinding> {
        match self {
            Self::TooMany => vec![RecognitionFinding::blocking(RecognitionAspect::MediaCount)],
            Self::Walked(walk) => vec![
                walk.media_finding(),
                walk.media_size_finding(max_total_bytes),
            ],
        }
    }

    /// The references the creation keeps and the basenames it leaves out —
    /// none of either when the media were never read.
    pub(super) fn into_retained_and_discarded(self) -> (Vec<RetainedMediaRef>, Vec<String>) {
        match self {
            Self::TooMany => (Vec::new(), Vec::new()),
            Self::Walked(walk) => (walk.retained, walk.discarded),
        }
    }
}

/// Judge the media references of a source: COUNT them against `max_files`
/// first — beyond it nothing is walked (the caller was handed nothing to
/// probe either) —, else decide each one against its probe.
pub(super) fn judge_media_references(
    references: &[MediaReference],
    probes: &BTreeMap<String, MediaProbe>,
    max_files: usize,
) -> MediaJudgement {
    if exceeds_media_file_bound(references, max_files) {
        MediaJudgement::TooMany
    } else {
        MediaJudgement::Walked(walk_media_references(references, probes))
    }
}

/// Extract every (node, slot, basename) media reference from the raw JSON,
/// tolerantly: nodes that are not objects or ids that are not strings are
/// skipped here (the structural extraction reports them; this walk only
/// feeds the media probe list and the media findings).
fn raw_media_refs(root: &Value) -> Vec<MediaReference> {
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
                refs.push(MediaReference {
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
/// probe). Pure and deterministic. Produces exactly one finding per JUDGED
/// aspect of the folder matrix: `Envelope`, `FormatVersion`, `Title` and
/// `Structure` whenever the manifest parses, then — for the listed format
/// only — `Media` and `MediaSize` when the media were read, or the single
/// `MediaCount` block when the manifest references more of them than its
/// bound. A malformed manifest is the single `Envelope` blocking verdict
/// (calque of the `.rustory` flow).
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

    // Media: judged ONLY when the declared format is the LISTED one — an
    // unlisted format is a `FormatVersion` block whose media references are
    // never evaluated, not even counted (no implicit / partial support), so
    // its verdict carries no media finding at all. For the listed format
    // the references are first COUNTED against the reference bound: beyond
    // it the verdict blocks on `MediaCount` without a single media read
    // (mirrors `referenced_media`, which exposed nothing to probe); within
    // it each REFERENCE is decided against its probe. Either way the
    // structure is left alone, and judged below for what it is.
    let media = extraction
        .format_version_ok
        .then(|| judge_media_references(&raw_media_refs(&root), probes, MAX_FOLDER_MEDIA_FILES));

    // Canonical oracle over the transcoded structure — the same
    // `validate_canonical` a transfer and the editor run. Only meaningful
    // when the extraction produced a transcodable shape.
    //
    // The TITLE is validated INDEPENDENTLY of the structural state: a
    // blocked structure must never mask an invalid title (the AC1 report
    // names EVERYTHING that is wrong, not just the first blocker found).
    let mut broken_option_link = false;
    let mut structure_blocked = extraction.structure_blocked;
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

    // The media aspects exist on the verdict ONLY for what was actually
    // judged — never asserted for work that was never done: `Media` and
    // `MediaSize` when the media were read, the `MediaCount` block alone
    // when they were too many to be. Both bounds are verdicts of their own
    // (`MediaSize` blocking for the aggregate bytes, `MediaCount` blocking
    // for the number of references): the structure they leave alone says
    // what it is.
    if let Some(media) = &media {
        findings.extend(media.findings(MAX_FOLDER_TOTAL_MEDIA_BYTES));
    }
    let (retained_media, discarded_media) = media
        .map(MediaJudgement::into_retained_and_discarded)
        .unwrap_or_default();

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
            RecognitionAspect::MediaSize,
        ] {
            let count = analysis
                .findings
                .iter()
                .filter(|f| f.aspect == aspect)
                .count();
            assert_eq!(count, 1, "exactly one finding for {aspect:?}");
        }
        assert_eq!(analysis.findings.len(), 6);
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

    // ---- Media count -------------------------------------------------------

    /// A manifest of `nodes` nodes, each with its OWN picture.
    fn manifest_of_pictures(format_version: u64, nodes: usize) -> String {
        let refs: Vec<String> = (0..nodes)
            .map(|i| format!(r#"{{ "id": "n{i}", "image": "img{i}.png" }}"#))
            .collect();
        format!(
            r#"{{ "formatVersion": {format_version}, "title": "Trop", "nodes": [ {} ] }}"#,
            refs.join(",")
        )
    }

    fn judges_no_media(analysis: &StructuredFolderAnalysis) -> bool {
        !analysis.findings.iter().any(|f| {
            matches!(
                f.aspect,
                RecognitionAspect::Media | RecognitionAspect::MediaSize
            )
        })
    }

    #[test]
    fn too_many_distinct_media_references_block_on_their_count_without_a_single_probe() {
        let manifest = manifest_of_pictures(1, MAX_FOLDER_MEDIA_FILES + 1);
        let analysis = analyze(&manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaCount),
            RecognitionCategory::Blocking,
            "more than MAX_FOLDER_MEDIA_FILES distinct files block on their NUMBER"
        );
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(analysis.state, ImportState::Blocked);
        assert!(analysis.creatable.is_none());
        // The count is the ONLY blocker: the manifest and its structure are
        // sound, and their own lines say so.
        for aspect in [
            RecognitionAspect::Envelope,
            RecognitionAspect::FormatVersion,
            RecognitionAspect::Title,
            RecognitionAspect::Structure,
        ] {
            assert_eq!(
                category_of(&analysis, aspect),
                RecognitionCategory::Recognized,
                "{aspect:?} is not blamed for a count reason"
            );
        }
        assert_eq!(analysis.findings.len(), 5);
        // The bound short-circuits BEFORE any media work: nothing exposed
        // to probe (zero I/O possible), no `Media` nor `MediaSize` finding
        // asserted, no discarded list built from unprobed references.
        assert!(
            referenced_media(manifest.as_bytes()).is_empty(),
            "a bounds-breaking manifest exposes nothing to probe"
        );
        assert!(
            judges_no_media(&analysis),
            "no media finding may be asserted for unprobed references"
        );
        assert!(analysis.discarded_media.is_empty());
    }

    #[test]
    fn the_media_count_bound_is_inclusive_and_says_nothing_within_it() {
        // Exactly MAX_FOLDER_MEDIA_FILES distinct files: the media are read
        // and judged, and the count — which has nothing to report — gets no
        // line at all.
        let manifest = manifest_of_pictures(1, MAX_FOLDER_MEDIA_FILES);
        let names: Vec<String> = (0..MAX_FOLDER_MEDIA_FILES)
            .map(|i| format!("img{i}.png"))
            .collect();
        assert_eq!(referenced_media(manifest.as_bytes()), names);
        let probes: Vec<(&str, MediaProbe)> = names
            .iter()
            .map(|name| (name.as_str(), usable_image()))
            .collect();
        let analysis = analyze(&manifest, &probes);
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        assert!(!analysis
            .findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::MediaCount));
        assert_eq!(analysis.findings.len(), 6);
        let creatable = analysis.creatable.expect("creatable at the bound");
        assert_eq!(creatable.retained_media.len(), MAX_FOLDER_MEDIA_FILES);
    }

    #[test]
    fn an_honest_long_story_reaches_the_media_count_bound() {
        // 33 nodes with a picture and a sound each: 66 distinct files for a
        // bound of 64. Nothing is wrong with the story — the refusal is a
        // matter of NUMBER, and that is the line that blocks.
        let nodes: Vec<String> = (0..33)
            .map(|i| {
                format!(r#"{{ "id": "n{i}", "image": "image-{i}.png", "audio": "son-{i}.mp3" }}"#)
            })
            .collect();
        let manifest = format!(
            r#"{{ "formatVersion": 1, "title": "Une longue histoire", "nodes": [ {} ] }}"#,
            nodes.join(",")
        );
        let analysis = analyze(&manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaCount),
            RecognitionCategory::Blocking
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Recognized
        );
    }

    #[test]
    fn a_media_referenced_by_several_nodes_counts_once_against_the_count_bound() {
        // One shared picture on every node, one sound of its own each: 63
        // nodes fill 126 slots with only 64 distinct files — the folder is
        // creatable; one node more makes 65 files, one too many.
        let manifest_of = |nodes: usize| {
            let refs: Vec<String> = (0..nodes)
                .map(|i| {
                    format!(r#"{{ "id": "n{i}", "image": "commune.png", "audio": "son-{i}.mp3" }}"#)
                })
                .collect();
            format!(
                r#"{{ "formatVersion": 1, "title": "Une image pour tous", "nodes": [ {} ] }}"#,
                refs.join(",")
            )
        };
        let within = manifest_of(MAX_FOLDER_MEDIA_FILES - 1);
        assert_eq!(
            referenced_media(within.as_bytes()).len(),
            MAX_FOLDER_MEDIA_FILES
        );
        let mut probes: Vec<(String, MediaProbe)> = (0..MAX_FOLDER_MEDIA_FILES - 1)
            .map(|i| (format!("son-{i}.mp3"), usable_audio()))
            .collect();
        probes.push(("commune.png".to_string(), usable_image()));
        let probe_refs: Vec<(&str, MediaProbe)> = probes
            .iter()
            .map(|(name, probe)| (name.as_str(), *probe))
            .collect();
        let analysis = analyze(&within, &probe_refs);
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        assert!(analysis.creatable.is_some());

        let beyond = analyze(&manifest_of(MAX_FOLDER_MEDIA_FILES), &[]);
        assert_eq!(
            category_of(&beyond, RecognitionAspect::MediaCount),
            RecognitionCategory::Blocking
        );
    }

    #[test]
    fn the_media_count_never_hides_what_else_is_wrong() {
        // The report names EVERYTHING to fix: a blank title and a broken
        // structure block on their own lines, next to the count.
        let refs: Vec<String> = (0..=MAX_FOLDER_MEDIA_FILES)
            .map(|i| format!(r#"{{ "id": "n{i}", "image": "img{i}.png" }}"#))
            .collect();
        let manifest = format!(
            r#"{{ "formatVersion": 1, "title": "   ", "startNodeId": "ghost", "nodes": [ {} ] }}"#,
            refs.join(",")
        );
        let analysis = analyze(&manifest, &[]);
        for aspect in [
            RecognitionAspect::Title,
            RecognitionAspect::Structure,
            RecognitionAspect::MediaCount,
        ] {
            assert_eq!(
                category_of(&analysis, aspect),
                RecognitionCategory::Blocking,
                "{aspect:?} must block on its own line"
            );
        }
        assert!(judges_no_media(&analysis));
    }

    #[test]
    fn the_references_of_an_unlisted_format_are_not_counted_either() {
        // No implicit / partial support: the media references of an
        // UNLISTED format are never evaluated — not probed, and not counted.
        // The verdict blocks on `FormatVersion`, and neither the count nor
        // the structure is blamed for references Rustory has no business
        // interpreting.
        let manifest = manifest_of_pictures(2, MAX_FOLDER_MEDIA_FILES + 1);
        assert!(referenced_media(manifest.as_bytes()).is_empty());
        let analysis = analyze(&manifest, &[]);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::FormatVersion),
            RecognitionCategory::Blocking
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Recognized
        );
        assert!(!analysis
            .findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::MediaCount));
        assert!(judges_no_media(&analysis));
    }

    // ---- Media size --------------------------------------------------------

    const TWO_IMAGES_MANIFEST: &str = r#"{
        "formatVersion": 1,
        "title": "Total",
        "nodes": [
            { "id": "n1", "image": "a.png" },
            { "id": "n2", "image": "b.png" }
        ]
    }"#;

    fn image_of(byte_size: u64) -> MediaProbe {
        MediaProbe::Usable {
            kind: FolderMediaKind::Image,
            byte_size,
        }
    }

    #[test]
    fn media_over_the_aggregate_bound_block_on_their_size_not_on_the_structure() {
        // The retained total beyond MAX_FOLDER_TOTAL_MEDIA_BYTES blocks —
        // and the blocked aspect is the SIZE of the media: the structure
        // and the media themselves are sound and say so.
        let half = MAX_FOLDER_TOTAL_MEDIA_BYTES / 2 + 1;
        let analysis = analyze(
            TWO_IMAGES_MANIFEST,
            &[("a.png", image_of(half)), ("b.png", image_of(half))],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Blocking
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Recognized
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Recognized
        );
        assert_eq!(analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(analysis.state, ImportState::Blocked);
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn the_aggregate_media_bound_is_inclusive() {
        let half = MAX_FOLDER_TOTAL_MEDIA_BYTES / 2;
        let analysis = analyze(
            TWO_IMAGES_MANIFEST,
            &[("a.png", image_of(half)), ("b.png", image_of(half))],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Recognized
        );
        assert_eq!(analysis.quality, RecognitionQuality::Clean);
        assert!(analysis.creatable.is_some());
    }

    #[test]
    fn a_media_referenced_twice_counts_once_in_the_aggregate() {
        // The SAME file on two nodes is one file to promote: it weighs once.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Deux fois",
            "nodes": [
                { "id": "n1", "image": "a.png" },
                { "id": "n2", "image": "a.png" }
            ]
        }"#;
        let analysis = analyze(
            manifest,
            &[("a.png", image_of(MAX_FOLDER_TOTAL_MEDIA_BYTES))],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Recognized
        );
    }

    #[test]
    fn a_media_over_the_per_file_bound_is_left_out_for_its_size_alone() {
        // The file is THERE and its format is fine: the `Media` aspect has
        // nothing to say, the `MediaSize` aspect names the only reason. An
        // ambiguity (the review is pending) — never a missing media.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Un long son",
            "nodes": [ { "id": "n1", "image": "couverture.png", "audio": "long.wav" } ]
        }"#;
        let analysis = analyze(
            manifest,
            &[
                ("couverture.png", usable_image()),
                (
                    "long.wav",
                    MediaProbe::Oversize {
                        kind: Some(FolderMediaKind::Audio),
                    },
                ),
            ],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Ambiguous
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Recognized
        );
        assert_eq!(analysis.quality, RecognitionQuality::Partial);
        assert_eq!(analysis.state, ImportState::NeedsReview);
        assert_eq!(analysis.discarded_media, vec!["long.wav".to_string()]);
        let creatable = analysis.creatable.expect("created without the long sound");
        assert_eq!(creatable.retained_media.len(), 1);
        assert_eq!(creatable.retained_media[0].basename, "couverture.png");
    }

    #[test]
    fn an_oversize_media_of_an_unknown_format_is_reported_for_both_reasons() {
        // Lightening the file would not be enough: its format is not one
        // Rustory reads either, and the report says so in the same pass.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Trop lourd et inconnu",
            "nodes": [ { "id": "n1", "audio": "long.flac" } ]
        }"#;
        let analysis = analyze(
            manifest,
            &[("long.flac", MediaProbe::Oversize { kind: None })],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Ambiguous
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Ambiguous
        );
        assert_eq!(analysis.discarded_media, vec!["long.flac".to_string()]);
    }

    #[test]
    fn an_oversize_media_in_the_wrong_slot_is_reported_for_both_reasons() {
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Trop lourd et mal placé",
            "nodes": [ { "id": "n1", "audio": "affiche.png" } ]
        }"#;
        let analysis = analyze(
            manifest,
            &[(
                "affiche.png",
                MediaProbe::Oversize {
                    kind: Some(FolderMediaKind::Image),
                },
            )],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Ambiguous
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Ambiguous
        );
    }

    #[test]
    fn a_missing_media_and_an_oversize_one_are_each_named() {
        // Two files left out for two different reasons: one finding each —
        // the size never hides behind the missing file, nor the reverse.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Absent et trop lourd",
            "nodes": [ { "id": "n1", "image": "absente.png", "audio": "long.wav" } ]
        }"#;
        let analysis = analyze(
            manifest,
            &[
                ("absente.png", MediaProbe::Absent),
                (
                    "long.wav",
                    MediaProbe::Oversize {
                        kind: Some(FolderMediaKind::Audio),
                    },
                ),
            ],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Missing
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Ambiguous
        );
        // The missing media still drives the durable state.
        assert_eq!(analysis.state, ImportState::Partial);
        assert_eq!(analysis.discarded_media.len(), 2);
    }

    #[test]
    fn the_aggregate_block_dominates_a_media_left_out_for_its_size() {
        // One finding per aspect: when the retained media block together,
        // that is what the `MediaSize` line says (nothing is created).
        let half = MAX_FOLDER_TOTAL_MEDIA_BYTES / 2 + 1;
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Tout est trop lourd",
            "nodes": [
                { "id": "n1", "image": "a.png", "audio": "long.wav" },
                { "id": "n2", "image": "b.png" }
            ]
        }"#;
        let analysis = analyze(
            manifest,
            &[
                ("a.png", image_of(half)),
                ("b.png", image_of(half)),
                (
                    "long.wav",
                    MediaProbe::Oversize {
                        kind: Some(FolderMediaKind::Audio),
                    },
                ),
            ],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Blocking
        );
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn a_media_left_out_for_its_size_never_weighs_in_the_aggregate() {
        // Only RETAINED media are summed: a file left out is never promoted.
        let manifest = r#"{
            "formatVersion": 1,
            "title": "Écarté, donc non compté",
            "nodes": [ { "id": "n1", "image": "a.png", "audio": "long.wav" } ]
        }"#;
        let analysis = analyze(
            manifest,
            &[
                ("a.png", image_of(MAX_FOLDER_TOTAL_MEDIA_BYTES)),
                (
                    "long.wav",
                    MediaProbe::Oversize {
                        kind: Some(FolderMediaKind::Audio),
                    },
                ),
            ],
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::MediaSize),
            RecognitionCategory::Ambiguous
        );
        assert!(analysis.creatable.is_some());
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
            !analysis.findings.iter().any(|f| matches!(
                f.aspect,
                RecognitionAspect::Media | RecognitionAspect::MediaSize
            )),
            "no media finding may be asserted for an unanalyzed aspect"
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
