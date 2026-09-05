use serde::Serialize;

use crate::ipc::dto::import_export::{ImportFindingDto, ImportStateDto};

/// Card projection of a single story displayed in the library collection.
///
/// The wire shape is defined upfront so frontend consumers can rely on a
/// stable contract before the projection is populated.
///
/// A locally-created NATIVE story serializes as exactly `{ id, title }`. A
/// device-copied story adds only `transferable: true` (it owns a writeback
/// pack). A FILE-IMPORTED story additionally carries `importState` (its
/// durable import provenance + issue state, driving the `Importée` origin
/// marker and the `Import Issue Marker`) and, when it has points of
/// attention, `importReport` (the FULL on-demand report content). Every
/// extra field is skipped when absent/false so the minimal shape is intact.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryCardDto {
    pub id: String,
    pub title: String,
    /// Present iff the story came from a local artifact import. Its value
    /// (`recognized` / `partial` / `needsReview`) drives the durable card
    /// marker; `blocked` / `resolved` are never persisted here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_state: Option<ImportStateDto>,
    /// The FULL per-aspect report (recognized elements + points of
    /// attention) backing the on-demand `Import Review Flow`. Present only
    /// for a `partial` / `needsReview` import (a clean import has none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_report: Option<Vec<ImportFindingDto>>,
    /// `true` iff the story owns a device-format pack (imported FROM a
    /// device) — the only stories MVP can write back to a Lunii. Drives the
    /// send gate's pre-click "native non transférable" block WITHOUT a
    /// preparation probe. Skipped on the wire when `false` so a native /
    /// file-imported card keeps its minimal shape.
    #[serde(skip_serializing_if = "is_not_transferable")]
    pub transferable: bool,
    /// `true` iff the story can be sent to a Lunii V3 via the single
    /// "Envoyer vers la Lunii" gesture: it either retains its ORIGINAL source
    /// `.zip` (a structured-archive import — transcode + re-cipher for the
    /// target) or its structure lays out as a sequential device pack (every
    /// episode has an audio, no choices — a web / RSS / editor story).
    /// Independent of `transferable` (the V1/V2 byte-copy round-trip).
    /// Skipped on the wire when `false` so a card without it keeps its
    /// minimal shape. The library overview projection is authoritative; a
    /// card returned by a creation flow may carry the conservative `false`
    /// (the overview re-read settles it).
    #[serde(skip_serializing_if = "is_false")]
    pub sendable: bool,
    /// WHY the story is not sendable, when it is not — drives the pre-click
    /// "Envoi indisponible: …" reason. Absent when `sendable` (or when the
    /// projection did not decide, e.g. a creation-flow card).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_blocker: Option<SendBlockerDto>,
    /// Asset id of the story's COVER image — the START node's image, when it
    /// has one. The frontend loads the actual pixels through the existing
    /// `read_node_media` command (a display PNG data URL); only the opaque
    /// asset id crosses here. Skipped on the wire when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_asset_id: Option<String>,
}

fn is_not_transferable(transferable: &bool) -> bool {
    !transferable
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl StoryCardDto {
    /// A native story card — locally created, or a file import with no
    /// device pack. NOT transferable to a device in MVP.
    pub fn native(id: String, title: String) -> Self {
        Self {
            id,
            title,
            import_state: None,
            import_report: None,
            transferable: false,
            sendable: false,
            send_blocker: None,
            cover_asset_id: None,
        }
    }

    /// A device-copied story card: it owns a device-format pack, so it is
    /// transferable back to a compatible device. Same bare `{ id, title }`
    /// user-facing shape as a native card, plus the `transferable` flag.
    pub fn device_pack(id: String, title: String) -> Self {
        Self {
            id,
            title,
            import_state: None,
            import_report: None,
            transferable: true,
            sendable: false,
            send_blocker: Some(SendBlockerDto::DevicePack),
            cover_asset_id: None,
        }
    }
}

/// Why a story cannot be sent to a Lunii V3 (see [`StoryCardDto::send_blocker`]).
/// Closed set, camelCase on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SendBlockerDto {
    /// A device-copied (V1/V2) pack: its content is the copied pack, which
    /// the V3 engine does not convert.
    DevicePack,
    /// The structure has no episode.
    Empty,
    /// The structure's start node is missing from its nodes.
    Malformed,
    /// The story offers choices — not laid out by the sequential synthesis.
    Branching,
    /// At least one episode has no audio.
    MissingAudio,
}

