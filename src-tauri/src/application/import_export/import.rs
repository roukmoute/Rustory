//! Local-artifact import application service — the inverse of `export`.
//!
//! Two phases, NO mutation before acceptance (AC1):
//!
//! 1. [`analyze_artifact`] — pure on already-read bytes: parse the
//!    `.rustory` v1 envelope (a parse failure / unknown field is a typed
//!    `blocked` verdict, never an `AppError`), classify every aspect, and
//!    carry the validated canonical content when importable. No DB, no FS.
//! 2. [`accept_import`] — re-validate the carried content FROM ZERO (never
//!    trusts the frontend), then commit one canonical `stories` row + one
//!    `story_local_imports` provenance row in a single `BEGIN IMMEDIATE`
//!    transaction (atomic — a failure leaves the library untouched).
//!
//! There is no filesystem stage: a `.rustory` is a small JSON file, so the
//! canonical commit is a pure SQLite insert (closer to `create_story` than
//! to the device-pack import). Only TRANSPORT failures (file unreadable, DB
//! write impossible) cross the boundary as `AppError` / `ImportFailed`.

use crate::application::story::now_iso_ms;
use crate::domain::export::{RustoryArtifactV1, RUSTORY_ARTIFACT_FORMAT_VERSION};
use crate::domain::import::{
    analyze_components, analyze_rustory_artifact, is_artifact_checksum,
    is_supported_artifact_source_name, ArtifactAnalysis, CanonicalContent, ImportState,
    ImportableContent, RecognitionFinding,
};
use crate::domain::shared::AppError;
use crate::domain::story::{
    content_checksum_bytes, map_error, normalize_title, validate_canonical, validate_title,
    CanonicalStoryFacts, CANONICAL_STORY_SCHEMA_VERSION,
};
use crate::infrastructure::db::DbHandle;
use crate::ipc::dto::import_export::{
    import_report_dto, serialize_findings_summary, state_db_tag, state_dto,
    AcceptArtifactImportInputDto,
};
use crate::ipc::dto::StoryCardDto;

use rusqlite::OptionalExtension;

/// The application-level outcome of analyzing artifact bytes: the typed
/// recognition verdict + the provenance metadata the accept phase needs.
/// The DTO layer maps it to [`ImportArtifactAnalysisDto::analyzed`].
///
/// [`ImportArtifactAnalysisDto::analyzed`]: crate::ipc::dto::ImportArtifactAnalysisDto::analyzed
#[derive(Debug, Clone)]
pub struct ImportAnalysis {
    pub analysis: ArtifactAnalysis,
    pub source_name: String,
    /// SHA-256 of the raw artifact bytes — the provenance fingerprint.
    pub artifact_checksum: String,
}

/// Provenance of a file-imported story, read back from
/// `story_local_imports`. Read-only; used by tests and any future
/// provenance surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalImportProvenance {
    pub source_format: String,
    pub source_format_version: u32,
    pub source_name: String,
    pub artifact_checksum: String,
    pub import_state: String,
    pub imported_at: String,
}

/// Phase 1 — classify already-read artifact bytes. PURE (no I/O), so it is
/// fully testable without a dialog. A `serde_json` parse failure (malformed
/// JSON, unknown field, missing field) is a typed `blocked` verdict, never
/// an `AppError`.
pub fn analyze_artifact(bytes: &[u8], source_name: String) -> ImportAnalysis {
    let artifact_checksum = content_checksum_bytes(bytes);
    // The explicitly-listed `.rustory` format is the AUTHORITY — never the UI
    // dialog filter alone. A source that is not a sober `.rustory` basename is
    // a blocked verdict BEFORE parsing (a non-`.rustory` file whose JSON would
    // happen to be compatible must not slip an implicit format through).
    let analysis = if !is_supported_artifact_source_name(&source_name) {
        ArtifactAnalysis::envelope_blocked()
    } else {
        match serde_json::from_slice::<RustoryArtifactV1>(bytes) {
            Ok(artifact) => analyze_rustory_artifact(&artifact),
            Err(_) => ArtifactAnalysis::envelope_blocked(),
        }
    };
    ImportAnalysis {
        analysis,
        source_name,
        artifact_checksum,
    }
}

