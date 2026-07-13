//! Official content-source registry: WHICH additional content sources
//! (RSS, Atom, JSON Feed…) the official distribution activates for story
//! creation, decided line by line — the exact pattern of the device
//! support matrix (`domain::device::profile`). Activating a source is a
//! DISTRIBUTION decision, never a user setting: no table, no migration,
//! no settings surface, no persistence. An alternative distribution edits
//! THIS matrix; the "visible default configuration" required by the
//! distribution policy IS this code plus its frozen, tested copies
//! (`docs/architecture/ui-states.md#Content Source Activation Contract`).
//!
//! Pure domain: facts in, activation out, zero I/O.

/// Closed set of KNOWN content-source kinds. A kind lands here when the
/// product speaks about it (its label and activation state render in the
/// creation dialog) — NOT when its ingestion is implemented: Atom and
/// JSON Feed are known kinds whose ingestion is deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentSourceKind {
    Rss,
    Atom,
    JsonFeed,
}

/// Every known kind, in the stable rendering order of the creation
/// dialog. Tripwire: a new enum variant fails the exhaustive `match`
/// below, forcing an explicit matrix decision for it.
pub const ALL_CONTENT_SOURCE_KINDS: [ContentSourceKind; 3] = [
    ContentSourceKind::Rss,
    ContentSourceKind::Atom,
    ContentSourceKind::JsonFeed,
];

impl ContentSourceKind {
    /// Stable camelCase wire tag (policy DTO, diagnostics). Must stay
    /// byte-identical to the TS mirror's closed set.
    pub const fn wire_tag(self) -> &'static str {
        match self {
            Self::Rss => "rss",
            Self::Atom => "atom",
            Self::JsonFeed => "jsonFeed",
        }
    }
}

/// Closed set of activation states a distribution can assign to a source
/// kind. `BlockedByPolicy` exists because the distribution policy names
/// it (protected-content-oriented flows are NEVER activated by default):
/// no line of the CURRENT matrix carries it (see the documenting test),
/// but its copies and mappings are frozen so the day a blocked source
/// appears is a re-scope, never an invention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentSourceActivation {
    Enabled,
    NotActivated,
    BlockedByPolicy,
}

/// Every activation state. Same tripwire role as
/// [`ALL_CONTENT_SOURCE_KINDS`].
pub const ALL_CONTENT_SOURCE_ACTIVATIONS: [ContentSourceActivation; 3] = [
    ContentSourceActivation::Enabled,
    ContentSourceActivation::NotActivated,
    ContentSourceActivation::BlockedByPolicy,
];

impl ContentSourceActivation {
    /// Stable camelCase wire tag (policy DTO, diagnostics). Must stay
    /// byte-identical to the TS mirror's closed set.
    pub const fn wire_tag(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::NotActivated => "notActivated",
            Self::BlockedByPolicy => "blockedByPolicy",
        }
    }
}

/// One line of the official matrix: a known kind and the activation the
/// distribution assigns to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentSourceLine {
    pub kind: ContentSourceKind,
    pub activation: ContentSourceActivation,
}

/// THE official content-source matrix of this distribution — activated
/// line by line, never wholesale (the device support-matrix pattern:
/// every line carries its own justification).
const OFFICIAL_CONTENT_SOURCES: &[ContentSourceLine] = &[
    // RSS ✅ Enabled — its ingestion mechanism is shipped (bounded
    // explicit fetch, two-phase creation, re-proven accept) and the
    // support policy validates it as the enabled external source of the
    // current distribution.
    ContentSourceLine {
        kind: ContentSourceKind::Rss,
        activation: ContentSourceActivation::Enabled,
    },
    // Atom ❌ NotActivated — a KNOWN kind whose ingestion is not
    // implemented (an Atom feed pasted into the RSS surface keeps its
    // honest format verdict) and whose activation the support policy has
    // not validated; its dialog entry renders disabled with the frozen
    // reason.
    ContentSourceLine {
        kind: ContentSourceKind::Atom,
        activation: ContentSourceActivation::NotActivated,
    },
    // JSON Feed ❌ NotActivated — same honest state as Atom: known,
    // not implemented, not validated.
    ContentSourceLine {
        kind: ContentSourceKind::JsonFeed,
        activation: ContentSourceActivation::NotActivated,
    },
];

/// The official matrix, as a borrowed slice: callers hand it to the
/// activation gate (and tests inject custom distributions instead).
pub fn official_content_sources() -> &'static [ContentSourceLine] {
    OFFICIAL_CONTENT_SOURCES
}

