use serde::{Deserialize, Serialize};

use crate::domain::import::{
    ArtifactAnalysis, ContentSourceActivation, ContentSourceKind, ContentSourceLine, ImportState,
    ImportableContent, RecognitionAspect, RecognitionCategory, RecognitionFinding,
    RecognitionQuality, RssItemRef, StructuredFolderAnalysis,
};
use crate::domain::story::normalize_title;

/// Input accepted by the `export_story_with_save_dialog` Tauri command.
/// `deny_unknown_fields` fails the deserialization if the UI ever adds a
/// field ahead of the Rust contract, so the boundary stays authoritative.
///
/// `suggested_filename` is the default text pre-filled in the native save
/// dialog (typically `{sanitizedTitle}.rustory`). The frontend never
/// constructs the actual destination path — the dialog returns it, and
/// Rust validates it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportStoryDialogInputDto {
    pub story_id: String,
    pub suggested_filename: String,
}

/// Tagged outcome returned by `export_story_with_save_dialog`.
///
/// A cancelled dialog is NOT an error — the command resolves with
/// `{ kind: "cancelled" }` so the UI can silently return to idle.
/// Errors (file-system denied, story missing, I/O failure, dialog
/// backend failure) cross the boundary as [`AppError`] rejections.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExportStoryDialogOutcomeDto {
    Exported {
        #[serde(rename = "destinationPath")]
        destination_path: String,
        #[serde(rename = "bytesWritten")]
        bytes_written: u64,
        #[serde(rename = "contentChecksum")]
        content_checksum: String,
    },
    Cancelled,
}

// ===== Local artifact import (`.rustory` file → library) =====

/// Recognition quality of an analyzed local artifact. Mirror of the domain
/// [`RecognitionQuality`] (UI: `Propre` / `Partiellement exploitable` /
/// `Inexploitable`).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportQualityDto {
    Clean,
    Partial,
    Unusable,
}

/// Durable per-story import state. Mirror of the domain [`ImportState`].
/// `recognized` / `partial` / `needsReview` are persisted at import time;
/// `resolved` is persisted by the write-path review resolution (a card chip
/// is never rendered for it); `blocked` is never imported nor persisted.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportStateDto {
    Recognized,
    Partial,
    NeedsReview,
    Blocked,
    Resolved,
}

impl ImportStateDto {
    /// The camelCase wire value as a plain string, for DTOs that carry the
    /// state as an `Option<String>` (`importState` on the story detail and
    /// the write acknowledgements). Must stay byte-identical to the serde
    /// rename above.
    pub fn wire_tag(self) -> &'static str {
        match self {
            ImportStateDto::Recognized => "recognized",
            ImportStateDto::Partial => "partial",
            ImportStateDto::NeedsReview => "needsReview",
            ImportStateDto::Blocked => "blocked",
            ImportStateDto::Resolved => "resolved",
        }
    }
}

/// The aspect of the analyzed input a finding refers to. Mirror of the
/// domain [`RecognitionAspect`] (`media` belongs to the structured-folder
/// and RSS flows, `source` to the RSS ingestion flow only).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportAspectDto {
    Envelope,
    FormatVersion,
    SchemaVersion,
    Structure,
    Integrity,
    Title,
    Timestamps,
    Media,
    Source,
}

/// The recognition category of a finding. Mirror of the domain
/// [`RecognitionCategory`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportCategoryDto {
    Recognized,
    Ambiguous,
    Missing,
    Blocking,
}

/// A single recognition finding surfaced in the analysis report: a closed
/// `(aspect, category)` pair plus the canonical FR `message`. The message
/// is Rust-authoritative and rendered verbatim — the UI branches on
/// `aspect` / `category`, never on the text.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportFindingDto {
    pub aspect: ImportAspectDto,
    pub category: ImportCategoryDto,
    pub message: String,
}

/// The validated canonical content carried from the analyze phase to the
/// accept phase. The frontend round-trips it verbatim; `accept` NEVER
/// trusts it — it re-validates every field from zero before committing
/// (the canonical schema version is the supported constant, re-proven
/// against the `structureJson`'s embedded `schemaVersion`, so it is not
/// carried on the wire). `deny_unknown_fields` keeps the boundary
/// authoritative on the way in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportableContentDto {
    pub title: String,
    pub structure_json: String,
    pub content_checksum: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Tagged outcome of `analyze_artifact_for_import`: either the typed
/// recognition verdict (`analyzed`) or a cancelled dialog (`cancelled`). A
/// TRANSPORT failure rejects with `AppError` instead — the functional
/// verdict is NEVER an error.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportArtifactAnalysisDto {
    #[serde(rename_all = "camelCase")]
    Analyzed {
        quality: ImportQualityDto,
        state: ImportStateDto,
        findings: Vec<ImportFindingDto>,
        /// The validated canonical content — present iff importable
        /// (`quality != unusable`). `None` ⇒ blocked (only `Abandonner`).
        #[serde(skip_serializing_if = "Option::is_none")]
        importable_content: Option<ImportableContentDto>,
        source_name: String,
        artifact_checksum: String,
    },
    Cancelled,
}

/// Input accepted by the `accept_artifact_import` Tauri command: the
/// validated content from a prior analysis, plus the provenance metadata.
/// `deny_unknown_fields` fails deserialization if the UI drifts ahead of
/// the contract.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptArtifactImportInputDto {
    pub content: ImportableContentDto,
    pub source_name: String,
    pub artifact_checksum: String,
}

// ===== Structured-folder creation (folder → new canonical story) =====

/// The creatable-content summary carried by an `analyzed` folder verdict:
/// what WILL be created if accepted — the (normalized) title, the node
/// count, the retained media and the discarded ones (by basename). The
/// per-file detail lives HERE only; the persisted findings stay aggregated
/// `(aspect, category)` pairs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreatableSummaryDto {
    pub title: String,
    pub node_count: u32,
    pub retained_media: Vec<String>,
    pub discarded_media: Vec<String>,
}

/// Tagged outcome of `analyze_structured_folder_for_creation`: either the
/// typed recognition verdict (`analyzed`) or a cancelled dialog
/// (`cancelled`). A TRANSPORT failure rejects with `AppError` instead —
/// the functional verdict is NEVER an error.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StructuredCreationAnalysisDto {
    #[serde(rename_all = "camelCase")]
    Analyzed {
        quality: ImportQualityDto,
        state: ImportStateDto,
        findings: Vec<ImportFindingDto>,
        /// Present iff creatable (`quality != unusable`). `None` ⇒ blocked
        /// (only `Abandonner`).
        #[serde(skip_serializing_if = "Option::is_none")]
        creatable_summary: Option<CreatableSummaryDto>,
        /// The folder's basename — the only name the surface renders.
        folder_name: String,
        /// The absolute path returned by the SYSTEM dialog, carried ONLY to
        /// be passed back to `accept_structured_creation`. NEVER rendered,
        /// NEVER persisted, NEVER logged (PII) — the accept phase grants it
        /// no authority (it re-analyzes the disk from zero).
        folder_path: String,
    },
    Cancelled,
}