/// Phase 2 — re-validate the carried content from ZERO and commit. Never
/// trusts the frontend: it re-runs the canonical title validation
/// (`INVALID_STORY_TITLE` on failure, exactly like `create_story`) and the
/// full canonical re-validation (`validate_canonical` + the checksum check,
/// via the SAME `analyze_components` used at analysis time), re-deriving the
/// durable state + attention findings from the re-validated content rather
/// than trusting the wire. Synchronous (the command hands it to
/// `spawn_blocking`), so no `MutexGuard` ever lives across an `await`.
pub fn accept_import(
    db: &mut DbHandle,
    input: &AcceptArtifactImportInputDto,
) -> Result<StoryCardDto, AppError> {
    let content = &input.content;

    // Provenance re-validation (defense in depth — `accept_artifact_import`
    // is a public IPC boundary): a direct call or a frontend drift must NEVER
    // persist an absolute path / PII as `source_name` nor a forged
    // fingerprint. The basename must be a sober `.rustory` name and the
    // checksum exactly `[0-9a-f]{64}`.
    if !is_supported_artifact_source_name(&input.source_name) {
        return Err(invalid_provenance_error("source_name"));
    }
    if !is_artifact_checksum(&input.artifact_checksum) {
        return Err(invalid_provenance_error("artifact_checksum"));
    }

    // Authoritative title re-validation — same canonical reason as the
    // creation dialog when it trips (`INVALID_STORY_TITLE`).
    let normalized = normalize_title(&content.title);
    validate_title(&normalized).map_err(map_error)?;

    // Full canonical re-validation: the carried structure must still parse,
    // its embedded `schemaVersion` must agree with the supported canonical
    // version, and `SHA-256(structureJson)` must equal the carried
    // `contentChecksum`. A defense-in-depth refusal of a frontend that
    // bypassed the verdict.
    let facts = CanonicalStoryFacts {
        title: normalized.clone(),
        schema_version: CANONICAL_STORY_SCHEMA_VERSION,
        structure_json: content.structure_json.clone(),
        content_checksum: content.content_checksum.clone(),
    };
    if !validate_canonical(&facts).is_empty() {
        return Err(revalidation_error());
    }

    // Re-derive the durable state + attention findings from the SAME logic
    // the analysis used (the envelope was already proven at analysis time →
    // pass the supported format version). Trusting the re-validated content,
    // not the wire.
    let canonical = CanonicalContent {
        title: content.title.clone(),
        schema_version: CANONICAL_STORY_SCHEMA_VERSION,
        structure_json: content.structure_json.clone(),
        content_checksum: content.content_checksum.clone(),
        created_at: content.created_at.clone(),
        updated_at: content.updated_at.clone(),
    };
    let analysis = analyze_components(RUSTORY_ARTIFACT_FORMAT_VERSION, &canonical);
    let importable = analysis.importable.ok_or_else(revalidation_error)?;

    commit_local_artifact_import(
        db,
        &importable,
        analysis.state,
        &analysis.findings,
        &input.source_name,
        &input.artifact_checksum,
    )
}

