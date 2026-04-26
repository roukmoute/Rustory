//! Recovery draft canonical model and equality classification.
//!
//! This module is intentionally free of `serde` / `tauri` / persistence
//! imports — it stays under the `domain` boundary rule (no infrastructure
//! or wire concerns leak into business invariants).
//!
//! The two types here express two distinct concepts:
//! - [`RecoveryDraft`] is the in-memory snapshot of a single row from the
//!   `story_drafts` table — a buffered keystroke value that survived the
//!   last app shutdown.
//! - [`RecoveryDraftDelta`] is the decision the UI must make when both a
//!   draft and the persisted title exist: surface a recovery banner, or
//!   silently drop the draft because it already matches what is in
//!   `stories.title`.

use crate::domain::story::validation::normalize_title;

/// In-memory snapshot of a `story_drafts` row.
///
/// Field semantics mirror the SQLite schema:
/// - `story_id`: foreign key to `stories.id`. Always non-empty by the
///   table's PRIMARY KEY constraint.
/// - `draft_title`: the raw value the user had typed, as captured by
///   `record_draft`. May be empty (the user erased everything before the
///   crash) and may contain characters that would fail `validate_title` —
///   re-validation happens at `apply_recovery` time, never at record time.
/// - `draft_at`: ISO-8601 UTC millisecond timestamp with the `Z` suffix,
///   produced by `now_iso_ms()` at record time. Stored as a string so the
///   serialization stays canonical across the `application` and `ipc`
///   layers without re-formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDraft {
    pub story_id: String,
    pub draft_title: String,
    pub draft_at: String,
}

/// Outcome of comparing a recoverable draft to the persisted story title.
///
/// `classify` is the single place where the equality rule lives. The rule
/// uses `normalize_title` (NFC + trim) on both sides because the user
/// might have typed a value that only differs from the persisted one by
/// surrounding whitespace or by Unicode normal form — neither warrants a
/// recovery banner, both must produce `AlreadyPersisted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryDraftDelta {
    /// The recovered draft matches the persisted title after normalization.
    /// The caller should silently delete the row: there is nothing to
    /// recover, and showing a banner would be cognitive noise.
    AlreadyPersisted,
    /// The recovered draft differs from the persisted title. The UI must
    /// surface a recovery banner with both values shown verbatim, so the
    /// user sees exactly what was typed and what is currently on disk.
    Recoverable {
        /// The byte-exact persisted title from `stories.title`. Never
        /// re-normalized for display: the user must see the actual value
        /// the app holds as truth.
        persisted_title: String,
        /// The byte-exact draft value from `story_drafts.draft_title`.
        /// Same rule: shown as typed, never re-normalized in flight.
        draft_title: String,
    },
}

