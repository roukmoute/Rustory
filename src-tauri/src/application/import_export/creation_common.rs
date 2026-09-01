//! Shared support of the two story-creation flows (RSS feed and non-RSS web
//! page): ONE implementation of the closed « création » error copies they
//! share, and of the content-source policy gate.
//!
//! The copies are the closed « création » copies — identical word for word
//! in both flows. Where the flows genuinely diverge (report copy, source
//! format) the per-flow closed-copy discipline is kept in each flow.

use crate::domain::import::{
    content_source_activation, ContentSourceActivation, ContentSourceKind, ContentSourceLine,
    ImportState, RecognitionFinding,
};
use crate::domain::shared::AppError;
use crate::domain::story::CANONICAL_STORY_SCHEMA_VERSION;
use crate::infrastructure::db::DbHandle;
use crate::ipc::dto::import_export::{
    serialize_findings_summary, state_db_tag, state_dto, ImportFindingDto,
};
use crate::ipc::dto::StoryCardDto;

/// The content-source policy gate, consulted by BOTH facades of each flow
/// (preview AND accept) BEFORE the address validation and BEFORE any
/// network dispatch: a policy refusal never produces a byte of traffic.
/// The matrix travels as a parameter — the commands hand
/// `official_content_sources()`, tests inject custom distributions — so
/// the policy stays consulted in one place per flow. Fail-closed: a kind
/// missing from the received matrix refuses exactly like a `NotActivated`
/// one (the gate never enables by default).
pub(crate) fn ensure_source_enabled(
    sources: &[ContentSourceLine],
    kind: ContentSourceKind,
) -> Result<(), AppError> {
    match content_source_activation(sources, kind) {
        ContentSourceActivation::Enabled => Ok(()),
        ContentSourceActivation::NotActivated | ContentSourceActivation::BlockedByPolicy => {
            Err(AppError::content_source_unavailable(kind))
        }
    }
}

/// The creation flows' closed copy of a db commit failure: a stable
/// diagnostics shape (PII discipline: the raw rusqlite message is dropped
/// — it can embed table names or filesystem detail — and kept as a stable
/// stage + a coarse kind).
pub(crate) fn db_commit_error(err: &rusqlite::Error, stage: &'static str) -> AppError {
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
        "Création impossible: enregistrement local refusé.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "db_commit",
        "stage": stage,
        "kind": kind,
    }))
}

/// The creation flows' closed copy of a spawn_blocking join failure
/// (command layer).
pub fn spawn_blocking_join_error() -> AppError {
    AppError::import_failed(
        "Création interrompue de façon inattendue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({ "source": "spawn_blocking_join" }))
}

/// ONE promoted media: everything its `assets` row needs, plus the
/// promoted file path so a failed commit can compensate the store.
#[derive(Debug)]
pub(crate) struct PromotedAsset {
    pub(crate) asset_id: String,
    pub(crate) content_hash: String,
    pub(crate) media_type: &'static str,
    pub(crate) media_format: &'static str,
    pub(crate) byte_size: u64,
    pub(crate) file_name: String,
    pub(crate) promoted_path: std::path::PathBuf,
}

/// Everything the atomic commit transaction needs — proven WITHOUT any DB
/// access by the prepare phase, so the network fetch never serializes
/// other commands behind the DB lock.
#[derive(Debug)]
pub(crate) struct StoryCreationCommit {
    pub(crate) title: String,
    pub(crate) structure_json: String,
    pub(crate) checksum: String,
    pub(crate) now_iso: String,
    /// The provenance `source_name` — the host in both flows.
    pub(crate) source_name: String,
    pub(crate) artifact_checksum: String,
    pub(crate) state: ImportState,
    pub(crate) findings: Vec<RecognitionFinding>,
}

/// Phase 2b — the single atomic transaction (`stories` + the provenance
/// row + every promoted media's `assets` row), shared by BOTH creation
/// flows: same SQL, same stages, same P1 guardrail. The flow identity
/// travels as parameters — `source_format` ('web'/'rss'), its format
/// version, and the flow's closed-copy report function. A failed
/// transaction rolls back fully; the caller compensates the promoted
/// files ([`compensate_promoted_assets`]).
pub(crate) fn commit_story_creation(
    db: &mut DbHandle,
    commit: &StoryCreationCommit,
    source_format: &str,
    source_format_version: u64,
    assets: &[PromotedAsset],
    import_report: fn(&[RecognitionFinding]) -> Vec<ImportFindingDto>,
) -> Result<StoryCardDto, AppError> {
    let StoryCreationCommit {
        title,
        structure_json,
        checksum,
        now_iso,
        source_name,
        artifact_checksum,
        state,
        findings,
    } = commit;
    let findings_summary = serialize_findings_summary(findings);
    let story_id = uuid::Uuid::now_v7().to_string();

    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| db_commit_error(&err, "begin_transaction"))?;
    tx.execute(
        "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![
            &story_id,
            title,
            CANONICAL_STORY_SCHEMA_VERSION,
            structure_json,
            checksum,
            now_iso,
        ],
    )
    .map_err(|err| db_commit_error(&err, "insert_story"))?;
    for asset in assets {
        tx.execute(
            "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                &asset.asset_id,
                &story_id,
                &asset.content_hash,
                asset.media_type,
                asset.media_format,
                asset.byte_size,
                &asset.file_name,
                now_iso,
            ],
        )
        .map_err(|err| db_commit_error(&err, "insert_asset"))?;
    }
    tx.execute(
        "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            &story_id,
            source_format,
            source_format_version,
            source_name,
            artifact_checksum,
            state_db_tag(*state),
            &findings_summary,
            now_iso,
        ],
    )
    .map_err(|err| db_commit_error(&err, "insert_provenance"))?;
    // Persist → VERIFY → report (the P1 guardrail): re-read both rows
    // INSIDE the transaction before composing the success DTO — a success
    // is never composed from data that was not proven committed-to-be.
    let verified: (String, String) = tx
        .query_row(
            "SELECT s.title, li.import_state FROM stories s \
             JOIN story_local_imports li ON li.story_id = s.id \
             WHERE s.id = ?1 AND li.source_format = ?2",
            rusqlite::params![&story_id, source_format],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|err| db_commit_error(&err, "verify_rows"))?;
    if verified.0 != *title || verified.1 != state_db_tag(*state) {
        return Err(db_commit_error(
            &rusqlite::Error::QueryReturnedNoRows,
            "verify_rows",
        ));
    }
    tx.commit().map_err(|err| db_commit_error(&err, "commit"))?;

    let report = import_report(findings);
    Ok(StoryCardDto {
        id: story_id,
        title: title.clone(),
        import_state: Some(state_dto(*state)),
        import_report: if report.is_empty() {
            None
        } else {
            Some(report)
        },
        transferable: false,
        sendable_archive: false,
        cover_asset_id: None,
    })
}