/// The PURE activation gate: which activation does `kind` have in the
/// given matrix? A kind ABSENT from the matrix is fail-closed
/// [`ContentSourceActivation::NotActivated`] — never a panic, never
/// enabled-by-default. The matrix travels as a parameter so the policy
/// stays consulted in one place per flow and tests can prove the refusal
/// with custom distributions (the device capability-gate pattern).
pub fn content_source_activation(
    sources: &[ContentSourceLine],
    kind: ContentSourceKind,
) -> ContentSourceActivation {
    sources
        .iter()
        .find(|line| line.kind == kind)
        .map(|line| line.activation)
        .unwrap_or(ContentSourceActivation::NotActivated)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== The official matrix — one test per line, like the device
    // support matrix. =====

    #[test]
    fn official_matrix_enables_rss() {
        assert_eq!(
            content_source_activation(official_content_sources(), ContentSourceKind::Rss),
            ContentSourceActivation::Enabled
        );
    }

    #[test]
    fn official_matrix_does_not_activate_atom() {
        assert_eq!(
            content_source_activation(official_content_sources(), ContentSourceKind::Atom),
            ContentSourceActivation::NotActivated
        );
    }

    #[test]
    fn official_matrix_does_not_activate_json_feed() {
        assert_eq!(
            content_source_activation(official_content_sources(), ContentSourceKind::JsonFeed),
            ContentSourceActivation::NotActivated
        );
    }

    #[test]
    fn official_matrix_carries_every_known_kind_exactly_once() {
        for kind in ALL_CONTENT_SOURCE_KINDS {
            let lines = official_content_sources()
                .iter()
                .filter(|line| line.kind == kind)
                .count();
            assert_eq!(lines, 1, "kind {kind:?} must have exactly one line");
        }
        assert_eq!(
            official_content_sources().len(),
            ALL_CONTENT_SOURCE_KINDS.len(),
            "no line may carry an unknown kind"
        );
    }

    #[test]
    fn no_current_line_is_blocked_by_policy() {
        // Documents the CURRENT distribution state: the variant exists
        // (the distribution policy names it), its copies and mappings are
        // frozen and tested, but no source is blocked today. A blocked
        // line appearing one day is an announced re-scope of this test.
        assert!(official_content_sources()
            .iter()
            .all(|line| line.activation != ContentSourceActivation::BlockedByPolicy));
    }

    // ===== The gate =====

    #[test]
    fn gate_returns_enabled_for_an_enabled_kind() {
        let matrix = [ContentSourceLine {
            kind: ContentSourceKind::Rss,
            activation: ContentSourceActivation::Enabled,
        }];
        assert_eq!(
            content_source_activation(&matrix, ContentSourceKind::Rss),
            ContentSourceActivation::Enabled
        );
    }

    #[test]
    fn gate_returns_not_activated_for_a_not_activated_kind() {
        let matrix = [ContentSourceLine {
            kind: ContentSourceKind::Rss,
            activation: ContentSourceActivation::NotActivated,
        }];
        assert_eq!(
            content_source_activation(&matrix, ContentSourceKind::Rss),
            ContentSourceActivation::NotActivated
        );
    }

    #[test]
    fn gate_returns_blocked_for_a_blocked_kind_in_a_custom_matrix() {
        let matrix = [ContentSourceLine {
            kind: ContentSourceKind::Rss,
            activation: ContentSourceActivation::BlockedByPolicy,
        }];
        assert_eq!(
            content_source_activation(&matrix, ContentSourceKind::Rss),
            ContentSourceActivation::BlockedByPolicy
        );
    }

    #[test]
    fn gate_fails_closed_on_a_kind_absent_from_the_matrix() {
        // An empty matrix (or a partial custom one) never panics and
        // never enables by default.
        assert_eq!(
            content_source_activation(&[], ContentSourceKind::Rss),
            ContentSourceActivation::NotActivated
        );
        let atom_only = [ContentSourceLine {
            kind: ContentSourceKind::Atom,
            activation: ContentSourceActivation::Enabled,
        }];
        assert_eq!(
            content_source_activation(&atom_only, ContentSourceKind::JsonFeed),
            ContentSourceActivation::NotActivated
        );
    }

    // ===== Wire tags — stable, distinct, exhaustive =====

    #[test]
    fn kind_wire_tags_are_stable() {
        // Exhaustive by construction: iterating the ALL_ tripwire array.
        let tags: Vec<&str> = ALL_CONTENT_SOURCE_KINDS
            .iter()
            .map(|kind| kind.wire_tag())
            .collect();
        assert_eq!(tags, vec!["rss", "atom", "jsonFeed"]);
    }

    #[test]
    fn activation_wire_tags_are_stable() {
        let tags: Vec<&str> = ALL_CONTENT_SOURCE_ACTIVATIONS
            .iter()
            .map(|activation| activation.wire_tag())
            .collect();
        assert_eq!(tags, vec!["enabled", "notActivated", "blockedByPolicy"]);
    }

    #[test]
    fn wire_tags_are_pairwise_distinct() {
        for (i, a) in ALL_CONTENT_SOURCE_KINDS.iter().enumerate() {
            for b in &ALL_CONTENT_SOURCE_KINDS[i + 1..] {
                assert_ne!(a.wire_tag(), b.wire_tag());
            }
        }
        for (i, a) in ALL_CONTENT_SOURCE_ACTIVATIONS.iter().enumerate() {
            for b in &ALL_CONTENT_SOURCE_ACTIVATIONS[i + 1..] {
                assert_ne!(a.wire_tag(), b.wire_tag());
            }
        }
    }
}