impl SendBlockerDto {
    pub const fn from_domain(blocker: crate::domain::device::StoryPackBlocker) -> Self {
        use crate::domain::device::StoryPackBlocker;
        match blocker {
            StoryPackBlocker::Empty => Self::Empty,
            StoryPackBlocker::Malformed => Self::Malformed,
            StoryPackBlocker::Branching => Self::Branching,
            StoryPackBlocker::MissingAudio => Self::MissingAudio,
        }
    }
}

/// Read-model returned by `get_library_overview`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryOverviewDto {
    pub stories: Vec<StoryCardDto>,
}

impl LibraryOverviewDto {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::dto::import_export::ImportAspectDto;

    #[test]
    fn empty_overview_serializes_as_empty_stories_array() {
        let dto = LibraryOverviewDto::empty();
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(v, serde_json::json!({ "stories": [] }));
    }

    #[test]
    fn native_story_card_uses_exact_camel_case_wire_shape() {
        // A native story serializes as EXACTLY `{ id, title }` — the import
        // fields are skipped when absent so the historical shape is intact.
        let card = StoryCardDto::native("s1".into(), "Titre".into());
        let v = serde_json::to_value(&card).expect("serialize");
        assert_eq!(v, serde_json::json!({ "id": "s1", "title": "Titre" }));
    }

    #[test]
    fn a_clean_imported_story_card_carries_only_the_import_state() {
        let card = StoryCardDto {
            id: "s2".into(),
            title: "Importée".into(),
            import_state: Some(ImportStateDto::Recognized),
            import_report: None,
            transferable: false,
            sendable: false,
            send_blocker: None,
            cover_asset_id: None,
        };
        let v = serde_json::to_value(&card).expect("serialize");
        assert_eq!(
            v,
            serde_json::json!({
                "id": "s2",
                "title": "Importée",
                "importState": "recognized",
            })
        );
        // No report key for a clean import.
        assert!(v.get("importReport").is_none());
    }

    #[test]
    fn a_needs_review_imported_story_card_carries_state_and_report() {
        let card = StoryCardDto {
            id: "s3".into(),
            title: "À revoir".into(),
            import_state: Some(ImportStateDto::NeedsReview),
            import_report: Some(vec![ImportFindingDto {
                aspect: ImportAspectDto::Title,
                category: crate::ipc::dto::import_export::ImportCategoryDto::Ambiguous,
                message: "Le titre a été normalisé à l'import (espaces ou caractères ajustés)."
                    .into(),
            }]),
            transferable: false,
            sendable: false,
            send_blocker: None,
            cover_asset_id: None,
        };
        let v = serde_json::to_value(&card).expect("serialize");
        assert_eq!(v["importState"], "needsReview");
        assert_eq!(v["importReport"][0]["aspect"], "title");
        assert_eq!(v["importReport"][0]["category"], "ambiguous");
        assert!(!v["importReport"][0]["message"]
            .as_str()
            .expect("message")
            .is_empty());
    }

    #[test]
    fn a_device_pack_card_says_why_it_cannot_be_sent_to_a_v3() {
        let card = StoryCardDto::device_pack("s4".into(), "Copiée".into());
        let v = serde_json::to_value(&card).expect("serialize");
        assert_eq!(
            v,
            serde_json::json!({
                "id": "s4",
                "title": "Copiée",
                "transferable": true,
                "sendBlocker": "devicePack",
            })
        );
    }

    #[test]
    fn a_sendable_card_carries_the_flag_and_no_blocker() {
        let mut card = StoryCardDto::native("s5".into(), "Web".into());
        card.sendable = true;
        let v = serde_json::to_value(&card).expect("serialize");
        assert_eq!(
            v,
            serde_json::json!({ "id": "s5", "title": "Web", "sendable": true })
        );
    }

    #[test]
    fn send_blockers_serialize_in_camel_case_from_their_domain_reasons() {
        use crate::domain::device::StoryPackBlocker;
        for (domain, wire) in [
            (StoryPackBlocker::Empty, "empty"),
            (StoryPackBlocker::Malformed, "malformed"),
            (StoryPackBlocker::Branching, "branching"),
            (StoryPackBlocker::MissingAudio, "missingAudio"),
        ] {
            let dto = SendBlockerDto::from_domain(domain);
            assert_eq!(serde_json::to_value(dto).expect("serialize"), wire);
        }
        assert_eq!(
            serde_json::to_value(SendBlockerDto::DevicePack).expect("serialize"),
            "devicePack"
        );
    }
}
