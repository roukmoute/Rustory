//! Persisted transfer-outcome domain model (pure, framework-free).
//!
//! The durable cross-session memory of a transfer's LAST terminal outcome (the
//! `transfer_jobs` table). Pure rules only: the closed set of PERSISTABLE
//! terminals, the mapping FROM a job terminal (a write-phase `(cause, completeness)`,
//! a `verify` verdict, or a `verified` summary) INTO the persisted form, and the
//! coherence invariant mirroring the `JobFailedEvent` F6 guard (a verify terminal
//! carries ONLY `verify_verdict`, never a write-phase `cause` / `completeness`).
//!
//! No serde, no infrastructure: the persistence adapter maps this to a SQLite row,
//! the IPC layer maps it to a wire DTO.

use super::{
    failure_copy, TransferCompleteness, TransferFailureCause, VerifiedSummary, VerifyVerdict,
};

/// Closed set of PERSISTABLE transfer terminals — the only outcomes durable across
/// an app restart. Every in-flight phase is excluded by construction (persisting a
/// `transferring` / `verifying` phase would be a lie after a restart: the job died
/// with the app).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedTerminalKind {
    /// `transférée et vérifiée` — the write landed and `verify` confirmed it.
    Verified,
    /// `état partiel` — `verify` found the pack present but incoherent.
    Partial,
    /// `échec récupérable` — a write-phase refusal (device intact) OR a `verify`
    /// that could not confirm (device gone / unreadable).
    Retryable,
    /// `transfert incomplet` — the write started then was interrupted (device
    /// mutated; a possible partial copy).
    Incomplete,
}

impl PersistedTerminalKind {
    /// Stable lowercase wire tag persisted in `transfer_jobs.terminal_kind` and
    /// mirrored by the `TransferTerminalKindDto` discriminant.
    pub const fn wire_tag(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Partial => "partial",
            Self::Retryable => "retryable",
            Self::Incomplete => "incomplete",
        }
    }

    /// Parse the [`wire_tag`](Self::wire_tag) back into the closed kind — the
    /// inverse used when re-hydrating a stored row. `None` for any value outside
    /// the set (also guaranteed by the `CHECK` constraint, but re-validated here
    /// so a corrupt row degrades to "no memory" rather than a panic).
    pub fn from_wire_tag(tag: &str) -> Option<Self> {
        match tag {
            "verified" => Some(Self::Verified),
            "partial" => Some(Self::Partial),
            "retryable" => Some(Self::Retryable),
            "incomplete" => Some(Self::Incomplete),
            _ => None,
        }
    }
}

/// A transfer terminal captured for durable cross-session memory. A pure record:
/// the persistence adapter writes it to / reads it from `transfer_jobs`, and the
/// IPC layer maps it to the wire DTO the panel re-hydrates from.
///
/// Coherence (mirrors `JobFailedEvent` F6): `verify_verdict` is mutually exclusive
/// with `cause` / `completeness`; a `summary` is present iff the kind is `Verified`.
/// The constructors build coherent values by construction; [`is_coherent`] guards
/// a reconstruction from stored columns.
///
/// [`is_coherent`]: PersistedTransferOutcome::is_coherent
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedTransferOutcome {
    pub terminal_kind: PersistedTerminalKind,
    /// Write-phase closed cause (`Some` for a write-phase terminal; `None` for a
    /// verify terminal and for `verified`).
    pub cause: Option<TransferFailureCause>,
    /// Write-phase device completeness (`Some` for a write-phase terminal; `None`
    /// otherwise).
    pub completeness: Option<TransferCompleteness>,
    /// `verify`-phase verdict (`Some(Partial)` / `Some(Failed)` for a verify
    /// terminal; `None` for a write-phase terminal and for `verified`).
    pub verify_verdict: Option<VerifyVerdict>,
    /// Canonical FR message — rendered verbatim by the panel. Non-empty.
    pub message: String,
    /// Canonical FR next gesture — rendered verbatim by the panel. Non-empty.
    pub user_action: String,
    /// The `verified` confirmation lines (`Some` iff `terminal_kind == Verified`).
    pub summary: Option<VerifiedSummary>,
}