impl StructuredCreationAnalysisDto {
    /// Map a domain folder analysis + the dialog facts to the `analyzed`
    /// wire verdict, with the FOLDER per-pair FR copy.
    pub fn analyzed(
        analysis: &StructuredFolderAnalysis,
        folder_name: String,
        folder_path: String,
    ) -> Self {
        Self::Analyzed {
            quality: quality_dto(analysis.quality),
            state: state_dto(analysis.state),
            findings: analysis
                .findings
                .iter()
                .map(ImportFindingDto::from_folder_domain)
                .collect(),
            creatable_summary: analysis.creatable.as_ref().map(|creatable| {
                let mut seen = std::collections::BTreeSet::new();
                let retained_media: Vec<String> = creatable
                    .retained_media
                    .iter()
                    .filter(|media| seen.insert(media.basename.clone()))
                    .map(|media| media.basename.clone())
                    .collect();
                CreatableSummaryDto {
                    // The summary shows what WILL be stored — the
                    // normalized title, exactly like the created row.
                    title: normalize_title(&creatable.title),
                    node_count: creatable.structure.nodes.len() as u32,
                    retained_media,
                    discarded_media: analysis.discarded_media.clone(),
                }
            }),
            folder_name,
            folder_path,
        }
    }
}

/// Input accepted by the `accept_structured_creation` Tauri command: the
/// folder path from a prior analysis, round-tripped verbatim. The accept
/// phase re-analyzes the disk from zero — the wire carries a POINTER, never
/// an authority. `deny_unknown_fields` keeps the boundary authoritative.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptStructuredCreationInputDto {
    pub folder_path: String,
}

// ===== RSS external-source creation (feed → new canonical story) =====

/// Ceiling on one previewed item's `summary`, in Unicode scalar values —
/// a WIRE bound (the full cleaned text can reach 65 536 chars per item ×
/// 100 items; the preview only needs a readable excerpt). Truncation
/// appends an ellipsis.
pub const MAX_RSS_SUMMARY_CHARS: usize = 280;

/// The stable reference of one previewed feed item, round-tripped by the
/// frontend to `accept_rss_story_creation` and re-resolved from zero
/// against a FRESH fetch. The selector mirrors the domain [`RssItemRef`];
/// `fingerprint` is the canonical proof of the PREVIEWED content
/// ([`crate::domain::import::rss_item_fingerprint`]) — the accept
/// recomputes it on the fresh item and refuses any divergence (the wire
/// carries a pointer + a proof, never content). `deny_unknown_fields`
/// keeps the boundary authoritative on the way in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RssItemRefDto {
    #[serde(rename_all = "camelCase")]
    Guid { guid: String, fingerprint: String },
    #[serde(rename_all = "camelCase")]
    TitleLink {
        title: String,
        link: Option<String>,
        fingerprint: String,
    },
}

impl RssItemRefDto {
    pub fn from_domain(reference: &RssItemRef, fingerprint: String) -> Self {
        match reference {
            RssItemRef::Guid(guid) => Self::Guid {
                guid: guid.clone(),
                fingerprint,
            },
            RssItemRef::TitleLink { title, link } => Self::TitleLink {
                title: title.clone(),
                link: link.clone(),
                fingerprint,
            },
        }
    }

    pub fn to_domain(&self) -> RssItemRef {
        match self {
            Self::Guid { guid, .. } => RssItemRef::Guid(guid.clone()),
            Self::TitleLink { title, link, .. } => RssItemRef::TitleLink {
                title: title.clone(),
                link: link.clone(),
            },
        }
    }

    /// The previewed-content proof carried by this reference.
    pub fn fingerprint(&self) -> &str {
        match self {
            Self::Guid { fingerprint, .. } => fingerprint,
            Self::TitleLink { fingerprint, .. } => fingerprint,
        }
    }
}

/// One selectable item of a previewed feed: the cleaned title (possibly
/// empty — the surface then leads with the summary), a bounded summary
/// excerpt, the enclosure fact and the round-trip reference.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RssPreviewItemDto {
    pub title: String,
    pub summary: String,
    pub has_enclosure: bool,
    pub item_ref: RssItemRefDto,
}

/// The typed outcome of `fetch_rss_source_preview`: the source HOST (the
/// only address fragment that ever crosses), the selectable items, the
/// flow-level findings (RSS per-pair copy) and the derived state.
/// `blocked` is the redundant-but-explicit branch flag — always coherent
/// with `state` (the TS guard refuses a divergence). A TRANSPORT failure
/// rejects with `RSS_SOURCE_UNREACHABLE` instead — the functional verdict
/// is NEVER an error.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RssPreviewDto {
    pub source_host: String,
    pub items: Vec<RssPreviewItemDto>,
    pub findings: Vec<ImportFindingDto>,
    pub state: ImportStateDto,
    pub blocked: bool,
}

impl RssPreviewDto {
    /// Map a domain feed analysis + the validated host to the wire preview
    /// (RSS per-pair copy, bounded summaries, per-item references).
    pub fn from_analysis(
        source_host: String,
        analysis: &crate::domain::import::RssAnalysis,
    ) -> Self {
        Self {
            source_host,
            items: analysis
                .items
                .iter()
                .map(|item| RssPreviewItemDto {
                    title: item.title.clone(),
                    summary: truncate_rss_summary(&item.text),
                    has_enclosure: item.has_enclosure,
                    item_ref: RssItemRefDto::from_domain(
                        &crate::domain::import::rss_item_ref(item),
                        crate::domain::import::rss_item_fingerprint(item),
                    ),
                })
                .collect(),
            findings: analysis
                .findings
                .iter()
                .map(ImportFindingDto::from_rss_domain)
                .collect(),
            state: state_dto(analysis.state),
            blocked: analysis.is_blocked(),
        }
    }
}

/// Truncate a cleaned item text to the wire summary bound, appending an
/// ellipsis when something was cut.
fn truncate_rss_summary(text: &str) -> String {
    if text.chars().count() <= MAX_RSS_SUMMARY_CHARS {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(MAX_RSS_SUMMARY_CHARS).collect();
    truncated.push('…');
    truncated
}

/// Tagged outcome of `accept_rss_story_creation`: the created card + its
/// report, or the honest recoverable refusal (`sourceChanged` — the feed
/// diverged since the preview; NOTHING was created). The refusal is a
/// typed verdict, never an `AppError` (only transport rejects).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RssCreationOutcomeDto {
    #[serde(rename_all = "camelCase")]
    Created {
        story: crate::ipc::dto::StoryCardDto,
        report: Vec<ImportFindingDto>,
    },
    SourceChanged,
}

/// The frozen user-facing label of a content-source kind
/// (`product-language.md`). Exhaustive match — adding a kind without
/// deciding its label is a compile error (the DTO tripwire pattern).
pub fn content_source_label(kind: ContentSourceKind) -> &'static str {
    match kind {
        ContentSourceKind::Rss => "Flux RSS",
        ContentSourceKind::Atom => "Flux Atom",
        ContentSourceKind::JsonFeed => "Flux JSON Feed",
    }
}