/// Insert the canonical `stories` row + the `story_local_imports`
/// provenance row in one `BEGIN IMMEDIATE` transaction. The canonical row
/// PRESERVES the artifact's `created_at` / `updated_at` (a re-openable
/// imported story keeps its history); `imported_at = now`. Atomic: any
/// failure rolls back both inserts, leaving the library untouched.
fn commit_local_artifact_import(
    db: &mut DbHandle,
    content: &ImportableContent,
    state: ImportState,
    findings: &[RecognitionFinding],
    source_name: &str,
    artifact_checksum: &str,
) -> Result<StoryCardDto, AppError> {
    let story_id = uuid::Uuid::now_v7().to_string();
    // Keep the commit failure inside the `IMPORT_FAILED` closed taxonomy (§6):
    // wrap the (theoretical) system-clock failure of `now_iso_ms` rather than
    // letting its `LOCAL_STORAGE_UNAVAILABLE` leak out of the accept boundary
    // with a `details.source` outside the import set.
    let now_iso = now_iso_ms().map_err(|_| clock_unavailable_error())?;
    let findings_summary = serialize_findings_summary(findings);
    // Store the CANONICAL (normalized) title — the artifact's verbatim title
    // is preserved only long enough to derive the normalization ambiguity;
    // the stored row is always normalized, exactly like `create_story`.
    let title = normalize_title(&content.title);

    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| db_commit_error(&err, "begin_transaction"))?;

    tx.execute(
        "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            &story_id,
            &title,
            CANONICAL_STORY_SCHEMA_VERSION,
            &content.structure_json,
            &content.content_checksum,
            &content.created_at,
            &content.updated_at,
        ],
    )
    .map_err(|err| db_commit_error(&err, "insert_story"))?;

    tx.execute(
        "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
         VALUES (?1, 'rustory', ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            &story_id,
            RUSTORY_ARTIFACT_FORMAT_VERSION,
            source_name,
            artifact_checksum,
            state_db_tag(state),
            &findings_summary,
            &now_iso,
        ],
    )
    .map_err(|err| db_commit_error(&err, "insert_provenance"))?;

    tx.commit().map_err(|err| db_commit_error(&err, "commit"))?;

    let import_report = import_report_dto(findings);
    Ok(StoryCardDto {
        id: story_id,
        title,
        import_state: Some(state_dto(state)),
        import_report: if import_report.is_empty() {
            None
        } else {
            Some(import_report)
        },
    })
}

/// Read the file-import provenance for a story (read-only). `Ok(None)` for
/// a native story (no provenance row).
pub fn read_local_import_provenance(
    db: &DbHandle,
    story_id: &str,
) -> Result<Option<LocalImportProvenance>, AppError> {
    db.conn()
        .query_row(
            "SELECT source_format, source_format_version, source_name, artifact_checksum, import_state, imported_at \
             FROM story_local_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |row| {
                Ok(LocalImportProvenance {
                    source_format: row.get(0)?,
                    source_format_version: row.get(1)?,
                    source_name: row.get(2)?,
                    artifact_checksum: row.get(3)?,
                    import_state: row.get(4)?,
                    imported_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|err| read_provenance_error(&err))
}

// ===== Closed user-facing copy — sober, no OS message, no path (PII). =====

/// A READ of the provenance row failed. Distinct from [`db_commit_error`]
/// (a WRITE): a neutral read message + the canonical local-storage read
/// taxonomy (`LOCAL_STORAGE_UNAVAILABLE` / `sqlite_select`), exactly like
/// the other read-only paths (`get_story_detail`, `read_stories`) — never
/// the `IMPORT_FAILED` / `db_commit` "enregistrement refusé" write copy.
fn read_provenance_error(_err: &rusqlite::Error) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu relire la provenance d'import de cette histoire.",
        "Relance l'application ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_select",
        "table": "story_local_imports",
    }))
}

/// File read failed (unreadable, oversize, metadata). The command layer
/// tags the precise `stage`.
pub fn file_read_error(stage: &'static str) -> AppError {
    AppError::import_failed(
        "Import impossible: fichier illisible.",
        "Vérifie que le fichier existe, qu'il s'agit bien d'un artefact Rustory, puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "file_read",
        "stage": stage,
    }))
}

/// The managed local store has no resolvable home.
pub fn app_data_unavailable_error() -> AppError {
    AppError::import_failed(
        "Import impossible: stockage local introuvable.",
        "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
    )
    .with_details(serde_json::json!({ "source": "app_data_unavailable" }))
}

