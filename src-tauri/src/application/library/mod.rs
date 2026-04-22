use std::collections::HashSet;

use tauri::AppHandle;

use crate::domain::shared::AppError;
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::ensure_app_data_dir;
use crate::ipc::dto::{LibraryOverviewDto, StoryCardDto};

/// Application service for the `library` flow.
///
/// Confirms the managed local storage is reachable, reads the persisted
/// story projection from SQLite, and returns a stable ordering suitable
/// for the UI. Any storage failure bubbles up as a normalized
/// [`AppError`] for the UI to render.
///
/// Duplicate story ids are refused as a structured error so the UI never
/// has to reconcile ambiguous `key={id}` collisions at runtime. This also
/// acts as a defense-in-depth check against an unexpected schema drift
/// even though the PRIMARY KEY already enforces uniqueness.
pub fn load_overview(app: &AppHandle, db: &DbHandle) -> Result<LibraryOverviewDto, AppError> {
    ensure_app_data_dir(app)?;
    let stories = read_stories(db)?;
    let overview = LibraryOverviewDto { stories };
    enforce_unique_ids(&overview)?;
    Ok(overview)
}

fn read_stories(db: &DbHandle) -> Result<Vec<StoryCardDto>, AppError> {
    let mut stmt = db
        .conn()
        .prepare("SELECT id, title FROM stories ORDER BY created_at ASC, id ASC")
        .map_err(map_select_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(StoryCardDto {
                id: row.get(0)?,
                title: row.get(1)?,
            })
        })
        .map_err(map_select_error)?;
    let mut stories = Vec::new();
    for entry in rows {
        stories.push(entry.map_err(map_select_error)?);
    }
    Ok(stories)
}

fn map_select_error(_err: rusqlite::Error) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu lire ta bibliothèque locale.",
        "Relance l'application ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_select",
        "table": "stories",
    }))
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
    use crate::application::story::{create_story, CreateStoryInput};
    use crate::infrastructure::db;

    fn fresh_db() -> DbHandle {
        let mut db = db::open_in_memory().expect("open");
        db::run_migrations(&mut db).expect("migrate");
        db
    }

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
        let serialized = serde_json::to_value(&err).expect("serialize");
        assert_eq!(serialized["code"], "LIBRARY_INCONSISTENT");
    }

    #[test]
    fn read_stories_returns_persisted_entries_in_creation_order() {
        let mut db = fresh_db();
        let first = create_story(
            &mut db,
            CreateStoryInput {
                title: "Histoire A".into(),
            },
        )
        .expect("create a");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = create_story(
            &mut db,
            CreateStoryInput {
                title: "Histoire B".into(),
            },
        )
        .expect("create b");

        let stories = read_stories(&db).expect("read");
        assert_eq!(stories.len(), 2);
        assert_eq!(stories[0].id, first.id);
        assert_eq!(stories[0].title, "Histoire A");
        assert_eq!(stories[1].id, second.id);
        assert_eq!(stories[1].title, "Histoire B");
    }

    #[test]
    fn read_stories_on_empty_db_returns_empty_vec() {
        let db = fresh_db();
        let stories = read_stories(&db).expect("read");
        assert!(stories.is_empty());
    }
}