/// The frozen disabled-entry reason of an activation state — `None` for
/// an enabled line (an active entry carries the activation marker, not a
/// reason). Exhaustive match (tripwire): a new activation state cannot
/// ship without deciding its reason copy.
pub fn content_source_reason(activation: ContentSourceActivation) -> Option<&'static str> {
    match activation {
        ContentSourceActivation::Enabled => None,
        ContentSourceActivation::NotActivated => {
            Some("Source indisponible: non activée dans la distribution officielle")
        }
        ContentSourceActivation::BlockedByPolicy => {
            Some("Source indisponible: bloquée par la politique de distribution")
        }
    }
}

/// The frozen entry-level activation marker of an ENABLED line — `None`
/// otherwise (a non-enabled line carries its reason instead). Carried by
/// the policy DTO so EVERY surface rendering the marker (the creation
/// dialog, the support-profile screen) renders the SAME Rust-owned copy
/// verbatim — never a re-typed frontend literal. Exhaustive match
/// (tripwire).
pub fn content_source_activation_marker(
    activation: ContentSourceActivation,
) -> Option<&'static str> {
    match activation {
        ContentSourceActivation::Enabled => Some("Activée par la distribution officielle"),
        ContentSourceActivation::NotActivated | ContentSourceActivation::BlockedByPolicy => None,
    }
}

/// One serialized line of the content-source policy: the closed wire tags
/// (`kind`, `activation`), the frozen label, and EXACTLY ONE of — the
/// frozen entry-level activation marker on an enabled line, or the frozen
/// disabled-entry reason on a non-enabled one (each key is OMITTED
/// otherwise, and the TS guard refuses any incoherence). Every copy is
/// Rust-authoritative: the frontend renders these strings verbatim and
/// never recomposes them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContentSourceDto {
    pub kind: &'static str,
    pub label: &'static str,
    pub activation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_marker: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// The serialized content-source policy: every line of the received
/// matrix, in its stable order (`read_content_source_policy` hands the
/// official matrix; tests may serialize custom ones).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContentSourcePolicyDto {
    pub sources: Vec<ContentSourceDto>,
}

impl ContentSourcePolicyDto {
    /// Map a content-source matrix to its wire policy (tags, frozen
    /// labels, frozen reasons).
    pub fn from_lines(lines: &[ContentSourceLine]) -> Self {
        Self {
            sources: lines
                .iter()
                .map(|line| ContentSourceDto {
                    kind: line.kind.wire_tag(),
                    label: content_source_label(line.kind),
                    activation: line.activation.wire_tag(),
                    activation_marker: content_source_activation_marker(line.activation),
                    reason: content_source_reason(line.activation),
                })
                .collect(),
        }
    }
}

/// The persisted shape of one attention finding inside
/// `story_local_imports.findings_summary` — `(aspect, category)` codes
/// only, NEVER the localized message (re-derived at read time, so the
/// stored JSON is PII-free and i18n-stable). `deny_unknown_fields` guards
/// against a drifting stored shape.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredImportFinding {
    pub aspect: ImportAspectDto,
    pub category: ImportCategoryDto,
}

/// The durable DB tag stored in `story_local_imports.import_state` for a
/// persistable import state. `recognized` / `partial` / `needs_review` are
/// written at import time; `resolved` is written by the write-path review
/// resolution ONLY (`application::story::review`). `Blocked` (never
/// imported) is unreachable here and falls back to `needs_review` so a
/// future code path passes the CHECK rather than corrupting the row.
pub fn state_db_tag(state: ImportState) -> &'static str {
    match state {
        ImportState::Recognized => "recognized",
        ImportState::Partial => "partial",
        ImportState::Resolved => "resolved",
        ImportState::NeedsReview | ImportState::Blocked => "needs_review",
    }
}

/// Parse a stored `story_local_imports.import_state` tag into its wire DTO.
/// An unknown tag yields `None` so the overview read degrades a corrupt
/// provenance row to a native card instead of failing the whole read.
pub fn import_state_dto_from_tag(tag: &str) -> Option<ImportStateDto> {
    match tag {
        "recognized" => Some(ImportStateDto::Recognized),
        "partial" => Some(ImportStateDto::Partial),
        "needs_review" => Some(ImportStateDto::NeedsReview),
        "resolved" => Some(ImportStateDto::Resolved),
        _ => None,
    }
}

pub fn quality_dto(quality: RecognitionQuality) -> ImportQualityDto {
    match quality {
        RecognitionQuality::Clean => ImportQualityDto::Clean,
        RecognitionQuality::Partial => ImportQualityDto::Partial,
        RecognitionQuality::Unusable => ImportQualityDto::Unusable,
    }
}

pub fn state_dto(state: ImportState) -> ImportStateDto {
    match state {
        ImportState::Recognized => ImportStateDto::Recognized,
        ImportState::Partial => ImportStateDto::Partial,
        ImportState::NeedsReview => ImportStateDto::NeedsReview,
        ImportState::Blocked => ImportStateDto::Blocked,
        ImportState::Resolved => ImportStateDto::Resolved,
    }
}

pub fn aspect_dto(aspect: RecognitionAspect) -> ImportAspectDto {
    match aspect {
        RecognitionAspect::Envelope => ImportAspectDto::Envelope,
        RecognitionAspect::FormatVersion => ImportAspectDto::FormatVersion,
        RecognitionAspect::SchemaVersion => ImportAspectDto::SchemaVersion,
        RecognitionAspect::Structure => ImportAspectDto::Structure,
        RecognitionAspect::Integrity => ImportAspectDto::Integrity,
        RecognitionAspect::Title => ImportAspectDto::Title,
        RecognitionAspect::Timestamps => ImportAspectDto::Timestamps,
        RecognitionAspect::Media => ImportAspectDto::Media,
        RecognitionAspect::Source => ImportAspectDto::Source,
    }
}

pub fn category_dto(category: RecognitionCategory) -> ImportCategoryDto {
    match category {
        RecognitionCategory::Recognized => ImportCategoryDto::Recognized,
        RecognitionCategory::Ambiguous => ImportCategoryDto::Ambiguous,
        RecognitionCategory::Missing => ImportCategoryDto::Missing,
        RecognitionCategory::Blocking => ImportCategoryDto::Blocking,
    }
}

impl ImportFindingDto {
    /// Map a domain finding to its wire shape, generating the single
    /// canonical FR message for its `(aspect, category)` pair.
    pub fn from_domain(finding: &RecognitionFinding) -> Self {
        let aspect = aspect_dto(finding.aspect);
        let category = category_dto(finding.category);
        Self {
            aspect,
            category,
            message: finding_message(aspect, category).to_string(),
        }
    }

    /// The structured-folder variant: same discriminants, the FOLDER
    /// per-pair FR copy ([`structured_folder_finding_message`]).
    pub fn from_folder_domain(finding: &RecognitionFinding) -> Self {
        let aspect = aspect_dto(finding.aspect);
        let category = category_dto(finding.category);
        Self {
            aspect,
            category,
            message: structured_folder_finding_message(aspect, category).to_string(),
        }
    }

