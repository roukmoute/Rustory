use std::collections::HashSet;

use tauri::AppHandle;

use crate::domain::shared::AppError;
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::ensure_app_data_dir;
use crate::ipc::dto::import_export::{import_findings_from_summary, import_state_dto_from_tag};
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
    // LEFT JOIN the optional file-import provenance: a native story has no
    // `story_local_imports` row (both projected columns are NULL), an
    // imported one carries its durable state + summary. The ordering is
    // unchanged (the join is one-to-at-most-one on the PK).
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT s.id, s.title, li.import_state, li.findings_summary \
             FROM stories s \
             LEFT JOIN story_local_imports li ON li.story_id = s.id \
             ORDER BY s.created_at ASC, s.id ASC",
        )
        .map_err(map_select_error)?;
    let rows = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let import_state: Option<String> = row.get(2)?;
            let findings_summary: Option<String> = row.get(3)?;
            Ok(project_story_card(
                id,
                title,
                import_state,
                findings_summary,
            ))
        })
        .map_err(map_select_error)?;
    let mut stories = Vec::new();
    for entry in rows {
        stories.push(entry.map_err(map_select_error)?);
    }
    Ok(stories)
}

/// Project a `stories` row joined with its optional `story_local_imports`
/// provenance into a card. A native story (no provenance row) yields the
/// bare `{ id, title }` card; an imported one additionally carries its
/// durable import state and — when it has points of attention — the
/// reconstructed on-demand report findings. An unrecognized stored state
/// tag degrades to a native card rather than failing the whole overview
/// read (defense in depth; the CHECK constraint already bounds the set).
fn project_story_card(
    id: String,
    title: String,
    import_state: Option<String>,
    findings_summary: Option<String>,
) -> StoryCardDto {
    let Some(state) = import_state.as_deref().and_then(import_state_dto_from_tag) else {
        return StoryCardDto::native(id, title);
    };
    // The FULL per-aspect report (recognized + attention) reconstructed from
    // the durable summary, so the on-demand report survives a restart with
    // its global outcome + recognized elements + points of attention (§5).
    let import_report = findings_summary
        .as_deref()
        .map(import_findings_from_summary)
        .filter(|report| !report.is_empty());
    StoryCardDto {
        id,
        title,
        import_state: Some(state),
        import_report,
    }
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
    use crate::ipc::dto::ImportCategoryDto;

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
                StoryCardDto::native("a".into(), "A".into()),
                StoryCardDto::native("b".into(), "B".into()),
            ],
        };
        assert!(enforce_unique_ids(&overview).is_ok());
    }

    #[test]
    fn duplicate_id_rejected() {
        let overview = LibraryOverviewDto {
            stories: vec![
                StoryCardDto::native("a".into(), "A".into()),
                StoryCardDto::native("a".into(), "A bis".into()),
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

    #[test]
    fn read_stories_projects_file_import_provenance() {
        let mut db = fresh_db();
        let native = create_story(
            &mut db,
            CreateStoryInput {
                title: "Native".into(),
            },
        )
        .expect("native");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let imported = create_story(
            &mut db,
            CreateStoryInput {
                title: "Importée".into(),
            },
        )
        .expect("imported");
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'h.rustory', ?2, 'needs_review', ?3, '2026-06-27T00:00:00.000Z')",
                rusqlite::params![
                    imported.id,
                    "a".repeat(64),
                    // A FULL durable report: a recognized aspect + the attention one.
                    "[{\"aspect\":\"envelope\",\"category\":\"recognized\"},{\"aspect\":\"title\",\"category\":\"ambiguous\"}]",
                ],
            )
            .expect("insert provenance");

        let stories = read_stories(&db).expect("read");
        assert_eq!(stories.len(), 2);
        let native_card = stories.iter().find(|s| s.id == native.id).expect("native");
        assert!(
            native_card.import_state.is_none(),
            "a native story carries no import provenance"
        );
        assert!(native_card.import_report.is_none());
        let imported_card = stories
            .iter()
            .find(|s| s.id == imported.id)
            .expect("imported");
        assert!(
            imported_card.import_state.is_some(),
            "an imported story carries its durable import state"
        );
        let report = imported_card
            .import_report
            .as_ref()
            .expect("full report reconstructed from the durable summary");
        // The durable report restores BOTH the recognized element AND the
        // point of attention (not just attention) after a restart (§5).
        assert!(report
            .iter()
            .any(|f| f.category == ImportCategoryDto::Recognized));
        assert!(report
            .iter()
            .any(|f| f.category == ImportCategoryDto::Ambiguous));
    }

    #[test]
    fn read_stories_degrades_a_corrupt_import_state_to_a_native_card() {
        // A stored state tag outside the known set (defense in depth — the
        // CHECK constraint already bounds it) must not fail the whole read;
        // the card degrades to native.
        let mut db = fresh_db();
        let story = create_story(
            &mut db,
            CreateStoryInput {
                title: "Douteuse".into(),
            },
        )
        .expect("create");
        // Bypass the CHECK with a raw write is impossible (constraint), so
        // assert the projection helper directly instead.
        let card = super::project_story_card(
            story.id.clone(),
            "Douteuse".into(),
            Some("not_a_known_state".into()),
            None,
        );
        assert!(card.import_state.is_none());
    }
}
