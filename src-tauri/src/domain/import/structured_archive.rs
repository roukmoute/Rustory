//! Structured-archive (.zip pack) domain analysis — pure, no I/O.
//!
//! The structured archive is the COMMUNITY pack format (a `story.json`
//! stage/action graph plus an `assets/` directory inside a zip): a FOREIGN
//! format, parsed TOLERANTLY through `serde_json::Value` exactly like the
//! author manifest — an unknown field never rejects. The analysis produces
//! the SAME verdict model as the structured folder
//! ([`StructuredFolderAnalysis`]) so the review surface, the report
//! serialization and the acceptance machinery are shared, not cloned.
//!
//! Mapping to the canonical structure: every stage node becomes a canonical
//! node (its uuid is the node id, its enriched `name` the label — the
//! format carries no narrative text); the stage's `okTransition` action
//! node contributes the node's options (one per action target, labeled by
//! the target's name); `squareOne` designates the start node (first stage
//! as fallback). Every media probe is an INPUT from the application layer,
//! so the whole matrix is testable without a disk or a zip.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::domain::story::{
    canonical_structure_json, content_checksum, normalize_title, validate_canonical,
    validate_title, CanonicalCause, CanonicalNode, CanonicalOption, CanonicalStoryFacts,
    CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION,
};

use super::recognition::{
    folder_import_state, recognition_quality, RecognitionAspect, RecognitionCategory,
    RecognitionFinding, RecognitionQuality,
};
use super::structured_folder::{
    is_sober_media_basename, CreatableStory, FolderMediaKind, MediaProbe, RetainedMediaRef,
    StructuredFolderAnalysis, MAX_FOLDER_MEDIA_FILES, MAX_FOLDER_NODE_LABEL_CHARS,
    MAX_FOLDER_TOTAL_MEDIA_BYTES,
};

/// The exact descriptor entry name inside the archive — ONE listed name,
/// no alias (the folder-manifest discipline).
pub const STRUCTURED_ARCHIVE_STORY_JSON_NAME: &str = "story.json";

/// The assets directory prefix inside the archive. `story.json` references
/// its media by bare file name; the bytes live under this prefix.
pub const STRUCTURED_ARCHIVE_ASSETS_PREFIX: &str = "assets/";

/// Revision of OUR structured-archive reader support (the pack format
/// itself declares no format version — this is the provenance row's
/// `source_format_version`, never a value read from the foreign file).
pub const STRUCTURED_ARCHIVE_FORMAT_VERSION: u64 = 1;

/// One media reference extracted from the stage graph: which stage (the
/// future canonical node), which slot, which bare basename.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveMediaReference {
    node_id: String,
    kind: FolderMediaKind,
    basename: String,
}

/// The DISTINCT sober basenames referenced by the descriptor with their
/// slot kinds, in first-reference order — exactly what the application
/// must probe inside the archive (and nothing else: a non-sober basename
/// never reaches an entry lookup; the analysis discards it as `Ambiguous`
/// on its own). An unparsable descriptor or one referencing MORE distinct
/// files than [`MAX_FOLDER_MEDIA_FILES`] yields an EMPTY list: a
/// bounds-breaking descriptor never triggers a single entry read.
pub fn archive_referenced_media(story_json: &[u8]) -> Vec<(String, FolderMediaKind)> {
    let Ok(root) = serde_json::from_slice::<Value>(story_json) else {
        return Vec::new();
    };
    let refs = raw_archive_media_refs(&root);
    if exceeds_media_file_bound(&refs) {
        return Vec::new();
    }
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for reference in refs {
        if is_sober_media_basename(&reference.basename) && seen.insert(reference.basename.clone()) {
            ordered.push((reference.basename, reference.kind));
        }
    }
    ordered
}

