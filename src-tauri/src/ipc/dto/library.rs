use serde::Serialize;

use crate::ipc::dto::import_export::{ImportFindingDto, ImportStateDto};

/// Card projection of a single story displayed in the library collection.
///
/// The wire shape is defined upfront so frontend consumers can rely on a
/// stable contract before the projection is populated.
///
/// A NATIVE story (created locally or copied from a device) serializes as
/// exactly `{ id, title }`. A FILE-IMPORTED story additionally carries
/// `importState` (its durable import provenance + issue state, driving the
/// `Importée` origin marker and the `Import Issue Marker`) and, when it has
/// points of attention, `importReport` (the FULL on-demand report content).
/// Both extra fields are skipped when absent so the native shape is unchanged.
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
}

impl StoryCardDto {
    /// A native story card — no import provenance. Used everywhere a
    /// locally-created or device-copied story is projected.
    pub fn native(id: String, title: String) -> Self {
        Self {
            id,
            title,
            import_state: None,
            import_report: None,
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
}