/// Best-effort compensation of the promoted files after a FAILED commit
/// transaction: the transaction left nothing in the DB, the promoted
/// files are the only remnants — a leftover is only a content-addressed
/// orphan, never a corruption.
pub(crate) fn compensate_promoted_assets(assets: &[PromotedAsset]) {
    for asset in assets {
        let _ = std::fs::remove_file(&asset.promoted_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;

    fn sqlite_failure(code: rusqlite::ErrorCode) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code,
                extended_code: 0,
            },
            None,
        )
    }

    #[test]
    fn spawn_blocking_join_error_uses_the_creation_closed_copy() {
        let err = spawn_blocking_join_error();
        assert_eq!(err.code, AppErrorCode::ImportFailed);
        assert_eq!(err.message, "Création interrompue de façon inattendue.");
        assert_eq!(
            err.user_action.as_deref(),
            Some("Réessaie ; si le problème persiste, redémarre Rustory.")
        );
        let value = serde_json::to_value(&err).expect("serialize");
        assert_eq!(value["details"]["source"], "spawn_blocking_join");
    }

    #[test]
    fn db_commit_error_uses_the_creation_closed_copy_and_a_stable_stage() {
        let err = db_commit_error(&sqlite_failure(rusqlite::ErrorCode::DatabaseBusy), "commit");
        assert_eq!(err.code, AppErrorCode::ImportFailed);
        assert_eq!(
            err.message,
            "Création impossible: enregistrement local refusé."
        );
        let value = serde_json::to_value(&err).expect("serialize");
        assert_eq!(value["details"]["source"], "db_commit");
        assert_eq!(value["details"]["stage"], "commit");
        assert_eq!(value["details"]["kind"], "busy");
    }

    #[test]
    fn db_commit_error_maps_each_sqlite_failure_to_its_coarse_kind() {
        let cases = [
            (rusqlite::ErrorCode::ConstraintViolation, "constraint_violation"),
            (rusqlite::ErrorCode::DatabaseBusy, "busy"),
            (rusqlite::ErrorCode::DatabaseLocked, "locked"),
            (rusqlite::ErrorCode::DiskFull, "other"),
        ];
        for (code, expected) in cases {
            let err = db_commit_error(&sqlite_failure(code), "insert_story");
            let value = serde_json::to_value(&err).expect("serialize");
            assert_eq!(value["details"]["kind"], expected, "{code:?}");
        }
    }

    #[test]
    fn db_commit_error_maps_non_sqlite_failures_to_other() {
        let err = db_commit_error(&rusqlite::Error::InvalidQuery, "insert_story");
        let value = serde_json::to_value(&err).expect("serialize");
        assert_eq!(value["details"]["kind"], "other");
    }

    #[test]
    fn ensure_source_enabled_rejects_a_not_activated_kind() {
        let sources = vec![ContentSourceLine {
            kind: ContentSourceKind::Web,
            activation: ContentSourceActivation::NotActivated,
        }];
        let err =
            ensure_source_enabled(&sources, ContentSourceKind::Web).expect_err("must reject");
        assert_eq!(err.code, AppErrorCode::ContentSourceUnavailable);
    }

    #[test]
    fn ensure_source_enabled_rejects_a_policy_blocked_kind() {
        let sources = vec![ContentSourceLine {
            kind: ContentSourceKind::Rss,
            activation: ContentSourceActivation::BlockedByPolicy,
        }];
        let err =
            ensure_source_enabled(&sources, ContentSourceKind::Rss).expect_err("must reject");
        assert_eq!(err.code, AppErrorCode::ContentSourceUnavailable);
    }

    #[test]
    fn ensure_source_enabled_allows_an_enabled_kind() {
        let sources = vec![ContentSourceLine {
            kind: ContentSourceKind::Web,
            activation: ContentSourceActivation::Enabled,
        }];
        assert!(ensure_source_enabled(&sources, ContentSourceKind::Web).is_ok());
    }

    #[test]
    fn ensure_source_enabled_is_fail_closed_for_a_missing_kind() {
        let sources = vec![ContentSourceLine {
            kind: ContentSourceKind::Rss,
            activation: ContentSourceActivation::Enabled,
        }];
        let err =
            ensure_source_enabled(&sources, ContentSourceKind::Web).expect_err("must reject");
        assert_eq!(err.code, AppErrorCode::ContentSourceUnavailable);
    }
}