    /// The RSS-ingestion variant: same discriminants, the RSS per-pair FR
    /// copy ([`rss_finding_message`]).
    pub fn from_rss_domain(finding: &RecognitionFinding) -> Self {
        let aspect = aspect_dto(finding.aspect);
        let category = category_dto(finding.category);
        Self {
            aspect,
            category,
            message: rss_finding_message(aspect, category).to_string(),
        }
    }
}

/// Single canonical FR copy per `(aspect, category)` — never two wordings
/// for one pair WITHIN a flow. Mirrors `docs/architecture/ui-states.md#Local
/// Artifact Import Contract`; the `media` pairs (folder flow only) mirror
/// `product-language.md#Structured-folder recognition copy`. The folder
/// flow overrides the shared-aspect pairs whose wording differs through
/// [`structured_folder_finding_message`].
pub fn finding_message(aspect: ImportAspectDto, category: ImportCategoryDto) -> &'static str {
    use ImportAspectDto as A;
    use ImportCategoryDto as C;
    match (aspect, category) {
        (A::Envelope, C::Recognized) => "L'enveloppe de l'artefact est valide.",
        (A::Envelope, _) => "Le fichier n'est pas un artefact Rustory valide.",
        (A::FormatVersion, C::Recognized) => "La version de format de l'artefact est prise en charge.",
        (A::FormatVersion, _) => {
            "La version de format de cet artefact n'est pas prise en charge par cette version de Rustory."
        }
        (A::SchemaVersion, C::Recognized) => "La version de schéma de l'histoire est prise en charge.",
        (A::SchemaVersion, _) => {
            "Cette histoire utilise un format plus récent que celui pris en charge par cette version de Rustory."
        }
        (A::Structure, C::Recognized) => "La structure interne de l'histoire est reconnue.",
        (A::Structure, _) => "La structure interne de l'histoire est illisible ou incohérente.",
        (A::Integrity, C::Recognized) => "L'intégrité de l'histoire est vérifiée (empreinte conforme).",
        (A::Integrity, _) => {
            "Les données de l'histoire ont changé de façon inattendue (corruption détectée)."
        }
        (A::Title, C::Recognized) => "Le titre de l'histoire est valide.",
        (A::Title, C::Ambiguous) => {
            "Le titre a été normalisé à l'import (espaces ou caractères ajustés)."
        }
        (A::Title, _) => "Le titre enregistré de l'histoire n'est pas valide.",
        (A::Timestamps, C::Recognized) => "Les dates de l'histoire sont au format attendu.",
        (A::Timestamps, _) => {
            "Une date de l'histoire n'a pas le format attendu ; elle a été conservée telle quelle."
        }
        // `media` pairs — emitted by the structured-folder flow only; the
        // copy lives here too so a persisted `(media, …)` pair re-rendered
        // through the shared path never panics nor falls back empty.
        (A::Media, C::Recognized) => {
            "Tous les fichiers audio et image référencés par le dossier sont présents et reconnus."
        }
        (A::Media, C::Missing) => {
            "Certains fichiers audio ou image référencés par le dossier sont introuvables. L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur."
        }
        // No flow emits `(media, blocking)` — the defensive copy stays
        // COHERENT with its chip (a real block promises no creation) in
        // case a persisted/forged summary carries the pair.
        (A::Media, C::Blocking) => "Un média référencé par le dossier bloque la création.",
        (A::Media, C::Ambiguous) => {
            "Certains fichiers audio ou image référencés ne sont pas utilisables (format non reconnu, fichier trop volumineux ou nom invalide). L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur."
        }
        // `source` pairs — the RSS ingestion flow owns the living copy
        // ([`rss_finding_message`]); only its `(source, ambiguous)` pair is
        // ever emitted. The defensive copies below keep a persisted/forged
        // pair renderable through the shared path without a panic and
        // without promising anything.
        (A::Source, C::Ambiguous) => {
            "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser."
        }
        (A::Source, C::Recognized) => "La provenance de cette histoire a été enregistrée.",
        (A::Source, C::Missing) => "La provenance de cette histoire n'a pas pu être établie.",
        (A::Source, C::Blocking) => "La provenance de cette histoire bloque la création.",
    }
}

/// The RSS-INGESTION per-pair FR copy (frozen in
/// `product-language.md#RSS ingestion copy`). The RSS flow owns the wording
/// of every pair it emits — it speaks of a feed, an episode and an
/// ingestion; the BLOCKING pairs ARE the feed verdicts, each carrying the
/// corrective gesture. Every other pair delegates to the shared table.
/// Used by the RSS preview/creation DTOs AND by the durable card report
/// when the provenance's `source_format` is `rss`.
pub fn rss_finding_message(aspect: ImportAspectDto, category: ImportCategoryDto) -> &'static str {
    use ImportAspectDto as A;
    use ImportCategoryDto as C;
    match (aspect, category) {
        (A::Envelope, C::Recognized) => "Le flux RSS est lisible.",
        (A::Envelope, C::Ambiguous | C::Missing | C::Blocking) => {
            "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux."
        }
        (A::FormatVersion, C::Recognized) => "Le flux est au format RSS 2.0 supporté.",
        (A::FormatVersion, C::Ambiguous | C::Missing | C::Blocking) => {
            "Ce flux n'est pas au format RSS supporté. Relance la récupération du flux."
        }
        (A::Title, C::Recognized) => "Le titre de l'épisode est valide.",
        (A::Title, C::Ambiguous | C::Missing | C::Blocking) => {
            "Le titre de l'épisode était absent ou a été ajusté à l'ingestion. Vérifie le titre de l'histoire dans l'éditeur."
        }
        (A::Structure, C::Recognized) => "Le texte de l'épisode est reconnu.",
        (A::Structure, C::Ambiguous) => {
            "Le texte de l'épisode était absent ou a été ajusté à l'ingestion (balises HTML retirées, blancs ou longueur réduits). Relis le texte dans l'éditeur."
        }
        (A::Structure, C::Missing | C::Blocking) => {
            "Ce flux ne contient aucun épisode exploitable. Relance la récupération du flux."
        }
        (A::Media, C::Missing) => {
            "Le média distant référencé par la source n'a pas été récupéré. Ajoute le média manuellement dans l'éditeur."
        }
        _ => finding_message(aspect, category),
    }
}