impl PersistedTransferOutcome {
    /// Map a `verified` job terminal. The confirmation lines double as the
    /// message / next gesture so every stored row carries non-empty copy without
    /// inventing new wording.
    pub fn from_verified(summary: VerifiedSummary) -> Self {
        Self {
            terminal_kind: PersistedTerminalKind::Verified,
            cause: None,
            completeness: None,
            verify_verdict: None,
            message: summary.changed.clone(),
            user_action: summary.unchanged.clone(),
            summary: Some(summary),
        }
    }

    /// Map a WRITE-phase functional terminal (`TransferOutcome::Retryable`). The
    /// device completeness selects the kind: `Incomplete` → `transfert incomplet`,
    /// `Failed` → `échec récupérable`. Reuses `failure_copy` (the single FR source).
    pub fn from_write_terminal(
        cause: TransferFailureCause,
        completeness: TransferCompleteness,
    ) -> Self {
        let (message, user_action) = failure_copy(cause, completeness);
        let terminal_kind = match completeness {
            TransferCompleteness::Incomplete => PersistedTerminalKind::Incomplete,
            TransferCompleteness::Failed => PersistedTerminalKind::Retryable,
        };
        Self {
            terminal_kind,
            cause: Some(cause),
            completeness: Some(completeness),
            verify_verdict: None,
            message: message.to_string(),
            user_action: user_action.to_string(),
            summary: None,
        }
    }

    /// Map a `verify`-phase NON-success verdict (`TransferOutcome::Unverified`):
    /// `Partial` → `état partiel`, `Failed` → `échec récupérable` (the verify
    /// `failed` folds onto `retryable`, carrying `verify_verdict = "failed"`).
    /// `Verified` is NOT a verify failure — it is mapped by [`from_verified`] — so
    /// it yields `None` here. Reuses `VerifyVerdict::copy` (the single FR source).
    ///
    /// [`from_verified`]: PersistedTransferOutcome::from_verified
    pub fn from_verify_verdict(verdict: VerifyVerdict) -> Option<Self> {
        let (message, user_action) = verdict.copy()?;
        let terminal_kind = match verdict {
            VerifyVerdict::Partial => PersistedTerminalKind::Partial,
            VerifyVerdict::Failed => PersistedTerminalKind::Retryable,
            // `copy()` already returned `None` for `Verified`; defensive.
            VerifyVerdict::Verified => return None,
        };
        Some(Self {
            terminal_kind,
            cause: None,
            completeness: None,
            verify_verdict: Some(verdict),
            message: message.to_string(),
            user_action: user_action.to_string(),
            summary: None,
        })
    }