impl RecoveryDraftDelta {
    /// Classify a draft against a persisted title using the canonical
    /// equality rule. Both sides are normalized through `normalize_title`
    /// before comparison; the original strings are preserved when the
    /// outcome is `Recoverable` so the UI can render them verbatim.
    pub fn classify(persisted_title: &str, draft_title: &str) -> Self {
        if normalize_title(persisted_title) == normalize_title(draft_title) {
            Self::AlreadyPersisted
        } else {
            Self::Recoverable {
                persisted_title: persisted_title.to_string(),
                draft_title: draft_title.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_returns_already_persisted_when_titles_match_byte_for_byte() {
        assert_eq!(
            RecoveryDraftDelta::classify("Histoire", "Histoire"),
            RecoveryDraftDelta::AlreadyPersisted,
        );
    }

    #[test]
    fn classify_returns_already_persisted_when_titles_match_after_nfc_trim() {
        // Persisted is already trimmed; draft has wrap-around whitespace
        // that disappears under `normalize_title`.
        assert_eq!(
            RecoveryDraftDelta::classify("Histoire", "  Histoire  "),
            RecoveryDraftDelta::AlreadyPersisted,
        );
    }

    #[test]
    fn classify_returns_already_persisted_when_titles_match_via_unicode_normalization() {
        // "é" precomposed (NFC) vs "e" + U+0301 combining acute (NFD).
        // Both normalize to the same NFC form.
        let nfc = "café";
        let nfd = "cafe\u{0301}";
        assert_ne!(
            nfc, nfd,
            "fixture sanity: byte sequences must differ before normalization"
        );
        assert_eq!(
            RecoveryDraftDelta::classify(nfc, nfd),
            RecoveryDraftDelta::AlreadyPersisted,
        );
    }

    #[test]
    fn classify_returns_recoverable_when_titles_differ_after_trim() {
        match RecoveryDraftDelta::classify("Le Petit Prince", "Le Petit Renard") {
            RecoveryDraftDelta::Recoverable {
                persisted_title,
                draft_title,
            } => {
                assert_eq!(persisted_title, "Le Petit Prince");
                assert_eq!(draft_title, "Le Petit Renard");
            }
            other => panic!("expected Recoverable, got {other:?}"),
        }
    }

    #[test]
    fn classify_returns_recoverable_when_draft_is_empty_string() {
        // The user may have erased everything before the crash. That
        // intent is recoverable — show the banner so they can choose to
        // discard or commit the empty value (which `apply_recovery` will
        // then refuse via `validate_title`, prompting Discard).
        match RecoveryDraftDelta::classify("Persisted", "") {
            RecoveryDraftDelta::Recoverable {
                persisted_title,
                draft_title,
            } => {
                assert_eq!(persisted_title, "Persisted");
                assert_eq!(draft_title, "");
            }
            other => panic!("expected Recoverable, got {other:?}"),
        }
    }

    #[test]
    fn classify_returns_recoverable_when_persisted_is_short_and_draft_is_longer() {
        match RecoveryDraftDelta::classify("Old", "Old but longer") {
            RecoveryDraftDelta::Recoverable {
                persisted_title,
                draft_title,
            } => {
                assert_eq!(persisted_title, "Old");
                assert_eq!(draft_title, "Old but longer");
            }
            other => panic!("expected Recoverable, got {other:?}"),
        }
    }

    #[test]
    fn classify_preserves_original_strings_in_recoverable_variant() {
        // Both inputs carry surrounding whitespace; the variant must
        // still expose them verbatim. The UI is responsible for
        // formatting the on-screen rendering — the domain only decides.
        match RecoveryDraftDelta::classify("  A  ", "  B  ") {
            RecoveryDraftDelta::Recoverable {
                persisted_title,
                draft_title,
            } => {
                assert_eq!(persisted_title, "  A  ");
                assert_eq!(draft_title, "  B  ");
            }
            other => panic!("expected Recoverable, got {other:?}"),
        }
    }

    #[test]
    fn recovery_draft_struct_round_trips_via_clone_and_eq() {
        let a = RecoveryDraft {
            story_id: "id-1".into(),
            draft_title: "T".into(),
            draft_at: "2026-04-25T12:00:00.000Z".into(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn recovery_draft_eq_is_byte_exact_on_all_three_fields() {
        let base = RecoveryDraft {
            story_id: "id-1".into(),
            draft_title: "T".into(),
            draft_at: "2026-04-25T12:00:00.000Z".into(),
        };
        // Each field individually disagreeing must break equality.
        assert_ne!(
            base,
            RecoveryDraft {
                story_id: "id-2".into(),
                ..base.clone()
            }
        );
        assert_ne!(
            base,
            RecoveryDraft {
                draft_title: "U".into(),
                ..base.clone()
            }
        );
        assert_ne!(
            base,
            RecoveryDraft {
                draft_at: "2026-04-25T12:00:00.001Z".into(),
                ..base.clone()
            }
        );
    }

    #[test]
    fn classify_treats_only_whitespace_drafts_as_persisted_when_persisted_is_empty() {
        // Edge case: both normalize to "" (empty). Persisted should never
        // actually be empty in production (CHECK constraint blocks it),
        // but the rule must still hold mathematically — the domain stays
        // pure of contextual assumptions.
        assert_eq!(
            RecoveryDraftDelta::classify("", "   "),
            RecoveryDraftDelta::AlreadyPersisted,
        );
    }
}
