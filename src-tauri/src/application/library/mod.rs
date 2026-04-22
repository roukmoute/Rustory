use std::collections::HashSet;

use tauri::AppHandle;

use crate::domain::shared::AppError;
use crate::infrastructure::filesystem::ensure_app_data_dir;
use crate::ipc::dto::LibraryOverviewDto;

/// Application service for the `library` flow.
///
/// Confirms the managed local storage is reachable and returns the current
/// overview. Any storage failure bubbles up as a normalized [`AppError`] for
/// the UI to render.
///
/// Duplicate story ids in the overview are refused as a structured error so
/// the UI never has to reconcile ambiguous `key={id}` collisions at runtime.
pub fn load_overview(app: &AppHandle) -> Result<LibraryOverviewDto, AppError> {
    ensure_app_data_dir(app)?;
    let overview = LibraryOverviewDto::empty();
    enforce_unique_ids(&overview)?;
    Ok(overview)
}

fn enforce_unique_ids(overview: &LibraryOverviewDto) -> Result<(), AppError> {
    let mut seen = HashSet::with_capacity(overview.stories.len());
    for story in &overview.stories {
        if !seen.insert(story.id.as_str()) {
            return Err(AppError::library_inconsistent(
                "La bibliothèque locale contient des histoires en double.",
                "Recharge Rustory pour reconstruire la vue cohérente.",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::dto::StoryCardDto;

    #[test]
    fn empty_overview_is_consistent() {
        let overview = LibraryOverviewDto::empty();
        assert!(enforce_unique_ids(&overview).is_ok());
    }

    #[test]
    fn unique_ids_pass() {
        let overview = LibraryOverviewDto {
            stories: vec![
                StoryCardDto {
                    id: "a".into(),
                    title: "A".into(),
                },
                StoryCardDto {
                    id: "b".into(),
                    title: "B".into(),
                },
            ],
        };
        assert!(enforce_unique_ids(&overview).is_ok());
    }

    #[test]
    fn duplicate_id_rejected() {
        let overview = LibraryOverviewDto {
            stories: vec![
                StoryCardDto {
                    id: "a".into(),
                    title: "A".into(),
                },
                StoryCardDto {
                    id: "a".into(),
                    title: "A bis".into(),
                },
            ],
        };
        let err = enforce_unique_ids(&overview).expect_err("should reject duplicate ids");
        // The discriminant is LIBRARY_INCONSISTENT; the UI switches on `code`
        // and shows the canonical "bibliothèque incohérente" banner.
        let serialized = serde_json::to_value(&err).expect("serialize");
        assert_eq!(serialized["code"], "LIBRARY_INCONSISTENT");
    }
}