/// The native file dialog backend could not open.
pub fn dialog_failed_error() -> AppError {
    AppError::import_failed(
        "Import impossible: la fenêtre de sélection n'a pas pu s'ouvrir.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "dialog_failed" }))
}

/// The blocking worker task could not be joined.
pub fn spawn_blocking_join_error() -> AppError {
    AppError::import_failed(
        "Import interrompu de façon inattendue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({ "source": "spawn_blocking_join" }))
}

/// Defensive: the accepted provenance is not a sober `.rustory` basename or
/// a `[0-9a-f]{64}` fingerprint (a direct call / frontend drift). Nothing is
/// committed. `field` names which provenance field was rejected — never the
/// rejected value itself (it could be an absolute path / PII).
fn invalid_provenance_error(field: &'static str) -> AppError {
    AppError::import_failed(
        "Import impossible: informations de provenance invalides.",
        "Relance l'analyse de l'artefact puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "cause": "invalid_provenance",
        "field": field,
    }))
}

/// The system clock could not produce an `imported_at` timestamp (a
/// theoretical clock fault). Kept inside the `IMPORT_FAILED` closed
/// taxonomy so every `accept` failure carries a closed `details.source`.
fn clock_unavailable_error() -> AppError {
    AppError::import_failed(
        "Import impossible: l'horloge système est indisponible.",
        "Vérifie la date et l'heure de ton ordinateur puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "cause": "system_clock_invalid",
    }))
}

