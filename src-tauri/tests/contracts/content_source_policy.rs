//! Wire contracts of the content-source activation policy (the
//! distribution governance): the frozen FR copies (byte-for-byte), the
//! policy DTO shape, the policy-refusal error — and the EXACT
//! serialization of the CURRENT official distribution (one assertion per
//! matrix line, like the device support matrix).

use rustory_lib::domain::import::{
    official_content_sources, ContentSourceActivation, ContentSourceKind, ContentSourceLine,
    ALL_CONTENT_SOURCE_ACTIVATIONS, ALL_CONTENT_SOURCE_KINDS,
};
use rustory_lib::domain::shared::AppError;
use rustory_lib::ipc::dto::import_export::{
    content_source_activation_marker, content_source_label, content_source_reason,
    ContentSourcePolicyDto,
};

// ===== Frozen copies (product-language.md — byte-for-byte) =====

#[test]
fn kind_labels_are_frozen() {
    assert_eq!(content_source_label(ContentSourceKind::Rss), "Flux RSS");
    assert_eq!(content_source_label(ContentSourceKind::Atom), "Flux Atom");
    assert_eq!(
        content_source_label(ContentSourceKind::JsonFeed),
        "Flux JSON Feed"
    );
}

#[test]
fn disabled_entry_reasons_are_frozen() {
    assert_eq!(
        content_source_reason(ContentSourceActivation::Enabled),
        None
    );
    assert_eq!(
        content_source_reason(ContentSourceActivation::NotActivated),
        Some("Source indisponible: non activée dans la distribution officielle")
    );
    assert_eq!(
        content_source_reason(ContentSourceActivation::BlockedByPolicy),
        Some("Source indisponible: bloquée par la politique de distribution")
    );
}

#[test]
fn entry_level_activation_marker_is_frozen_and_enabled_only() {
    // The marker is Rust-owned so BOTH rendering surfaces (creation
    // dialog, support-profile screen) render the same copy verbatim —
    // never a re-typed frontend literal.
    assert_eq!(
        content_source_activation_marker(ContentSourceActivation::Enabled),
        Some("Activée par la distribution officielle")
    );
    assert_eq!(
        content_source_activation_marker(ContentSourceActivation::NotActivated),
        None
    );
    assert_eq!(
        content_source_activation_marker(ContentSourceActivation::BlockedByPolicy),
        None
    );
}

#[test]
fn policy_refusal_copy_is_frozen() {
    let err = AppError::content_source_unavailable(ContentSourceKind::Rss);
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "CONTENT_SOURCE_UNAVAILABLE");
    assert_eq!(
        v["message"],
        "Cette source de contenu n'est pas activée dans la distribution officielle."
    );
    assert_eq!(
        v["userAction"],
        "Utilise une source activée ou consulte le profil de support de ta version."
    );
    assert_eq!(v["details"]["source"], "content_source_policy");
    assert_eq!(v["details"]["kind"], "rss");
    // PII discipline: the refusal names the KIND family only.
    let raw = serde_json::to_string(&err).expect("ser");
    assert!(!raw.contains("http"), "never a URL fragment");
}

// ===== The CURRENT official policy, serialized EXACTLY =====

#[test]
fn the_official_policy_serializes_exactly() {
    let dto = ContentSourcePolicyDto::from_lines(official_content_sources());
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "sources": [
                {
                    "kind": "rss",
                    "label": "Flux RSS",
                    "activation": "enabled",
                    "activationMarker": "Activée par la distribution officielle",
                },
                {
                    "kind": "atom",
                    "label": "Flux Atom",
                    "activation": "notActivated",
                    "reason": "Source indisponible: non activée dans la distribution officielle",
                },
                {
                    "kind": "jsonFeed",
                    "label": "Flux JSON Feed",
                    "activation": "notActivated",
                    "reason": "Source indisponible: non activée dans la distribution officielle",
                },
            ]
        })
    );
}

#[test]
fn an_enabled_line_omits_the_reason_key_and_carries_the_marker() {
    let dto = ContentSourcePolicyDto::from_lines(&[ContentSourceLine {
        kind: ContentSourceKind::Rss,
        activation: ContentSourceActivation::Enabled,
    }]);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["sources"][0]["activation"], "enabled");
    assert!(
        v["sources"][0].get("reason").is_none(),
        "an enabled line carries NO reason key (the marker replaces it)"
    );
    assert_eq!(
        v["sources"][0]["activationMarker"],
        "Activée par la distribution officielle"
    );
}

#[test]
fn a_blocked_line_serializes_its_own_frozen_reason_and_no_marker() {
    // No official line is blocked today — the wire shape is proven on a
    // custom distribution so the copy stays frozen ahead of need.
    let dto = ContentSourcePolicyDto::from_lines(&[ContentSourceLine {
        kind: ContentSourceKind::Atom,
        activation: ContentSourceActivation::BlockedByPolicy,
    }]);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["sources"][0]["kind"], "atom");
    assert_eq!(v["sources"][0]["activation"], "blockedByPolicy");
    assert_eq!(
        v["sources"][0]["reason"],
        "Source indisponible: bloquée par la politique de distribution"
    );
    assert!(
        v["sources"][0].get("activationMarker").is_none(),
        "a non-enabled line carries NO marker key (the reason replaces it)"
    );
}

// ===== Exhaustiveness (tripwire round-trip: every domain value has its
// wire face, and the reason is coherent with the activation) =====

#[test]
fn every_known_kind_serializes_a_tag_and_a_label() {
    for kind in ALL_CONTENT_SOURCE_KINDS {
        assert!(!kind.wire_tag().is_empty());
        assert!(!content_source_label(kind).is_empty());
    }
}

#[test]
fn every_activation_serializes_a_tag_and_a_coherent_reason() {
    for activation in ALL_CONTENT_SOURCE_ACTIVATIONS {
        assert!(!activation.wire_tag().is_empty());
        let reason = content_source_reason(activation);
        assert_eq!(
            reason.is_none(),
            activation == ContentSourceActivation::Enabled,
            "reason present IFF the line is not enabled"
        );
        // The marker is the exact complement of the reason: present
        // IFF the line is enabled — a line always carries exactly one
        // of the two copies.
        assert_eq!(
            content_source_activation_marker(activation).is_some(),
            activation == ContentSourceActivation::Enabled,
            "marker present IFF the line is enabled"
        );
    }
}