/// The STRUCTURED-FOLDER per-pair FR copy (frozen in
/// `product-language.md#Structured-folder recognition copy`). The folder
/// flow owns the wording of the shared-aspect pairs that speak of a
/// manifest and a creation (`Envelope`, `FormatVersion`, `Title`,
/// `Structure`) — the `.rustory` copy keeps speaking of an artifact and an
/// import; every other pair (including `media`) delegates to the shared
/// table. Used by the folder analysis DTO AND by the durable card report
/// when the provenance's `source_format` is `structured-folder`.
pub fn structured_folder_finding_message(
    aspect: ImportAspectDto,
    category: ImportCategoryDto,
) -> &'static str {
    use ImportAspectDto as A;
    use ImportCategoryDto as C;
    match (aspect, category) {
        (A::Envelope, C::Recognized) => "Le manifest histoire.json est présent et lisible.",
        (A::Envelope, C::Ambiguous | C::Missing | C::Blocking) => {
            "Le dossier ne contient pas de manifest histoire.json lisible. Corrige le dossier puis relance l'analyse."
        }
        (A::FormatVersion, C::Recognized) => "La version de format du manifest est prise en charge.",
        (A::FormatVersion, C::Ambiguous | C::Missing | C::Blocking) => {
            "La version de format de ce manifest n'est pas prise en charge par cette version de Rustory. Corrige le manifest puis relance l'analyse."
        }
        (A::Title, C::Ambiguous) => {
            "Le titre a été normalisé à la création (espaces ou caractères ajustés)."
        }
        (A::Title, C::Missing | C::Blocking) => {
            "Le titre du manifest est manquant ou n'est pas valide. Corrige le manifest puis relance l'analyse."
        }
        (A::Structure, C::Recognized) => "La structure de l'histoire est reconnue.",
        (A::Structure, C::Ambiguous) => {
            "La structure contient un champ inattendu ou un lien d'option vers un nœud inconnu ; l'histoire sera créée telle quelle et tu pourras corriger dans l'éditeur."
        }
        (A::Structure, C::Missing | C::Blocking) => {
            "La structure du manifest est incomplète ou incohérente. Corrige le manifest puis relance l'analyse."
        }
        _ => finding_message(aspect, category),
    }
}

/// Serialize the FULL per-aspect report of an analysis into the compact
/// JSON stored in `story_local_imports.findings_summary`. Returns `None`
/// for a clean import (all aspects recognized ⇒ no marker, no report, NULL
/// column). When there IS at least one point of attention, EVERY aspect is
/// stored — recognized AND attention — so the durable on-demand report can
/// show the global outcome + the recognized elements + the points of
/// attention after a restart, not just the attention items (§5).
pub fn serialize_findings_summary(findings: &[RecognitionFinding]) -> Option<String> {
    if findings
        .iter()
        .all(|f| f.category == RecognitionCategory::Recognized)
    {
        return None;
    }
    let report: Vec<StoredImportFinding> = findings
        .iter()
        .map(|f| StoredImportFinding {
            aspect: aspect_dto(f.aspect),
            category: category_dto(f.category),
        })
        .collect();
    // Serializing a small `Vec` of plain enums cannot fail in practice.
    serde_json::to_string(&report).ok()
}

/// Reconstruct the on-demand report (with each aspect's canonical message)
/// from a stored `findings_summary` JSON — the FULL per-aspect report
/// (recognized + attention). A malformed summary degrades to an empty list
/// (the marker still shows from the state column; the report is just empty)
/// — never a hard failure of the overview read.
pub fn import_findings_from_summary(summary: &str) -> Vec<ImportFindingDto> {
    findings_from_summary_with(summary, finding_message)
}

/// The structured-folder variant of [`import_findings_from_summary`]: the
/// same stored pairs, re-rendered with the FOLDER per-pair copy. The
/// projection picks it by the provenance's `source_format` so a folder
/// story's durable card report speaks of a manifest, never of an artifact.
pub fn folder_import_findings_from_summary(summary: &str) -> Vec<ImportFindingDto> {
    findings_from_summary_with(summary, structured_folder_finding_message)
}

/// The RSS variant of [`import_findings_from_summary`]: the same stored
/// pairs, re-rendered with the RSS per-pair copy. Picked by the
/// provenance's `source_format = 'rss'` so an ingested story's durable
/// card report speaks of a feed and an episode.
pub fn rss_import_findings_from_summary(summary: &str) -> Vec<ImportFindingDto> {
    findings_from_summary_with(summary, rss_finding_message)
}

fn findings_from_summary_with(
    summary: &str,
    message: fn(ImportAspectDto, ImportCategoryDto) -> &'static str,
) -> Vec<ImportFindingDto> {
    serde_json::from_str::<Vec<StoredImportFinding>>(summary)
        .unwrap_or_default()
        .into_iter()
        .map(|stored| ImportFindingDto {
            aspect: stored.aspect,
            category: stored.category,
            message: message(stored.aspect, stored.category).to_string(),
        })
        .collect()
}

/// The FULL per-aspect report of an analysis as wire DTOs with their
/// canonical messages — the `importReport` carried on a freshly imported
/// Story Card, built directly without the storage round-trip. Empty for a
/// clean import (no marker, no report).
pub fn import_report_dto(findings: &[RecognitionFinding]) -> Vec<ImportFindingDto> {
    if findings
        .iter()
        .all(|f| f.category == RecognitionCategory::Recognized)
    {
        return Vec::new();
    }
    findings.iter().map(ImportFindingDto::from_domain).collect()
}

/// The structured-folder variant of [`import_report_dto`] — same rules,
/// the FOLDER per-pair copy.
pub fn folder_import_report_dto(findings: &[RecognitionFinding]) -> Vec<ImportFindingDto> {
    if findings
        .iter()
        .all(|f| f.category == RecognitionCategory::Recognized)
    {
        return Vec::new();
    }
    findings
        .iter()
        .map(ImportFindingDto::from_folder_domain)
        .collect()
}

/// The RSS variant of [`import_report_dto`] — same rules, the RSS per-pair
/// copy. In practice never empty: every ingestion carries the nominal
/// `(source, ambiguous)` finding.
pub fn rss_import_report_dto(findings: &[RecognitionFinding]) -> Vec<ImportFindingDto> {
    if findings
        .iter()
        .all(|f| f.category == RecognitionCategory::Recognized)
    {
        return Vec::new();
    }
    findings
        .iter()
        .map(ImportFindingDto::from_rss_domain)
        .collect()
}

impl ImportableContentDto {
    pub fn from_domain(content: &ImportableContent) -> Self {
        Self {
            title: content.title.clone(),
            structure_json: content.structure_json.clone(),
            content_checksum: content.content_checksum.clone(),
            created_at: content.created_at.clone(),
            updated_at: content.updated_at.clone(),
        }
    }
}

impl ImportArtifactAnalysisDto {
    /// Map a domain analysis + its provenance metadata to the `analyzed`
    /// wire verdict (generating every finding's canonical message).
    pub fn analyzed(
        analysis: &ArtifactAnalysis,
        source_name: String,
        artifact_checksum: String,
    ) -> Self {
        Self::Analyzed {
            quality: quality_dto(analysis.quality),
            state: state_dto(analysis.state),
            findings: analysis
                .findings
                .iter()
                .map(ImportFindingDto::from_domain)
                .collect(),
            importable_content: analysis
                .importable
                .as_ref()
                .map(ImportableContentDto::from_domain),
            source_name,
            artifact_checksum,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_story_dialog_input_accepts_canonical_camel_case_payload() {
        let dto: ExportStoryDialogInputDto = serde_json::from_value(serde_json::json!({
            "storyId": "0197a5d0-0000-7000-8000-000000000000",
            "suggestedFilename": "Mon histoire.rustory",
        }))
        .expect("deser");
        assert_eq!(dto.story_id, "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(dto.suggested_filename, "Mon histoire.rustory");
    }

    #[test]
    fn export_story_dialog_input_rejects_snake_case_story_id() {
        let err = serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
            "story_id": "x",
            "suggestedFilename": "y.rustory",
        }))
        .expect_err("must reject snake_case");
        let message = err.to_string().to_lowercase();
        assert!(
            message.contains("story_id") || message.contains("unknown field"),
            "expected snake_case or unknown-field rejection, got: {message}"
        );
    }