/// Defensive: the accepted content failed the from-zero re-validation (a
/// frontend that bypassed the verdict). Nothing is committed.
fn revalidation_error() -> AppError {
    AppError::import_failed(
        "Import impossible: le contenu reçu n'a pas pu être revalidé.",
        "Relance l'analyse de l'artefact puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "other", "cause": "revalidation" }))
}

fn db_commit_error(err: &rusqlite::Error, stage: &'static str) -> AppError {
    // PII discipline: drop the raw rusqlite message, keep a stable
    // stage + coarse kind.
    let kind = match err {
        rusqlite::Error::SqliteFailure(code, _) => match code.code {
            rusqlite::ErrorCode::ConstraintViolation => "constraint_violation",
            rusqlite::ErrorCode::DatabaseBusy => "busy",
            rusqlite::ErrorCode::DatabaseLocked => "locked",
            _ => "other",
        },
        _ => "other",
    };
    AppError::import_failed(
        "Import impossible: enregistrement local refusé.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "db_commit",
        "stage": stage,
        "kind": kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::get_story_detail;
    use crate::domain::export::{ArtifactEnvelopeV1, ExportedStoryV1};
    use crate::domain::import::{ImportState, RecognitionQuality};
    use crate::domain::shared::AppErrorCode;
    use crate::domain::story::content_checksum;
    use crate::infrastructure::db;
    use crate::ipc::dto::import_export::{ImportCategoryDto, ImportableContentDto};

    const CANONICAL_STRUCTURE: &str = "{\"schemaVersion\":1,\"nodes\":[]}";

    fn fresh_db() -> DbHandle {
        let mut db = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut db).expect("migrate");
        db
    }

    fn clean_artifact_bytes(title: &str) -> Vec<u8> {
        let artifact = RustoryArtifactV1 {
            rustory_artifact: ArtifactEnvelopeV1 {
                format_version: RUSTORY_ARTIFACT_FORMAT_VERSION,
                exported_at: "2026-06-27T10:00:00.000Z".into(),
                exported_by: "rustory/0.1.0".into(),
            },
            story: ExportedStoryV1 {
                schema_version: 1,
                title: title.into(),
                structure_json: CANONICAL_STRUCTURE.into(),
                content_checksum: content_checksum(CANONICAL_STRUCTURE),
                created_at: "2026-06-20T10:00:00.000Z".into(),
                updated_at: "2026-06-24T14:15:00.000Z".into(),
            },
        };
        artifact.to_canonical_json().expect("serialize")
    }

    fn accept_input_from(analysis: &ImportAnalysis) -> AcceptArtifactImportInputDto {
        let ImportArtifactAnalysisAnalyzed {
            importable_content,
            source_name,
            artifact_checksum,
        } = expect_analyzed(analysis);
        AcceptArtifactImportInputDto {
            content: importable_content,
            source_name,
            artifact_checksum,
        }
    }

    struct ImportArtifactAnalysisAnalyzed {
        importable_content: ImportableContentDto,
        source_name: String,
        artifact_checksum: String,
    }

    fn expect_analyzed(analysis: &ImportAnalysis) -> ImportArtifactAnalysisAnalyzed {
        ImportArtifactAnalysisAnalyzed {
            importable_content: ImportableContentDto::from_domain(
                analysis
                    .analysis
                    .importable
                    .as_ref()
                    .expect("importable content present"),
            ),
            source_name: analysis.source_name.clone(),
            artifact_checksum: analysis.artifact_checksum.clone(),
        }
    }

    #[test]
    fn analyze_pure_on_bytes_recognizes_a_clean_artifact() {
        let analysis =
            analyze_artifact(&clean_artifact_bytes("Le Soleil"), "soleil.rustory".into());
        assert_eq!(analysis.analysis.quality, RecognitionQuality::Clean);
        assert_eq!(analysis.analysis.state, ImportState::Recognized);
        assert_eq!(analysis.source_name, "soleil.rustory");
        assert_eq!(analysis.artifact_checksum.len(), 64);
        assert!(analysis.analysis.importable.is_some());
    }

    #[test]
    fn analyze_maps_a_parse_failure_to_a_blocked_verdict_not_an_error() {
        let analysis = analyze_artifact(b"{ this is not json", "broken.rustory".into());
        assert_eq!(analysis.analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(analysis.analysis.state, ImportState::Blocked);
        assert!(analysis.analysis.importable.is_none());
    }

    #[test]
    fn analyze_maps_an_unknown_field_to_a_blocked_verdict() {
        // `deny_unknown_fields` refuses an extra envelope field — a typed
        // blocked verdict, never an AppError.
        let json = br#"{
            "rustoryArtifact": { "formatVersion": 1, "exportedAt": "2026-06-27T10:00:00.000Z", "exportedBy": "x", "surprise": "f" },
            "story": { "schemaVersion": 1, "title": "t", "structureJson": "{}", "contentChecksum": "0000000000000000000000000000000000000000000000000000000000000000", "createdAt": "2026-06-20T10:00:00.000Z", "updatedAt": "2026-06-24T14:15:00.000Z" }
        }"#;
        let analysis = analyze_artifact(json, "x.rustory".into());
        assert_eq!(analysis.analysis.state, ImportState::Blocked);
    }

    #[test]
    fn accept_commits_a_canonical_story_and_provenance_in_one_transaction() {
        let mut db = fresh_db();
        let analysis =
            analyze_artifact(&clean_artifact_bytes("Mon Histoire"), "mon.rustory".into());
        let card = accept_import(&mut db, &accept_input_from(&analysis)).expect("accept");

        assert_eq!(card.title, "Mon Histoire");
        assert!(card.import_state.is_some());

        // The story is re-openable WITHOUT the artifact, with PRESERVED
        // timestamps (fidelity of the AC3 re-openable story).
        let detail = get_story_detail(&db, &card.id)
            .expect("read detail")
            .expect("row present");
        assert_eq!(detail.title, "Mon Histoire");
        assert_eq!(detail.schema_version, 1);
        assert_eq!(detail.structure_json, CANONICAL_STRUCTURE);
        assert_eq!(detail.created_at, "2026-06-20T10:00:00.000Z");
        assert_eq!(detail.updated_at, "2026-06-24T14:15:00.000Z");

        // Provenance row.
        let provenance = read_local_import_provenance(&db, &card.id)
            .expect("read provenance")
            .expect("provenance present");
        assert_eq!(provenance.source_format, "rustory");
        assert_eq!(provenance.source_format_version, 1);
        assert_eq!(provenance.source_name, "mon.rustory");
        assert_eq!(provenance.artifact_checksum.len(), 64);
        assert_eq!(provenance.import_state, "recognized");
    }

    #[test]
    fn analyze_alone_never_mutates_the_library() {
        let mut db = fresh_db();
        let _ = analyze_artifact(&clean_artifact_bytes("Analyse seule"), "a.rustory".into());
        // Analysis is pure on bytes — it never touched `db`. Prove the
        // library is still empty (AC1: no mutation before acceptance).
        let count: u32 = db
            .conn_mut()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 0);
        let imports: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM story_local_imports", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(imports, 0);
    }

    #[test]
    fn accept_a_non_normalized_title_stores_needs_review_with_a_findings_summary() {
        let mut db = fresh_db();
        let analysis = analyze_artifact(
            &clean_artifact_bytes("  Titre à espaces  "),
            "spaced.rustory".into(),
        );
        assert_eq!(analysis.analysis.state, ImportState::NeedsReview);
        let card = accept_import(&mut db, &accept_input_from(&analysis)).expect("accept");

        // The stored title is the NORMALIZED form; the durable state is
        // needs_review with a FULL report (recognized + attention).
        assert_eq!(card.title, "Titre à espaces");
        let report = card.import_report.expect("durable report present");
        // The full report carries every aspect (recognized + the ambiguity).
        assert!(report
            .iter()
            .any(|f| f.category == ImportCategoryDto::Ambiguous));
        assert!(report
            .iter()
            .any(|f| f.category == ImportCategoryDto::Recognized));

        let provenance = read_local_import_provenance(&db, &card.id)
            .expect("read")
            .expect("present");
        assert_eq!(provenance.import_state, "needs_review");
    }

    #[test]
    fn accept_rejects_a_falsified_checksum_via_from_zero_revalidation() {
        let mut db = fresh_db();
        let analysis = analyze_artifact(&clean_artifact_bytes("Falsifiée"), "f.rustory".into());
        let mut input = accept_input_from(&analysis);
        // Tamper the carried checksum so it no longer matches the structure.
        input.content.content_checksum = "0".repeat(64);

        let err = accept_import(&mut db, &input).expect_err("revalidation must reject");
        assert_eq!(err.code, AppErrorCode::ImportFailed);
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["cause"], "revalidation");
        // Nothing was committed.
        let count: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn accept_rejects_a_non_basename_source_name_without_committing() {
        let mut db = fresh_db();
        let analysis = analyze_artifact(&clean_artifact_bytes("Provenance"), "p.rustory".into());
        let mut input = accept_input_from(&analysis);
        // A direct call / frontend drift tries to persist an absolute path
        // (PII) as the provenance source name.
        input.source_name = "/home/user/secret.rustory".into();
        let err = accept_import(&mut db, &input).expect_err("absolute path must be refused");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["details"]["cause"], "invalid_provenance");
        assert_eq!(v["details"]["field"], "source_name");
        let count: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM story_local_imports", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(count, 0, "no provenance row may be persisted");
    }

    #[test]
    fn accept_rejects_a_forged_artifact_checksum() {
        let mut db = fresh_db();
        let analysis = analyze_artifact(&clean_artifact_bytes("Empreinte"), "e.rustory".into());
        let mut input = accept_input_from(&analysis);
        input.artifact_checksum = "NOT-A-HEX-DIGEST".into();
        let err = accept_import(&mut db, &input).expect_err("forged checksum must be refused");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["cause"], "invalid_provenance");
        assert_eq!(v["details"]["field"], "artifact_checksum");
    }

    #[test]
    fn analyze_blocks_a_non_rustory_source_before_parsing() {
        // A non-`.rustory` file whose JSON is a perfectly valid artifact must
        // STILL be blocked — the explicit format is the authority, not the
        // dialog filter.
        let bytes = clean_artifact_bytes("Bien formée mais mauvaise extension");
        let analysis = analyze_artifact(&bytes, "histoire.txt".into());
        assert_eq!(analysis.analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(analysis.analysis.state, ImportState::Blocked);
        assert!(analysis.analysis.importable.is_none());
    }

    #[test]
    fn accept_rolls_back_atomically_when_the_provenance_insert_fails() {
        // §8 atomicity, LOCKED by a fault injection: sabotage the SECOND insert
        // (`story_local_imports`) so the commit fails AFTER the `stories`
        // insert ran inside the same transaction — the whole transaction must
        // roll back, leaving no half-imported story.
        let mut db = fresh_db();
        db.conn()
            .execute_batch(
                "CREATE TRIGGER sabotage_provenance BEFORE INSERT ON story_local_imports \
                 BEGIN SELECT RAISE(ABORT, 'sabotage'); END;",
            )
            .expect("install sabotage trigger");

        let analysis = analyze_artifact(&clean_artifact_bytes("Atomique"), "a.rustory".into());
        let err = accept_import(&mut db, &accept_input_from(&analysis))
            .expect_err("the provenance insert must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "db_commit");
        assert_eq!(v["details"]["stage"], "insert_provenance");

        // The `stories` insert is rolled back with the transaction — atomicity.
        let stories: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count stories");
        assert_eq!(stories, 0, "no half-imported story may survive");
        let imports: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM story_local_imports", [], |row| {
                row.get(0)
            })
            .expect("count imports");
        assert_eq!(imports, 0);
    }

    #[test]
    fn accept_rejects_an_invalid_title_with_invalid_story_title() {
        let mut db = fresh_db();
        let analysis = analyze_artifact(&clean_artifact_bytes("Bonne"), "g.rustory".into());
        let mut input = accept_input_from(&analysis);
        // A blank title slipped past phase 1 (a buggy/hostile frontend).
        input.content.title = "   ".into();
        let err = accept_import(&mut db, &input).expect_err("must reject blank title");
        assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);
    }

    #[test]
    fn import_does_not_touch_existing_stories_fr18() {
        let mut db = fresh_db();
        // Seed a native story.
        let native = crate::application::story::create_story(
            &mut db,
            crate::application::story::CreateStoryInput {
                title: "Native intacte".into(),
            },
        )
        .expect("create native");
        let before = get_story_detail(&db, &native.id)
            .expect("read")
            .expect("present");

        let analysis = analyze_artifact(&clean_artifact_bytes("Importée"), "i.rustory".into());
        accept_import(&mut db, &accept_input_from(&analysis)).expect("accept");

        let after = get_story_detail(&db, &native.id)
            .expect("read")
            .expect("present");
        assert_eq!(before.title, after.title);
        assert_eq!(before.updated_at, after.updated_at);
        assert_eq!(before.content_checksum, after.content_checksum);
    }

    #[test]
    fn native_story_has_no_local_import_provenance() {
        let mut db = fresh_db();
        let native = crate::application::story::create_story(
            &mut db,
            crate::application::story::CreateStoryInput {
                title: "Native".into(),
            },
        )
        .expect("create");
        let provenance = read_local_import_provenance(&db, &native.id).expect("read");
        assert!(provenance.is_none());
    }

    /// Every import-refusal constructor must be ACTIONABLE — a non-empty
    /// cause AND a non-empty next gesture — so the UI never surfaces an
    /// opaque refusal (calque of `every_import_refusal_constructor_is_actionable`).
    #[test]
    fn every_local_import_refusal_constructor_is_actionable() {
        let sqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let refusals = [
            file_read_error("read"),
            app_data_unavailable_error(),
            dialog_failed_error(),
            spawn_blocking_join_error(),
            revalidation_error(),
            invalid_provenance_error("source_name"),
            clock_unavailable_error(),
            db_commit_error(&sqlite_err, "insert_story"),
        ];
        for err in &refusals {
            assert_eq!(err.code, AppErrorCode::ImportFailed, "{err:?}");
            assert!(!err.message.is_empty(), "refusal needs a cause: {err:?}");
            let action = err.user_action.as_deref().unwrap_or("");
            assert!(!action.is_empty(), "refusal needs a next gesture: {err:?}");
            // PII discipline: no absolute path leaks into the wire payload.
            let v = serde_json::to_value(err).expect("ser");
            assert!(v["details"]["source"].is_string());
        }
    }
}
