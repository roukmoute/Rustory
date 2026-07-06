use serde::{Deserialize, Serialize};

use crate::domain::import::{
    ArtifactAnalysis, ImportState, ImportableContent, RecognitionAspect, RecognitionCategory,
    RecognitionFinding, RecognitionQuality,
};

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

/// The aspect of the artifact a finding refers to. Mirror of the domain
/// [`RecognitionAspect`].
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
}

/// Single canonical FR copy per `(aspect, category)` — never two wordings
/// for one pair. Mirrors `docs/architecture/ui-states.md#Local Artifact
/// Import Contract`.
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
    serde_json::from_str::<Vec<StoredImportFinding>>(summary)
        .unwrap_or_default()
        .into_iter()
        .map(|stored| ImportFindingDto {
            aspect: stored.aspect,
            category: stored.category,
            message: finding_message(stored.aspect, stored.category).to_string(),
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
        ];
        let categories = [Recognized, Ambiguous, Missing, Blocking];
        for aspect in aspects {
            for category in categories {
                assert!(
                    !finding_message(aspect, category).is_empty(),
                    "{aspect:?}/{category:?} message empty"
                );
            }
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