    #[test]
    fn export_story_dialog_input_rejects_unknown_field() {
        let err = serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
            "storyId": "x",
            "suggestedFilename": "y.rustory",
            "extra": "z",
        }))
        .expect_err("must reject unknown field");
        assert!(err.to_string().contains("extra"));
    }

    #[test]
    fn export_story_dialog_input_rejects_missing_fields() {
        serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
            "storyId": "x",
        }))
        .expect_err("must reject missing suggestedFilename");
        serde_json::from_value::<ExportStoryDialogInputDto>(serde_json::json!({
            "suggestedFilename": "y.rustory",
        }))
        .expect_err("must reject missing storyId");
    }

    #[test]
    fn exported_outcome_wire_shape_is_tagged_camel_case() {
        let dto = ExportStoryDialogOutcomeDto::Exported {
            destination_path: "/tmp/histoire.rustory".into(),
            bytes_written: 451,
            content_checksum: "a".repeat(64),
        };
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(v["kind"], "exported");
        assert_eq!(v["destinationPath"], "/tmp/histoire.rustory");
        assert_eq!(v["bytesWritten"], 451);
        assert_eq!(v["contentChecksum"].as_str().unwrap().len(), 64);
        for snake in ["destination_path", "bytes_written", "content_checksum"] {
            assert!(v.get(snake).is_none(), "{snake} must be camelCase");
        }
    }

    #[test]
    fn cancelled_outcome_wire_shape_carries_only_kind() {
        let dto = ExportStoryDialogOutcomeDto::Cancelled;
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(v["kind"], "cancelled");
        // Only the discriminant is present — no destination, no bytes.
        assert_eq!(v.as_object().expect("object").len(), 1);
    }

    // ===== Local artifact import DTOs =====

    fn importable_content() -> ImportableContentDto {
        ImportableContentDto {
            title: "Le Soleil".into(),
            structure_json: "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}".into(),
            content_checksum: "a".repeat(64),
            created_at: "2026-06-20T10:00:00.000Z".into(),
            updated_at: "2026-06-24T14:15:00.000Z".into(),
        }
    }

    #[test]
    fn analyzed_verdict_wire_shape_is_tagged_camel_case() {
        let dto = ImportArtifactAnalysisDto::Analyzed {
            quality: ImportQualityDto::Partial,
            state: ImportStateDto::NeedsReview,
            findings: vec![ImportFindingDto {
                aspect: ImportAspectDto::Title,
                category: ImportCategoryDto::Ambiguous,
                message: "msg".into(),
            }],
            importable_content: Some(importable_content()),
            source_name: "histoire.rustory".into(),
            artifact_checksum: "b".repeat(64),
        };
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(v["kind"], "analyzed");
        assert_eq!(v["quality"], "partial");
        assert_eq!(v["state"], "needsReview");
        assert_eq!(v["findings"][0]["aspect"], "title");
        assert_eq!(v["findings"][0]["category"], "ambiguous");
        assert_eq!(v["sourceName"], "histoire.rustory");
        assert_eq!(
            v["importableContent"]["structureJson"].as_str().unwrap(),
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}"
        );
        for snake in ["source_name", "artifact_checksum", "importable_content"] {
            assert!(v.get(snake).is_none(), "{snake} must be camelCase");
        }
    }

    #[test]
    fn a_blocked_verdict_omits_importable_content() {
        let dto = ImportArtifactAnalysisDto::Analyzed {
            quality: ImportQualityDto::Unusable,
            state: ImportStateDto::Blocked,
            findings: vec![ImportFindingDto {
                aspect: ImportAspectDto::Integrity,
                category: ImportCategoryDto::Blocking,
                message: "msg".into(),
            }],
            importable_content: None,
            source_name: "corrompu.rustory".into(),
            artifact_checksum: "c".repeat(64),
        };
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(v["state"], "blocked");
        assert!(
            v.get("importableContent").is_none(),
            "a blocked verdict carries no importable content"
        );
    }

    #[test]
    fn analysis_cancelled_wire_shape_carries_only_kind() {
        let v = serde_json::to_value(ImportArtifactAnalysisDto::Cancelled).expect("serialize");
        assert_eq!(v, serde_json::json!({ "kind": "cancelled" }));
    }

    #[test]
    fn accept_input_accepts_canonical_camel_case_payload() {
        let dto: AcceptArtifactImportInputDto = serde_json::from_value(serde_json::json!({
            "content": {
                "title": "Le Soleil",
                "structureJson": "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}",
                "contentChecksum": "a".repeat(64),
                "createdAt": "2026-06-20T10:00:00.000Z",
                "updatedAt": "2026-06-24T14:15:00.000Z",
            },
            "sourceName": "histoire.rustory",
            "artifactChecksum": "b".repeat(64),
        }))
        .expect("deser");
        assert_eq!(dto.content.title, "Le Soleil");
        assert_eq!(dto.source_name, "histoire.rustory");
    }

    #[test]
    fn accept_input_rejects_snake_case_and_unknown_field() {
        let snake = serde_json::from_value::<AcceptArtifactImportInputDto>(serde_json::json!({
            "content": importable_content_json(),
            "source_name": "x.rustory",
            "artifactChecksum": "b".repeat(64),
        }));
        assert!(snake.is_err(), "snake_case source_name must be refused");

        let unknown = serde_json::from_value::<AcceptArtifactImportInputDto>(serde_json::json!({
            "content": importable_content_json(),
            "sourceName": "x.rustory",
            "artifactChecksum": "b".repeat(64),
            "extra": "z",
        }));
        assert!(unknown.is_err(), "unknown field must be refused");
    }

    #[test]
    fn importable_content_rejects_an_unknown_field() {
        let mut content = importable_content_json();
        content
            .as_object_mut()
            .expect("obj")
            .insert("schemaVersion".into(), serde_json::json!(1));
        let err = serde_json::from_value::<ImportableContentDto>(content)
            .expect_err("schemaVersion is not part of the wire content");
        assert!(err.to_string().contains("schemaVersion") || err.to_string().contains("unknown"));
    }

    fn importable_content_json() -> serde_json::Value {
        serde_json::json!({
            "title": "Le Soleil",
            "structureJson": "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}",
            "contentChecksum": "a".repeat(64),
            "createdAt": "2026-06-20T10:00:00.000Z",
            "updatedAt": "2026-06-24T14:15:00.000Z",
        })
    }

    #[test]
    fn every_aspect_category_pair_has_a_non_empty_message() {
        use ImportAspectDto::*;
        use ImportCategoryDto::*;
        let aspects = [
            Envelope,
            FormatVersion,
            SchemaVersion,
            Structure,
            Integrity,
            Title,
            Timestamps,
            Media,
            Source,
        ];
        let categories = [Recognized, Ambiguous, Missing, Blocking];
        for aspect in aspects {
            for category in categories {
                assert!(
                    !finding_message(aspect, category).is_empty(),
                    "{aspect:?}/{category:?} message empty"
                );
                // The folder copy covers every pair too (its own wording or
                // the shared delegation) — no panic, no empty fallback.
                assert!(
                    !structured_folder_finding_message(aspect, category).is_empty(),
                    "folder {aspect:?}/{category:?} message empty"
                );
                // The RSS copy covers every pair too.
                assert!(
                    !rss_finding_message(aspect, category).is_empty(),
                    "rss {aspect:?}/{category:?} message empty"
                );
            }
        }
    }

    #[test]
    fn folder_copy_speaks_of_the_manifest_and_shares_the_media_copy() {
        // The folder flow owns the wording of its shared-aspect pairs…
        assert_eq!(
            structured_folder_finding_message(
                ImportAspectDto::Envelope,
                ImportCategoryDto::Blocking
            ),
            "Le dossier ne contient pas de manifest histoire.json lisible. Corrige le dossier puis relance l'analyse."
        );
        assert_ne!(
            structured_folder_finding_message(
                ImportAspectDto::Envelope,
                ImportCategoryDto::Blocking
            ),
            finding_message(ImportAspectDto::Envelope, ImportCategoryDto::Blocking),
            "the .rustory copy keeps speaking of an artifact"
        );
        // …and the `media` pairs are ONE copy, shared by construction.
        for category in [
            ImportCategoryDto::Recognized,
            ImportCategoryDto::Ambiguous,
            ImportCategoryDto::Missing,
            ImportCategoryDto::Blocking,
        ] {
            assert_eq!(
                structured_folder_finding_message(ImportAspectDto::Media, category),
                finding_message(ImportAspectDto::Media, category)
            );
        }
    }

    #[test]
    fn the_dead_media_blocking_pair_never_promises_a_creation() {
        // No flow emits `(media, blocking)`, but a persisted/forged summary
        // can re-render it: the defensive copy must stay coherent with the
        // `blocage réel` chip — never "the story will be created anyway".
        let copy = finding_message(ImportAspectDto::Media, ImportCategoryDto::Blocking);
        assert!(!copy.contains("sera créée"), "no creation promise: {copy}");
        assert!(!copy.is_empty());
    }

    #[test]
    fn every_folder_blocking_copy_names_the_corrective_gesture() {
        // Cause + impact + GESTURE: a blocked report tells the user what to
        // do next (fix the folder, re-run the analysis) — the blocked
        // surface only offers `Abandonner`.
        for aspect in [
            ImportAspectDto::Envelope,
            ImportAspectDto::FormatVersion,
            ImportAspectDto::Title,
            ImportAspectDto::Structure,
        ] {
            let copy = structured_folder_finding_message(aspect, ImportCategoryDto::Blocking);
            assert!(
                copy.contains("relance l'analyse"),
                "{aspect:?} blocking copy must name the gesture: {copy}"
            );
        }
    }

    #[test]
    fn findings_summary_round_trips_the_full_report_when_there_is_attention() {
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::ambiguous(RecognitionAspect::Title),
            RecognitionFinding::recognized(RecognitionAspect::Integrity),
        ];
        let summary = serialize_findings_summary(&findings).expect("an attention finding exists");
        let report = import_findings_from_summary(&summary);
        // The FULL report is stored (recognized AND attention), so the durable
        // on-demand report shows both groups after a restart (§5).
        assert_eq!(
            report.len(),
            3,
            "every aspect is stored, not only attention"
        );
        assert!(report
            .iter()
            .any(|f| f.aspect == ImportAspectDto::Title
                && f.category == ImportCategoryDto::Ambiguous));
        assert_eq!(
            report
                .iter()
                .filter(|f| f.category == ImportCategoryDto::Recognized)
                .count(),
            2
        );
        assert!(report.iter().all(|f| !f.message.is_empty()));
    }

    #[test]
    fn a_clean_analysis_serializes_no_findings_summary_and_empty_report() {
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::recognized(RecognitionAspect::Title),
        ];
        assert!(serialize_findings_summary(&findings).is_none());
        assert!(import_report_dto(&findings).is_empty());
    }

    #[test]
    fn import_report_dto_carries_the_full_report_when_there_is_attention() {
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::ambiguous(RecognitionAspect::Timestamps),
        ];
        let report = import_report_dto(&findings);
        assert_eq!(report.len(), 2);
        assert!(report
            .iter()
            .any(|f| f.category == ImportCategoryDto::Recognized));
        assert!(report
            .iter()
            .any(|f| f.category == ImportCategoryDto::Ambiguous));
    }

    #[test]
    fn a_malformed_summary_degrades_to_an_empty_report() {
        assert!(import_findings_from_summary("not json").is_empty());
        assert!(import_findings_from_summary("[]").is_empty());
    }

    // ===== RSS external-source DTOs =====

    #[test]
    fn rss_item_ref_round_trips_both_variants_with_the_fingerprint() {
        for reference in [
            RssItemRef::Guid("g-1".into()),
            RssItemRef::TitleLink {
                title: "Episode".into(),
                link: Some("https://exemple.fr/ep".into()),
            },
            RssItemRef::TitleLink {
                title: "Episode".into(),
                link: None,
            },
        ] {
            let dto = RssItemRefDto::from_domain(&reference, "a".repeat(64));
            let json = serde_json::to_value(&dto).expect("ser");
            let back: RssItemRefDto = serde_json::from_value(json).expect("deser");
            assert_eq!(back.to_domain(), reference);
            assert_eq!(back.fingerprint(), "a".repeat(64));
        }
    }

    #[test]
    fn rss_item_ref_wire_shape_is_tagged_camel_case() {
        let guid = serde_json::to_value(RssItemRefDto::Guid {
            guid: "g".into(),
            fingerprint: "f".repeat(64),
        })
        .expect("ser");
        assert_eq!(
            guid,
            serde_json::json!({ "kind": "guid", "guid": "g", "fingerprint": "f".repeat(64) })
        );
        let title_link = serde_json::to_value(RssItemRefDto::TitleLink {
            title: "T".into(),
            link: None,
            fingerprint: "f".repeat(64),
        })
        .expect("ser");
        assert_eq!(title_link["kind"], "titleLink");
        assert_eq!(title_link["link"], serde_json::Value::Null);
        assert_eq!(title_link["fingerprint"], "f".repeat(64));
    }

    #[test]
    fn rss_item_ref_rejects_unknown_fields_kinds_and_a_missing_fingerprint() {
        assert!(serde_json::from_value::<RssItemRefDto>(serde_json::json!({
            "kind": "guid", "guid": "g", "fingerprint": "f", "extra": 1
        }))
        .is_err());
        assert!(serde_json::from_value::<RssItemRefDto>(serde_json::json!({
            "kind": "byIndex", "index": 0
        }))
        .is_err());
        // The previewed-content proof is REQUIRED on the way in.
        assert!(serde_json::from_value::<RssItemRefDto>(serde_json::json!({
            "kind": "guid", "guid": "g"
        }))
        .is_err());
    }

    fn exploitable_rss_analysis() -> crate::domain::import::RssAnalysis {
        crate::domain::import::parse_rss(
            "<rss version=\"2.0\"><channel><title>Flux</title>\
             <item><title>Episode</title><description>Texte.</description><guid>g-1</guid></item>\
             <item><description>Sans titre, avec enclosure.</description>\
             <enclosure url=\"https://exemple.fr/e.mp3\" length=\"1\" type=\"audio/mpeg\"/></item>\
             </channel></rss>"
                .as_bytes(),
        )
    }

    #[test]
    fn rss_preview_dto_wire_shape_is_camel_case_and_coherent() {
        let analysis = exploitable_rss_analysis();
        let dto = RssPreviewDto::from_analysis("exemple.fr".into(), &analysis);
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["sourceHost"], "exemple.fr");
        assert_eq!(v["state"], "needsReview");
        assert_eq!(v["blocked"], false);
        assert_eq!(v["items"][0]["title"], "Episode");
        assert_eq!(v["items"][0]["hasEnclosure"], false);
        assert_eq!(v["items"][0]["itemRef"]["kind"], "guid");
        assert_eq!(v["items"][1]["hasEnclosure"], true);
        assert_eq!(v["items"][1]["itemRef"]["kind"], "titleLink");
        // The flow findings carry the RSS copy with the nominal source pair.
        assert_eq!(v["findings"][2]["aspect"], "source");
        assert_eq!(v["findings"][2]["category"], "ambiguous");
        for snake in ["source_host", "has_enclosure", "item_ref"] {
            assert!(v.get(snake).is_none(), "{snake} must be camelCase");
        }
    }

    #[test]
    fn rss_preview_dto_marks_a_blocked_verdict() {
        let analysis = crate::domain::import::parse_rss(b"pas du xml");
        let dto = RssPreviewDto::from_analysis("exemple.fr".into(), &analysis);
        assert!(dto.blocked);
        assert!(dto.items.is_empty());
        assert_eq!(dto.state, ImportStateDto::Blocked);
        assert_eq!(
            dto.findings[0].message,
            "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux."
        );
    }

    #[test]
    fn rss_preview_summary_is_bounded_with_an_ellipsis() {
        assert_eq!(truncate_rss_summary("court"), "court");
        let long = "a".repeat(MAX_RSS_SUMMARY_CHARS + 50);
        let truncated = truncate_rss_summary(&long);
        assert_eq!(truncated.chars().count(), MAX_RSS_SUMMARY_CHARS + 1);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn rss_creation_outcome_wire_shapes_are_tagged_camel_case() {
        let created = RssCreationOutcomeDto::Created {
            story: crate::ipc::dto::StoryCardDto {
                id: "id-1".into(),
                title: "Episode".into(),
                import_state: Some(ImportStateDto::NeedsReview),
                import_report: None,
            },
            report: vec![ImportFindingDto {
                aspect: ImportAspectDto::Source,
                category: ImportCategoryDto::Ambiguous,
                message: "msg".into(),
            }],
        };
        let v = serde_json::to_value(&created).expect("ser");
        assert_eq!(v["kind"], "created");
        assert_eq!(v["story"]["id"], "id-1");
        assert_eq!(v["report"][0]["aspect"], "source");

        let changed = serde_json::to_value(RssCreationOutcomeDto::SourceChanged).expect("ser");
        assert_eq!(changed, serde_json::json!({ "kind": "sourceChanged" }));
    }

    #[test]
    fn rss_copy_speaks_of_the_feed_and_owns_its_media_pair() {
        // The RSS flow owns the wording of the pairs it emits…
        assert_eq!(
            rss_finding_message(ImportAspectDto::Envelope, ImportCategoryDto::Blocking),
            "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux."
        );
        assert_ne!(
            rss_finding_message(ImportAspectDto::Envelope, ImportCategoryDto::Blocking),
            finding_message(ImportAspectDto::Envelope, ImportCategoryDto::Blocking),
        );
        // …including its own `media` missing copy (the folder one speaks
        // of folder files, the RSS one of a remote enclosure).
        assert_eq!(
            rss_finding_message(ImportAspectDto::Media, ImportCategoryDto::Missing),
            "Le média distant référencé par la source n'a pas été récupéré. Ajoute le média manuellement dans l'éditeur."
        );
        assert_ne!(
            rss_finding_message(ImportAspectDto::Media, ImportCategoryDto::Missing),
            finding_message(ImportAspectDto::Media, ImportCategoryDto::Missing),
        );
        // The nominal source pair is the shared copy (defensive table and
        // living table agree byte-for-byte).
        assert_eq!(
            rss_finding_message(ImportAspectDto::Source, ImportCategoryDto::Ambiguous),
            finding_message(ImportAspectDto::Source, ImportCategoryDto::Ambiguous),
        );
    }

    #[test]
    fn rss_summary_round_trips_through_the_rss_renderer() {
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::ambiguous(RecognitionAspect::Source),
            RecognitionFinding {
                aspect: RecognitionAspect::Media,
                category: RecognitionCategory::Missing,
            },
        ];
        let summary = serialize_findings_summary(&findings).expect("summary");
        let report = rss_import_findings_from_summary(&summary);
        assert_eq!(report.len(), 3);
        assert!(report.iter().any(|f| f.aspect == ImportAspectDto::Source
            && f.message
                .starts_with("Contenu ingéré depuis une source externe (RSS).")));
        assert!(report.iter().any(|f| f.aspect == ImportAspectDto::Media
            && f.message.starts_with("Le média distant référencé")));
    }

    #[test]
    fn db_tag_round_trips_the_persistable_states() {
        for (state, tag, dto) in [
            (
                crate::domain::import::ImportState::Recognized,
                "recognized",
                ImportStateDto::Recognized,
            ),
            (
                crate::domain::import::ImportState::Partial,
                "partial",
                ImportStateDto::Partial,
            ),
            (
                crate::domain::import::ImportState::NeedsReview,
                "needs_review",
                ImportStateDto::NeedsReview,
            ),
            (
                crate::domain::import::ImportState::Resolved,
                "resolved",
                ImportStateDto::Resolved,
            ),
        ] {
            assert_eq!(state_db_tag(state), tag);
            assert_eq!(import_state_dto_from_tag(tag), Some(dto));
        }
        assert_eq!(import_state_dto_from_tag("garbage"), None);
        // `Blocked` is never persisted: the defensive fallback keeps it
        // inside the CHECK set rather than corrupting the row.
        assert_eq!(
            state_db_tag(crate::domain::import::ImportState::Blocked),
            "needs_review"
        );
    }
}
