use serde::Serialize;

/// Card projection of a single story displayed in the library collection.
///
/// The wire shape is defined upfront so frontend consumers can rely on a
/// stable contract before the projection is populated.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryCardDto {
    pub id: String,
    pub title: String,
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

    #[test]
    fn empty_overview_serializes_as_empty_stories_array() {
        let dto = LibraryOverviewDto::empty();
        let v = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(v, serde_json::json!({ "stories": [] }));
    }

    #[test]
    fn story_card_uses_camel_case_wire_format() {
        let card = StoryCardDto {
            id: "s1".into(),
            title: "Titre".into(),
        };
        let v = serde_json::to_value(&card).expect("serialize");
        assert_eq!(v, serde_json::json!({ "id": "s1", "title": "Titre" }));
    }
}
