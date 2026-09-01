//! Shared support of the two story-creation flows (RSS feed and non-RSS web
//! page): ONE implementation of the closed « création » error copies they
//! share, and of the content-source policy gate.
//!
//! The copies are the closed « création » copies — identical word for word
//! in both flows. Where the flows genuinely diverge (report copy, source
//! format) the per-flow closed-copy discipline is kept in each flow.

use crate::domain::import::{
    content_source_activation, ContentSourceActivation, ContentSourceKind, ContentSourceLine,
};
use crate::domain::shared::AppError;

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