    /// Whether the record satisfies the coherence invariant (mirrors F6). Used to
    /// reject a corrupt reconstruction from stored columns:
    /// - a `verify_verdict` is mutually exclusive with `cause` / `completeness`;
    /// - a `summary` is present iff the kind is `Verified`;
    /// - the kind matches the structured discriminants it carries;
    /// - `message` / `user_action` are non-empty.
    pub fn is_coherent(&self) -> bool {
        if self.message.is_empty() || self.user_action.is_empty() {
            return false;
        }
        let has_write = self.cause.is_some() || self.completeness.is_some();
        let has_verify = self.verify_verdict.is_some();
        if has_write && has_verify {
            return false; // F6: never both at once.
        }
        match self.terminal_kind {
            PersistedTerminalKind::Verified => self.summary.is_some() && !has_write && !has_verify,
            PersistedTerminalKind::Partial => {
                self.summary.is_none()
                    && self.verify_verdict == Some(VerifyVerdict::Partial)
                    && !has_write
            }
            PersistedTerminalKind::Incomplete => {
                self.summary.is_none()
                    && self.completeness == Some(TransferCompleteness::Incomplete)
                    && self.cause.is_some()
                    && !has_verify
            }
            PersistedTerminalKind::Retryable => {
                // Two honest sources converge on `retryable`: a write-phase Failed
                // (cause + completeness=Failed) OR a verify Failed (verify_verdict).
                if self.summary.is_some() {
                    return false;
                }
                let write_failed = self.completeness == Some(TransferCompleteness::Failed)
                    && self.cause.is_some()
                    && !has_verify;
                let verify_failed =
                    self.verify_verdict == Some(VerifyVerdict::Failed) && !has_write;
                write_failed || verify_failed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> VerifiedSummary {
        VerifiedSummary {
            changed: "« Mon histoire » est maintenant sur la Lunii.".into(),
            unchanged: "2 autres histoires de l'appareil restent inchangées.".into(),
        }
    }

    #[test]
    fn wire_tag_round_trips_for_every_kind() {
        for kind in [
            PersistedTerminalKind::Verified,
            PersistedTerminalKind::Partial,
            PersistedTerminalKind::Retryable,
            PersistedTerminalKind::Incomplete,
        ] {
            assert_eq!(
                PersistedTerminalKind::from_wire_tag(kind.wire_tag()),
                Some(kind)
            );
        }
        assert_eq!(PersistedTerminalKind::from_wire_tag("transferring"), None);
        assert_eq!(PersistedTerminalKind::from_wire_tag(""), None);
    }

    #[test]
    fn from_verified_carries_summary_lines_as_message_and_action() {
        let outcome = PersistedTransferOutcome::from_verified(summary());
        assert_eq!(outcome.terminal_kind, PersistedTerminalKind::Verified);
        assert!(outcome.cause.is_none());
        assert!(outcome.completeness.is_none());
        assert!(outcome.verify_verdict.is_none());
        let s = outcome.summary.as_ref().expect("summary present");
        assert_eq!(outcome.message, s.changed);
        assert_eq!(outcome.user_action, s.unchanged);
        assert!(!outcome.message.is_empty() && !outcome.user_action.is_empty());
        assert!(outcome.is_coherent());
    }

    #[test]
    fn from_write_terminal_failed_maps_to_retryable() {
        let outcome = PersistedTransferOutcome::from_write_terminal(
            TransferFailureCause::DeviceChanged,
            TransferCompleteness::Failed,
        );
        assert_eq!(outcome.terminal_kind, PersistedTerminalKind::Retryable);
        assert_eq!(outcome.cause, Some(TransferFailureCause::DeviceChanged));
        assert_eq!(outcome.completeness, Some(TransferCompleteness::Failed));
        assert!(outcome.verify_verdict.is_none());
        assert!(outcome.summary.is_none());
        // The copy is the cause's own copy (device intact).
        let (message, action) = failure_copy(
            TransferFailureCause::DeviceChanged,
            TransferCompleteness::Failed,
        );
        assert_eq!(outcome.message, message);
        assert_eq!(outcome.user_action, action);
        assert!(outcome.is_coherent());
    }

    #[test]
    fn from_write_terminal_incomplete_maps_to_incomplete() {
        let outcome = PersistedTransferOutcome::from_write_terminal(
            TransferFailureCause::WriteRejected,
            TransferCompleteness::Incomplete,
        );
        assert_eq!(outcome.terminal_kind, PersistedTerminalKind::Incomplete);
        assert_eq!(outcome.completeness, Some(TransferCompleteness::Incomplete));
        assert_eq!(outcome.cause, Some(TransferFailureCause::WriteRejected));
        assert!(outcome.verify_verdict.is_none());
        assert!(outcome.is_coherent());
    }

    #[test]
    fn from_verify_verdict_partial_carries_only_the_verdict() {
        let outcome = PersistedTransferOutcome::from_verify_verdict(VerifyVerdict::Partial)
            .expect("partial is a non-success");
        assert_eq!(outcome.terminal_kind, PersistedTerminalKind::Partial);
        assert_eq!(outcome.verify_verdict, Some(VerifyVerdict::Partial));
        assert!(
            outcome.cause.is_none() && outcome.completeness.is_none(),
            "F6: a verify terminal carries no write-phase cause/completeness"
        );
        assert!(outcome.summary.is_none());
        assert!(outcome.is_coherent());
    }

    #[test]
    fn from_verify_verdict_failed_folds_onto_retryable() {
        let outcome = PersistedTransferOutcome::from_verify_verdict(VerifyVerdict::Failed)
            .expect("failed is a non-success");
        assert_eq!(outcome.terminal_kind, PersistedTerminalKind::Retryable);
        assert_eq!(outcome.verify_verdict, Some(VerifyVerdict::Failed));
        assert!(outcome.cause.is_none() && outcome.completeness.is_none());
        assert!(outcome.is_coherent());
    }

    #[test]
    fn from_verify_verdict_verified_is_none() {
        // A `verified` verdict is not a verify FAILURE — it is mapped by
        // `from_verified`, so this entry point declines it.
        assert!(PersistedTransferOutcome::from_verify_verdict(VerifyVerdict::Verified).is_none());
    }

    #[test]
    fn is_coherent_rejects_cause_and_verify_verdict_simultaneously() {
        // F6 violation: a row carrying BOTH a write cause AND a verify verdict.
        let incoherent = PersistedTransferOutcome {
            terminal_kind: PersistedTerminalKind::Retryable,
            cause: Some(TransferFailureCause::WriteRejected),
            completeness: Some(TransferCompleteness::Failed),
            verify_verdict: Some(VerifyVerdict::Failed),
            message: "m".into(),
            user_action: "a".into(),
            summary: None,
        };
        assert!(!incoherent.is_coherent());
    }

    #[test]
    fn is_coherent_rejects_verified_without_summary_and_summary_on_non_verified() {
        let verified_without_summary = PersistedTransferOutcome {
            terminal_kind: PersistedTerminalKind::Verified,
            cause: None,
            completeness: None,
            verify_verdict: None,
            message: "m".into(),
            user_action: "a".into(),
            summary: None,
        };
        assert!(!verified_without_summary.is_coherent());

        let partial_with_summary = PersistedTransferOutcome {
            terminal_kind: PersistedTerminalKind::Partial,
            cause: None,
            completeness: None,
            verify_verdict: Some(VerifyVerdict::Partial),
            message: "m".into(),
            user_action: "a".into(),
            summary: Some(summary()),
        };
        assert!(!partial_with_summary.is_coherent());
    }

    #[test]
    fn is_coherent_rejects_empty_copy() {
        let empty_message = PersistedTransferOutcome {
            terminal_kind: PersistedTerminalKind::Retryable,
            cause: Some(TransferFailureCause::Interrupted),
            completeness: Some(TransferCompleteness::Failed),
            verify_verdict: None,
            message: String::new(),
            user_action: "a".into(),
            summary: None,
        };
        assert!(!empty_message.is_coherent());
    }

    #[test]
    fn every_forward_constructor_is_coherent() {
        assert!(PersistedTransferOutcome::from_verified(summary()).is_coherent());
        for cause in [
            TransferFailureCause::WriteNotAuthorized,
            TransferFailureCause::NotPrepared,
            TransferFailureCause::NotTransferable,
            TransferFailureCause::DeviceChanged,
            TransferFailureCause::WriteRejected,
            TransferFailureCause::Interrupted,
        ] {
            for completeness in [
                TransferCompleteness::Failed,
                TransferCompleteness::Incomplete,
            ] {
                assert!(
                    PersistedTransferOutcome::from_write_terminal(cause, completeness)
                        .is_coherent(),
                    "{cause:?}/{completeness:?} must be coherent"
                );
            }
        }
        for verdict in [VerifyVerdict::Partial, VerifyVerdict::Failed] {
            assert!(PersistedTransferOutcome::from_verify_verdict(verdict)
                .expect("non-success verdict")
                .is_coherent());
        }
    }
}