/// Analyze a structured archive's components: the descriptor bytes
/// (`None` when the entry is absent / oversize / unreadable — an envelope
/// block) and the probe facts of every referenced basename. `fallback_title`
/// is the archive's own sober stem, used when the descriptor carries no
/// title — surfaced as an `Ambiguous` title finding, never silently clean.
pub fn analyze_structured_archive_components(
    story_json: Option<&[u8]>,
    probes: &BTreeMap<String, MediaProbe>,
    fallback_title: Option<&str>,
) -> StructuredFolderAnalysis {
    let Some(bytes) = story_json else {
        return StructuredFolderAnalysis::envelope_blocked();
    };
    let Ok(root) = serde_json::from_slice::<Value>(bytes) else {
        return StructuredFolderAnalysis::envelope_blocked();
    };

    let extraction = extract(&root);
    let mut findings = vec![RecognitionFinding::recognized(RecognitionAspect::Envelope)];

    // Media walk — the exact folder-walk semantics: decide each REFERENCE,
    // collect retained refs + discarded basenames. Evaluated only within
    // the anti-DoS reference bound (a bounds-breaking descriptor is a
    // `Structure` block whose media were never probed — no `Media` finding
    // may be asserted for work that was never done).
    let mut media_missing = false;
    let mut media_ambiguous = false;
    let mut retained_media = Vec::new();
    let mut discarded_media = Vec::new();
    let mut discarded_seen = BTreeSet::new();
    let mut retained_sizes: BTreeMap<&str, u64> = BTreeMap::new();
    let mut structure_blocked = extraction.structure_blocked;
    let raw_refs = raw_archive_media_refs(&root);
    let media_bound_exceeded = exceeds_media_file_bound(&raw_refs);
    if media_bound_exceeded {
        structure_blocked = true;
    }
    let media_analyzed = !media_bound_exceeded;
    if media_analyzed {
        for reference in &raw_refs {
            if !is_sober_media_basename(&reference.basename) {
                media_ambiguous = true;
                if discarded_seen.insert(reference.basename.clone()) {
                    discarded_media.push(reference.basename.clone());
                }
                continue;
            }
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
        if retained_sizes.values().sum::<u64>() > MAX_FOLDER_TOTAL_MEDIA_BYTES {
            structure_blocked = true;
        }
    }

    // Title: the descriptor's own title wins; the archive stem is the
    // HONEST fallback (flagged, never clean). No title at all blocks.
    let (title, title_fallback_used) = match (&extraction.title, fallback_title) {
        (Some(own), _) => (own.clone(), false),
        (None, Some(stem)) if !stem.trim().is_empty() => (stem.to_string(), true),
        (None, _) => (String::new(), false),
    };
    let mut title_blocked = validate_title(&normalize_title(&title)).is_err();

    // Canonical oracle over the transcoded structure — the same
    // `validate_canonical` the editor and a transfer run.
    let mut broken_option_link = false;
    let structure = CanonicalStructure {
        schema_version: CANONICAL_STORY_SCHEMA_VERSION,
        start_node_id: extraction.start_node_id.clone().unwrap_or_default(),
        nodes: extraction.nodes.clone(),
    };
    if !structure_blocked {
        let structure_json = canonical_structure_json(&structure);
        let facts = CanonicalStoryFacts {
            title: normalize_title(&title),
            schema_version: CANONICAL_STORY_SCHEMA_VERSION,
            structure_json: structure_json.clone(),
            content_checksum: content_checksum(&structure_json),
        };
        for blocker in validate_canonical(&facts) {
            match blocker.cause {
                CanonicalCause::BrokenOptionLink => broken_option_link = true,
                CanonicalCause::TitleInvalid => title_blocked = true,
                _ => structure_blocked = true,
            }
        }
    }

    findings.push(if title_blocked {
        RecognitionFinding::blocking(RecognitionAspect::Title)
    } else if title_fallback_used || title != normalize_title(&title) {
        RecognitionFinding::ambiguous(RecognitionAspect::Title)
    } else {
        RecognitionFinding::recognized(RecognitionAspect::Title)
    });

    findings.push(if structure_blocked {
        RecognitionFinding::blocking(RecognitionAspect::Structure)
    } else if extraction.structure_ambiguous || broken_option_link {
        RecognitionFinding::ambiguous(RecognitionAspect::Structure)
    } else {
        RecognitionFinding::recognized(RecognitionAspect::Structure)
    });

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
            title,
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

/// What the tolerant extraction pulled out of the descriptor.
struct ArchiveExtraction {
    title: Option<String>,
    start_node_id: Option<String>,
    nodes: Vec<CanonicalNode>,
    structure_blocked: bool,
    /// Non-blocking oddities (an action reference that resolves nowhere,
    /// a missing action target…) — the structure stays creatable but the
    /// finding is `Ambiguous`, never silently clean.
    structure_ambiguous: bool,
}

fn extract(root: &Value) -> ArchiveExtraction {
    let title = root
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);

    let Some(stages) = root.get("stageNodes").and_then(Value::as_array) else {
        return ArchiveExtraction {
            title,
            start_node_id: None,
            nodes: Vec::new(),
            structure_blocked: true,
            structure_ambiguous: false,
        };
    };
    let Some(actions) = root.get("actionNodes").and_then(Value::as_array) else {
        return ArchiveExtraction {
            title,
            start_node_id: None,
            nodes: Vec::new(),
            structure_blocked: true,
            structure_ambiguous: false,
        };
    };

    // Index the action nodes by id: the option lists the stage graph
    // navigates through. A duplicate id keeps the FIRST definition and
    // flags the structure as ambiguous.
    let mut structure_ambiguous = false;
    let mut actions_by_id: BTreeMap<&str, &Value> = BTreeMap::new();
    for action in actions {
        let Some(id) = action.get("id").and_then(Value::as_str) else {
            structure_ambiguous = true;
            continue;
        };
        if actions_by_id.insert(id, action).is_some() {
            structure_ambiguous = true;
        }
    }

    // Names of every stage, resolved first: option labels point AT the
    // target stage, so labeling needs the full map before the walk.
    let mut stage_names: BTreeMap<&str, &str> = BTreeMap::new();
    for stage in stages {
        if let (Some(uuid), Some(name)) = (
            stage.get("uuid").and_then(Value::as_str),
            stage.get("name").and_then(Value::as_str),
        ) {
            stage_names.entry(uuid).or_insert(name);
        }
    }

    let mut structure_blocked = false;
    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
    let mut nodes: Vec<CanonicalNode> = Vec::new();
    let mut start_node_id: Option<String> = None;

    for stage in stages {
        let Some(uuid) = stage.get("uuid").and_then(Value::as_str) else {
            // A stage without identity cannot become a node — the graph
            // is not transcodable as declared.
            structure_blocked = true;
            continue;
        };
        if !seen_ids.insert(uuid) {
            structure_blocked = true;
            continue;
        }
        if stage.get("squareOne").and_then(Value::as_bool) == Some(true) && start_node_id.is_none()
        {
            start_node_id = Some(uuid.to_string());
        }

        // The stage's options come from its OK transition's action node:
        // one option per action target, labeled by the target's name. A
        // dangling action reference degrades to zero options (ambiguous),
        // a dangling TARGET keeps the link and lets the canonical oracle
        // flag it as repairable — the editor's own semantic.
        let mut options: Vec<CanonicalOption> = Vec::new();
        if let Some(action_ref) = stage
            .get("okTransition")
            .and_then(|t| t.get("actionNode"))
            .and_then(Value::as_str)
        {
            if let Some(action) = actions_by_id.get(action_ref) {
                if let Some(targets) = action.get("options").and_then(Value::as_array) {
                    for target in targets {
                        let Some(target_id) = target.as_str() else {
                            structure_ambiguous = true;
                            continue;
                        };
                        let label = stage_names
                            .get(target_id)
                            .map(|name| truncate_label(name))
                            .unwrap_or_default();
                        options.push(CanonicalOption {
                            label,
                            target: Some(target_id.to_string()),
                        });
                    }
                }
            } else {
                structure_ambiguous = true;
            }
        }

        nodes.push(CanonicalNode {
            id: uuid.to_string(),
            text: String::new(),
            label: stage
                .get("name")
                .and_then(Value::as_str)
                .map(truncate_label)
                .unwrap_or_default(),
            image_asset_id: None,
            audio_asset_id: None,
            options,
        });
    }

    if nodes.is_empty() {
        structure_blocked = true;
    }
    if start_node_id.is_none() {
        // No squareOne declared: the FIRST stage is the entry — the
        // documented community fallback, flagged as ambiguous.
        if let Some(first) = nodes.first() {
            start_node_id = Some(first.id.clone());
            structure_ambiguous = true;
        }
    }

    ArchiveExtraction {
        title,
        start_node_id,
        nodes,
        structure_blocked,
        structure_ambiguous,
    }
}

/// A label is cosmetic metadata: a foreign name longer than the editor's
/// own bound is truncated at a char boundary rather than blocking the
/// whole pack.
fn truncate_label(name: &str) -> String {
    name.chars().take(MAX_FOLDER_NODE_LABEL_CHARS).collect()
}

fn raw_archive_media_refs(root: &Value) -> Vec<ArchiveMediaReference> {
    let mut refs = Vec::new();
    let Some(stages) = root.get("stageNodes").and_then(Value::as_array) else {
        return refs;
    };
    for stage in stages {
        let Some(uuid) = stage.get("uuid").and_then(Value::as_str) else {
            continue;
        };
        for (field, kind) in [
            ("image", FolderMediaKind::Image),
            ("audio", FolderMediaKind::Audio),
        ] {
            if let Some(raw) = stage.get(field).and_then(Value::as_str) {
                // Some packs reference `assets/<name>` instead of the bare
                // name — tolerate the canonical prefix, nothing else.
                let basename = raw
                    .strip_prefix(STRUCTURED_ARCHIVE_ASSETS_PREFIX)
                    .unwrap_or(raw);
                if !basename.is_empty() {
                    refs.push(ArchiveMediaReference {
                        node_id: uuid.to_string(),
                        kind,
                        basename: basename.to_string(),
                    });
                }
            }
        }
    }
    refs
}

fn exceeds_media_file_bound(refs: &[ArchiveMediaReference]) -> bool {
    let distinct: BTreeSet<&str> = refs.iter().map(|r| r.basename.as_str()).collect();
    distinct.len() > MAX_FOLDER_MEDIA_FILES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(
        story_json: &str,
        probes: &[(&str, MediaProbe)],
        fallback_title: Option<&str>,
    ) -> StructuredFolderAnalysis {
        let map: BTreeMap<String, MediaProbe> = probes
            .iter()
            .map(|(name, probe)| (name.to_string(), *probe))
            .collect();
        analyze_structured_archive_components(Some(story_json.as_bytes()), &map, fallback_title)
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

    const CLEAN_PACK: &str = r#"{
        "format": "v1",
        "version": 2,
        "title": "Le voyage de Nour",
        "stageNodes": [
            {
                "uuid": "stage-1",
                "squareOne": true,
                "name": "Départ",
                "image": "cover.png",
                "audio": "intro.mp3",
                "okTransition": { "actionNode": "action-1", "optionIndex": 0 },
                "controlSettings": { "wheel": true, "ok": true, "home": false, "pause": false, "autoplay": false }
            },
            {
                "uuid": "stage-2",
                "name": "La forêt",
                "image": null,
                "audio": "foret.mp3",
                "okTransition": null,
                "controlSettings": { "wheel": false, "ok": false, "home": true, "pause": true, "autoplay": true }
            }
        ],
        "actionNodes": [
            { "id": "action-1", "options": ["stage-2"] }
        ]
    }"#;

    const CLEAN_PROBES: &[(&str, MediaProbe)] = &[
        (
            "cover.png",
            MediaProbe::Usable {
                kind: FolderMediaKind::Image,
                byte_size: 10,
            },
        ),
        (
            "intro.mp3",
            MediaProbe::Usable {
                kind: FolderMediaKind::Audio,
                byte_size: 20,
            },
        ),
        (
            "foret.mp3",
            MediaProbe::Usable {
                kind: FolderMediaKind::Audio,
                byte_size: 30,
            },
        ),
    ];

    #[test]
    fn clean_pack_is_fully_recognized_and_creatable() {
        let analysis = analyze(CLEAN_PACK, CLEAN_PROBES, None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Envelope),
            RecognitionCategory::Recognized
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Recognized
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Recognized
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Recognized
        );

        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(creatable.title, "Le voyage de Nour");
        assert_eq!(creatable.structure.start_node_id, "stage-1");
        assert_eq!(creatable.structure.nodes.len(), 2);
        // stage-1's ok transition resolves action-1 → one option to
        // stage-2, labeled by the TARGET's name.
        let start = &creatable.structure.nodes[0];
        assert_eq!(start.id, "stage-1");
        assert_eq!(start.label, "Départ");
        assert_eq!(start.options.len(), 1);
        assert_eq!(start.options[0].target.as_deref(), Some("stage-2"));
        assert_eq!(start.options[0].label, "La forêt");
        // Three usable references retained, none discarded.
        assert_eq!(creatable.retained_media.len(), 3);
        assert!(analysis.discarded_media.is_empty());
    }

    #[test]
    fn absent_story_json_blocks_the_envelope() {
        let analysis = analyze_structured_archive_components(None, &BTreeMap::new(), Some("pack"));
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Envelope),
            RecognitionCategory::Blocking
        );
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn malformed_json_blocks_the_envelope() {
        let analysis = analyze("{not json", &[], None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Envelope),
            RecognitionCategory::Blocking
        );
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn missing_stage_nodes_blocks_the_structure() {
        let analysis = analyze(r#"{"title": "T", "actionNodes": []}"#, &[], None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn missing_title_falls_back_to_the_archive_stem_as_ambiguous() {
        let pack = r#"{
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": null, "audio": null,
                  "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        let analysis = analyze(pack, &[], Some("Mon pack"));
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Ambiguous
        );
        let creatable = analysis.creatable.expect("creatable with fallback title");
        assert_eq!(creatable.title, "Mon pack");
    }

    #[test]
    fn missing_title_without_fallback_blocks() {
        let pack = r#"{
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": null, "audio": null,
                  "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        let analysis = analyze(pack, &[], None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Title),
            RecognitionCategory::Blocking
        );
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn no_square_one_falls_back_to_the_first_stage_as_ambiguous() {
        let pack = r#"{
            "title": "T",
            "stageNodes": [
                { "uuid": "s1", "image": null, "audio": null, "okTransition": null, "controlSettings": {} },
                { "uuid": "s2", "image": null, "audio": null, "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        let analysis = analyze(pack, &[], None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Ambiguous
        );
        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(creatable.structure.start_node_id, "s1");
    }

    #[test]
    fn dangling_action_reference_is_ambiguous_not_blocking() {
        let pack = r#"{
            "title": "T",
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": null, "audio": null,
                  "okTransition": { "actionNode": "ghost", "optionIndex": 0 }, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        let analysis = analyze(pack, &[], None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Ambiguous
        );
        let creatable = analysis.creatable.expect("creatable");
        assert!(creatable.structure.nodes[0].options.is_empty());
    }

    #[test]
    fn duplicate_stage_uuid_blocks_the_structure() {
        let pack = r#"{
            "title": "T",
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": null, "audio": null, "okTransition": null, "controlSettings": {} },
                { "uuid": "s1", "image": null, "audio": null, "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        let analysis = analyze(pack, &[], None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Structure),
            RecognitionCategory::Blocking
        );
        assert!(analysis.creatable.is_none());
    }

    #[test]
    fn unsupported_media_bytes_are_discarded_and_flagged_never_blocking() {
        // The community's BMP images sniff outside the closed set: the
        // reference is discarded (report line), the pack stays creatable.
        let pack = r#"{
            "title": "T",
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": "cover.bmp", "audio": "voix.mp3",
                  "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        let analysis = analyze(
            pack,
            &[
                ("cover.bmp", MediaProbe::Unusable),
                (
                    "voix.mp3",
                    MediaProbe::Usable {
                        kind: FolderMediaKind::Audio,
                        byte_size: 5,
                    },
                ),
            ],
            None,
        );
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Ambiguous
        );
        let creatable = analysis.creatable.expect("creatable");
        assert_eq!(creatable.retained_media.len(), 1);
        assert_eq!(creatable.retained_media[0].basename, "voix.mp3");
        assert_eq!(analysis.discarded_media, vec!["cover.bmp".to_string()]);
    }

    #[test]
    fn absent_media_is_missing_and_the_pack_stays_creatable() {
        let pack = r#"{
            "title": "T",
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": "gone.png", "audio": null,
                  "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        let analysis = analyze(pack, &[("gone.png", MediaProbe::Absent)], None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Missing
        );
        assert!(analysis.creatable.is_some());
        assert_eq!(analysis.discarded_media, vec!["gone.png".to_string()]);
    }

    #[test]
    fn assets_prefix_in_references_is_tolerated() {
        let pack = r#"{
            "title": "T",
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": "assets/cover.png", "audio": null,
                  "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        let refs = archive_referenced_media(pack.as_bytes());
        assert_eq!(
            refs,
            vec![("cover.png".to_string(), FolderMediaKind::Image)]
        );
    }

    #[test]
    fn traversal_smuggling_basenames_never_reach_the_probe_list() {
        let pack = r#"{
            "title": "T",
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": "../evil.png", "audio": "sous/dossier.mp3",
                  "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        assert!(archive_referenced_media(pack.as_bytes()).is_empty());
        // The analysis discards them as ambiguous instead of probing.
        let analysis = analyze(pack, &[], None);
        assert_eq!(
            category_of(&analysis, RecognitionAspect::Media),
            RecognitionCategory::Ambiguous
        );
        assert_eq!(analysis.discarded_media.len(), 2);
    }
}
