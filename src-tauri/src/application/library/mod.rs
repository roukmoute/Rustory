use std::collections::HashSet;

use tauri::AppHandle;

use crate::domain::shared::AppError;
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::ensure_app_data_dir;
use crate::ipc::dto::import_export::{
    folder_import_findings_from_summary, import_findings_from_summary, import_state_dto_from_tag,
    ImportStateDto,
};
use crate::ipc::dto::{LibraryOverviewDto, StoryCardDto};

/// The `story_local_imports.source_format` tag of a structured-folder
/// creation — selects the FOLDER per-pair copy for the durable card report.
const STRUCTURED_FOLDER_FORMAT: &str = "structured-folder";

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
    // imported one carries its durable state + summary. The device-pack
    // provenance (`story_imports`) is joined too, because the pack RULES: a
    // forged double-provenance row must not surface a local import state or
    // report on its card when the detail and the ACKs already neutralize it
    // (`titleOnly` scope, `importState: null` — same precedence as
    // `story_edit_scope`). The ordering is unchanged (both joins are
    // one-to-at-most-one on the PK).
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT s.id, s.title, li.import_state, li.findings_summary, \
                    li.source_format, pi.story_id IS NOT NULL \
             FROM stories s \
             LEFT JOIN story_local_imports li ON li.story_id = s.id \
             LEFT JOIN story_imports pi ON pi.story_id = s.id \
             ORDER BY s.created_at ASC, s.id ASC",
        )
        .map_err(map_select_error)?;
    let rows = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let import_state: Option<String> = row.get(2)?;
            let findings_summary: Option<String> = row.get(3)?;
            let source_format: Option<String> = row.get(4)?;
            let device_pack: bool = row.get(5)?;
            Ok(project_story_card(
                id,
                title,
                import_state,
                findings_summary,
                source_format,
                device_pack,
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
/// durable import state and — while the review is PENDING — the
/// reconstructed on-demand report findings. An unrecognized stored state
/// tag degrades to a native card rather than failing the whole overview
/// read (defense in depth; the CHECK constraint already bounds the set).
/// A device-pack row (`story_imports`) PRIMES over any forged local-import
/// provenance: its content is carried by the copied pack, so the card never
/// surfaces a local import state or report the rest of the app neutralizes.
fn project_story_card(
    id: String,
    title: String,
    import_state: Option<String>,
    findings_summary: Option<String>,
    source_format: Option<String>,
    device_pack: bool,
) -> StoryCardDto {
    if device_pack {
        return StoryCardDto::native(id, title);
    }
    let Some(state) = import_state.as_deref().and_then(import_state_dto_from_tag) else {
        return StoryCardDto::native(id, title);
    };
    // The FULL per-aspect report (recognized + attention) reconstructed from
    // the durable summary, so the on-demand report survives a restart with
    // its global outcome + recognized elements + points of attention (§5) —
    // projected ONLY while the review is PENDING. A `resolved` review keeps
    // its findings in base as the trace but never renders them again: its
    // card goes quiet, exactly like a recognized import. The per-pair copy
    // follows the provenance's format: a structured-folder story speaks of
    // its manifest, a `.rustory` one of its artifact.
    let review_pending = matches!(state, ImportStateDto::Partial | ImportStateDto::NeedsReview);
    let import_report = if review_pending {
        let render = if source_format.as_deref() == Some(STRUCTURED_FOLDER_FORMAT) {
            folder_import_findings_from_summary
        } else {
            import_findings_from_summary
        };
        findings_summary
            .as_deref()
            .map(render)
            .filter(|report| !report.is_empty())
    } else {
        None
    };
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
    fn read_stories_projects_a_resolved_review_without_its_report() {
        // A SETTLED review renders exactly like a recognized import: the
        // provenance survives (importState: "resolved") but the findings
        // trace stays in base — never on the wire, never on the card.
        let mut db = fresh_db();
        let resolved = create_story(
            &mut db,
            CreateStoryInput {
                title: "Résolue".into(),
            },
        )
        .expect("create");
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'h.rustory', ?2, 'resolved', ?3, '2026-07-06T00:00:00.000Z')",
                rusqlite::params![
                    resolved.id,
                    "a".repeat(64),
                    "[{\"aspect\":\"structure\",\"category\":\"ambiguous\"}]",
                ],
            )
            .expect("insert resolved provenance");

        let stories = read_stories(&db).expect("read");
        let card = stories
            .iter()
            .find(|s| s.id == resolved.id)
            .expect("resolved card");
        assert_eq!(
            card.import_state,
            Some(crate::ipc::dto::import_export::ImportStateDto::Resolved),
            "the provenance stays projected"
        );
        assert!(
            card.import_report.is_none(),
            "the findings trace is NEVER rendered for a settled review"
        );
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
            Some("rustory".into()),
            false,
        );
        assert!(card.import_state.is_none());
    }

    #[test]
    fn read_stories_projects_a_structured_folder_creation_with_the_folder_copy() {
        // The card projection covers the new format with NO hidden filter:
        // the durable state + report surface exactly like a `.rustory`
        // import, and the report's copy is the FOLDER one (manifest
        // wording), selected by the provenance's source_format.
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Depuis un dossier".into(),
            },
        )
        .expect("create");
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'structured-folder', 1, 'mon-dossier', ?2, 'partial', ?3, '2026-07-06T00:00:00.000Z')",
                rusqlite::params![
                    created.id,
                    "a".repeat(64),
                    "[{\"aspect\":\"envelope\",\"category\":\"recognized\"},{\"aspect\":\"media\",\"category\":\"missing\"}]",
                ],
            )
            .expect("insert folder provenance");

        let stories = read_stories(&db).expect("read");
        let card = stories
            .iter()
            .find(|s| s.id == created.id)
            .expect("folder card");
        assert_eq!(
            card.import_state,
            Some(crate::ipc::dto::import_export::ImportStateDto::Partial),
            "the partial marker projects for the new format"
        );
        let report = card.import_report.as_ref().expect("durable report");
        let envelope = report
            .iter()
            .find(|f| f.aspect == crate::ipc::dto::ImportAspectDto::Envelope)
            .expect("envelope finding");
        assert!(
            envelope.message.contains("manifest"),
            "the folder copy speaks of the manifest, not of an artifact: {}",
            envelope.message
        );
        assert!(report
            .iter()
            .any(|f| f.category == ImportCategoryDto::Missing));
    }

    #[test]
    fn a_double_provenance_row_renders_as_a_pack_card_never_a_local_import() {
        // Pack-prime rule, same precedence as `story_edit_scope`: a forged
        // row present in BOTH provenance tables is a device pack — its card
        // must not surface the local import state/report the detail and the
        // ACKs already neutralize (`titleOnly`, `importState: null`).
        let mut db = fresh_db();
        let forged = create_story(
            &mut db,
            CreateStoryInput {
                title: "Forgée".into(),
            },
        )
        .expect("create");
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'h.rustory', ?2, 'needs_review', ?3, '2026-07-06T00:00:00.000Z')",
                rusqlite::params![
                    forged.id,
                    "a".repeat(64),
                    "[{\"aspect\":\"structure\",\"category\":\"ambiguous\"}]",
                ],
            )
            .expect("insert local provenance");
        db.conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
                 VALUES (?1, '019739b2-0000-7000-8000-000000000000', '0123456789abcdef0123456789abcdef', '2026-07-06T00:00:00.000Z', 5, 18, ?2, 'lunii')",
                rusqlite::params![forged.id, "ab".repeat(32)],
            )
            .expect("insert pack provenance");

        let stories = read_stories(&db).expect("read");
        let card = stories
            .iter()
            .find(|s| s.id == forged.id)
            .expect("forged card");
        assert!(
            card.import_state.is_none(),
            "the pack primes: no local import state on the card"
        );
        assert!(
            card.import_report.is_none(),
            "the pack primes: no local import report on the card"
        );
    }
}
