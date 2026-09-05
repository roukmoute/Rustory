use std::collections::HashSet;

use tauri::AppHandle;

use crate::domain::device::{linear_episodes, menu_blocker, StoryLayout, StoryPackBlocker};
use crate::domain::shared::AppError;
use crate::domain::story::CanonicalStructure;
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::ensure_app_data_dir;
use crate::ipc::dto::import_export::{
    folder_import_findings_from_summary, import_findings_from_summary, import_state_dto_from_tag,
    rss_import_findings_from_summary, ImportStateDto,
};
use crate::ipc::dto::{LibraryOverviewDto, SendBlockerDto, StoryCardDto};

/// The `story_local_imports.source_format` tag of a structured-folder
/// creation — selects the FOLDER per-pair copy for the durable card report.
const STRUCTURED_FOLDER_FORMAT: &str = "structured-folder";

/// The `story_local_imports.source_format` tag of an RSS ingestion —
/// selects the RSS per-pair copy (feed / episode wording).
const RSS_FORMAT: &str = "rss";

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
                    li.source_format, pi.story_id IS NOT NULL, \
                    COALESCE(li.source_archive_retained, 0), s.structure_json, \
                    sl.layout, sl.question_audio_asset_id IS NOT NULL \
             FROM stories s \
             LEFT JOIN story_local_imports li ON li.story_id = s.id \
             LEFT JOIN story_imports pi ON pi.story_id = s.id \
             LEFT JOIN story_layouts sl ON sl.story_id = s.id \
             ORDER BY s.created_at ASC, s.id ASC",
        )
        .map_err(map_select_error)?;
    // Raw columns first (the statement borrows the connection), projection
    // after — a menu story needs one more short read for its announcements.
    struct Row {
        id: String,
        title: String,
        import_state: Option<String>,
        findings_summary: Option<String>,
        source_format: Option<String>,
        device_pack: bool,
        sendable_archive: bool,
        structure_json: String,
        layout: StoryLayout,
        has_question: bool,
    }
    let rows = stmt
        .query_map([], |row| {
            let layout_tag: Option<String> = row.get(8)?;
            Ok(Row {
                id: row.get(0)?,
                title: row.get(1)?,
                import_state: row.get(2)?,
                findings_summary: row.get(3)?,
                source_format: row.get(4)?,
                device_pack: row.get(5)?,
                sendable_archive: row.get(6)?,
                structure_json: row.get(7)?,
                layout: layout_tag
                    .as_deref()
                    .and_then(StoryLayout::parse)
                    .unwrap_or_default(),
                has_question: row.get::<_, Option<bool>>(9)?.unwrap_or(false),
            })
        })
        .map_err(map_select_error)?
        .collect::<Result<Vec<Row>, _>>()
        .map_err(map_select_error)?;
    drop(stmt);

    let mut stories = Vec::with_capacity(rows.len());
    for row in rows {
        // Parsed ONCE for the cover and the send readiness. Defensive: a
        // malformed structure yields no cover and a blocked send, never a
        // failed overview.
        let structure: Option<CanonicalStructure> = serde_json::from_str(&row.structure_json).ok();
        // The menu layout is judged on its spoken announcements: one more
        // (short) read per menu story, never for the default layout.
        let prompt_node_ids = if row.layout == StoryLayout::Menu {
            read_prompt_node_ids(db, &row.id).map_err(map_select_error)?
        } else {
            Vec::new()
        };
        let presentation = PresentationFacts {
            layout: row.layout,
            has_question: row.has_question,
            prompt_node_ids: &prompt_node_ids,
        };
        let mut card = project_story_card(
            row.id,
            row.title,
            row.import_state,
            row.findings_summary,
            row.source_format,
            row.device_pack,
            row.sendable_archive,
            structure.as_ref(),
            &presentation,
        );
        // Cover = the START node's image, for every card shape alike (native
        // stories get one from the editor, imported ones from their pack).
        card.cover_asset_id = structure.as_ref().and_then(cover_asset_id_of);
        stories.push(card);
    }
    Ok(stories)
}

/// The node ids that carry a spoken title (menu announcement).
fn read_prompt_node_ids(db: &DbHandle, story_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = db
        .conn()
        .prepare("SELECT node_id FROM story_node_prompts WHERE story_id = ?1")?;
    let ids = stmt
        .query_map(rusqlite::params![story_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
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
#[allow(clippy::too_many_arguments)]
fn project_story_card(
    id: String,
    title: String,
    import_state: Option<String>,
    findings_summary: Option<String>,
    source_format: Option<String>,
    device_pack: bool,
    sendable_archive: bool,
    structure: Option<&CanonicalStructure>,
    presentation: &PresentationFacts<'_>,
) -> StoryCardDto {
    if device_pack {
        // A device-pack story owns its writeback artifacts — transferable
        // (and says so as its V3 send blocker).
        return StoryCardDto::device_pack(id, title);
    }
    let (sendable, send_blocker) = send_readiness(structure, sendable_archive, presentation);
    let Some(state) = import_state.as_deref().and_then(import_state_dto_from_tag) else {
        let mut card = StoryCardDto::native(id, title);
        card.sendable = sendable;
        card.send_blocker = send_blocker;
        return card;
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
        let render = match source_format.as_deref() {
            Some(STRUCTURED_FOLDER_FORMAT) => folder_import_findings_from_summary,
            Some(RSS_FORMAT) => rss_import_findings_from_summary,
            _ => import_findings_from_summary,
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
        // A file import (`.rustory` / folder / archive / rss / web) owns no
        // device-format pack — not transferable via the V1/V2 byte-copy
        // round-trip.
        transferable: false,
        // …but it CAN be sent to a Lunii V3 when it retained its source
        // `.zip` or when its structure lays out as a device pack.
        sendable,
        send_blocker,
        // Filled by the caller from the story's canonical structure.
        cover_asset_id: None,
    }
}

/// How the story is presented on a device, as the send readiness needs it:
/// its layout and, for the menu layout, which spoken announcements exist.
/// The default is the sequential layout (nothing to announce).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PresentationFacts<'a> {
    pub(crate) layout: StoryLayout,
    pub(crate) has_question: bool,
    pub(crate) prompt_node_ids: &'a [String],
}

/// Whether a (non device-pack) story can be sent to a Lunii V3, and why not
/// otherwise: a retained source archive always can (the archive engine); a
/// structure that lays out as a device pack can — every episode has an
/// audio, no choices (`domain::device::story_pack`) and, for the menu
/// layout, every spoken announcement exists; anything else is blocked for
/// the domain's reason (an unparsable structure counts as malformed).
pub(crate) fn send_readiness(
    structure: Option<&CanonicalStructure>,
    retained_archive: bool,
    presentation: &PresentationFacts<'_>,
) -> (bool, Option<SendBlockerDto>) {
    if retained_archive {
        return (true, None);
    }
    let by_structure = match structure {
        Some(structure) => {
            linear_episodes(structure).and_then(|episodes| match presentation.layout {
                StoryLayout::Sequential => Ok(()),
                StoryLayout::Menu => {
                    let has_prompt =
                        |node_id: &str| presentation.prompt_node_ids.iter().any(|n| n == node_id);
                    match menu_blocker(&episodes, presentation.has_question, has_prompt) {
                        Some(blocker) => Err(blocker),
                        None => Ok(()),
                    }
                }
            })
        }
        None => Err(StoryPackBlocker::Malformed),
    };
    match by_structure {
        Ok(()) => (true, None),
        Err(blocker) => (false, Some(SendBlockerDto::from_domain(blocker))),
    }
}

/// [`send_readiness`] from the persisted canonical JSON, for a story in the
/// default (sequential) presentation — a creation-flow card.
pub(crate) fn send_readiness_of_json(
    structure_json: &str,
    retained_archive: bool,
) -> (bool, Option<SendBlockerDto>) {
    let structure: Option<CanonicalStructure> = serde_json::from_str(structure_json).ok();
    send_readiness(
        structure.as_ref(),
        retained_archive,
        &PresentationFacts::default(),
    )
}

/// The story's cover = its START node's image asset id, when that node
/// carries one. PURE + DEFENSIVE: a legacy structure without one yields
/// `None` — a cover is decoration, it must never fail (or slow) the overview
/// read.
fn cover_asset_id_of(structure: &CanonicalStructure) -> Option<String> {
    structure
        .nodes
        .iter()
        .find(|n| n.id == structure.start_node_id)
        .and_then(|n| n.image_asset_id.clone())
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
    fn cover_asset_id_comes_from_the_start_node_image_and_degrades_to_none() {
        // The start node's image is the cover; other nodes' images never are.
        let structure = r#"{
            "schemaVersion": 3,
            "startNodeId": "n2",
            "nodes": [
                {"id":"n1","text":"","label":"","imageAssetId":"asset-OTHER","audioAssetId":null,"options":[]},
                {"id":"n2","text":"","label":"","imageAssetId":"asset-COVER","audioAssetId":null,"options":[]}
            ]
        }"#;
        assert_eq!(
            cover_asset_id_from_structure(structure).as_deref(),
            Some("asset-COVER")
        );
        // The test keeps the JSON entry point: parse then project, exactly
        // like `read_stories`.
        fn cover_asset_id_from_structure(json: &str) -> Option<String> {
            let structure: Option<CanonicalStructure> = serde_json::from_str(json).ok();
            structure.as_ref().and_then(super::cover_asset_id_of)
        }
        // A start node WITHOUT an image → no cover.
        let no_image = r#"{
            "schemaVersion": 3,
            "startNodeId": "n1",
            "nodes": [
                {"id":"n1","text":"","label":"","imageAssetId":null,"audioAssetId":null,"options":[]}
            ]
        }"#;
        assert_eq!(cover_asset_id_from_structure(no_image), None);
        // Malformed / legacy structures degrade to None, never an error.
        assert_eq!(cover_asset_id_from_structure("not json"), None);
        assert_eq!(cover_asset_id_from_structure("{}"), None);
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
            false,
            None,
            &PresentationFacts::default(),
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
        assert!(
            card.transferable,
            "a device-pack story is transferable back to a device"
        );
    }

    #[test]
    fn transferability_follows_the_device_pack_only() {
        // A device-pack story is transferable; a native one and a
        // file-import one are not — the send gate's pre-click block reads
        // exactly this flag, no preparation probe needed.
        let device = super::project_story_card(
            "d".into(),
            "Pack".into(),
            None,
            None,
            None,
            true,
            false,
            None,
            &PresentationFacts::default(),
        );
        assert!(device.transferable);
        assert!(!device.sendable);
        assert_eq!(device.send_blocker, Some(SendBlockerDto::DevicePack));

        let native = super::project_story_card(
            "n".into(),
            "Native".into(),
            None,
            None,
            None,
            false,
            false,
            None,
            &PresentationFacts::default(),
        );
        assert!(!native.transferable);

        let file_import = super::project_story_card(
            "f".into(),
            "Importée".into(),
            Some("recognized".into()),
            None,
            Some("rustory".into()),
            false,
            false,
            None,
            &PresentationFacts::default(),
        );
        assert!(!file_import.transferable);
        assert!(!file_import.sendable);

        // A structured-archive import that retained its source `.zip` is
        // V3-sendable (but still not `transferable` via the byte-copy path),
        // whatever its structure says.
        let archive_sendable = super::project_story_card(
            "a".into(),
            "Archive".into(),
            Some("recognized".into()),
            None,
            Some("structured-archive".into()),
            false,
            true,
            None,
            &PresentationFacts::default(),
        );
        assert!(!archive_sendable.transferable);
        assert!(archive_sendable.sendable);
        assert_eq!(archive_sendable.send_blocker, None);
    }

    fn structure_json(nodes: &[(&str, Option<&str>, bool)]) -> String {
        let nodes: Vec<serde_json::Value> = nodes
            .iter()
            .map(|(id, audio, branching)| {
                serde_json::json!({
                    "id": id, "text": "", "label": id,
                    "imageAssetId": null, "audioAssetId": audio,
                    "options": if *branching { serde_json::json!([{"label": "suite", "target": null}]) } else { serde_json::json!([]) },
                })
            })
            .collect();
        serde_json::json!({ "schemaVersion": 3, "startNodeId": "n1", "nodes": nodes }).to_string()
    }

    #[test]
    fn send_readiness_follows_the_structure_for_stories_without_an_archive() {
        // Every episode with an audio, no choices → sendable (a web / RSS
        // creation, or an editor story with narration).
        let linear = structure_json(&[("n1", Some("a1"), false), ("n2", Some("a2"), false)]);
        assert_eq!(super::send_readiness_of_json(&linear, false), (true, None));

        // An episode without audio → blocked, and the card says so.
        let mute = structure_json(&[("n1", Some("a1"), false), ("n2", None, false)]);
        assert_eq!(
            super::send_readiness_of_json(&mute, false),
            (false, Some(SendBlockerDto::MissingAudio))
        );
        // Choices → blocked as branching.
        let branching = structure_json(&[("n1", Some("a1"), true)]);
        assert_eq!(
            super::send_readiness_of_json(&branching, false),
            (false, Some(SendBlockerDto::Branching))
        );
        // The freshly created empty start node (no audio) → missing audio.
        let fresh = structure_json(&[("n1", None, false)]);
        assert_eq!(
            super::send_readiness_of_json(&fresh, false),
            (false, Some(SendBlockerDto::MissingAudio))
        );
        // Unparsable JSON → malformed; a retained archive still sends.
        assert_eq!(
            super::send_readiness_of_json("not json", false),
            (false, Some(SendBlockerDto::Malformed))
        );
        assert_eq!(
            super::send_readiness_of_json("not json", true),
            (true, None)
        );
    }

    #[test]
    fn the_menu_layout_is_sendable_only_with_its_announcements() {
        let structure: CanonicalStructure = serde_json::from_str(&structure_json(&[
            ("n1", Some("a1"), false),
            ("n2", Some("a2"), false),
        ]))
        .unwrap();
        let prompts = vec!["n1".to_string(), "n2".to_string()];
        let complete = PresentationFacts {
            layout: StoryLayout::Menu,
            has_question: true,
            prompt_node_ids: &prompts,
        };
        assert_eq!(
            super::send_readiness(Some(&structure), false, &complete),
            (true, None)
        );
        let one_short = vec!["n1".to_string()];
        let partial = PresentationFacts {
            layout: StoryLayout::Menu,
            has_question: true,
            prompt_node_ids: &one_short,
        };
        assert_eq!(
            super::send_readiness(Some(&structure), false, &partial),
            (false, Some(SendBlockerDto::MissingAnnouncements))
        );
        let no_question = PresentationFacts {
            layout: StoryLayout::Menu,
            has_question: false,
            prompt_node_ids: &prompts,
        };
        assert_eq!(
            super::send_readiness(Some(&structure), false, &no_question),
            (false, Some(SendBlockerDto::MissingAnnouncements))
        );
        // A retained archive is sent as such, whatever the layout says.
        assert_eq!(
            super::send_readiness(Some(&structure), true, &no_question),
            (true, None)
        );
        // The structure's own blockers come first.
        let mute: CanonicalStructure =
            serde_json::from_str(&structure_json(&[("n1", None, false)])).unwrap();
        assert_eq!(
            super::send_readiness(Some(&mute), false, &complete),
            (false, Some(SendBlockerDto::MissingAudio))
        );
    }

    #[test]
    fn read_stories_projects_a_menu_story_from_its_announcement_rows() {
        let mut db = fresh_db();
        let story = create_story(
            &mut db,
            CreateStoryInput {
                title: "Menu".into(),
            },
        )
        .expect("create");
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = ?1 WHERE id = ?2",
                rusqlite::params![
                    structure_json(&[("n1", Some("a1"), false), ("n2", Some("a2"), false)]),
                    &story.id
                ],
            )
            .expect("update");
        db.conn()
            .execute(
                "INSERT INTO story_layouts (story_id, layout, question_audio_asset_id, question_spoken_text, voice_id, updated_at) \
                 VALUES (?1, 'menu', 'q', 'Question ?', 'v', '2026-01-01T00:00:00Z')",
                rusqlite::params![&story.id],
            )
            .expect("layout");
        db.conn()
            .execute(
                "INSERT INTO story_node_prompts (story_id, node_id, audio_asset_id, spoken_text, voice_id, updated_at) \
                 VALUES (?1, 'n1', 'p1', 'Un.', 'v', '2026-01-01T00:00:00Z')",
                rusqlite::params![&story.id],
            )
            .expect("prompt");
        let cards = super::read_stories(&db).expect("read");
        let card = cards.iter().find(|c| c.id == story.id).expect("card");
        assert!(!card.sendable);
        assert_eq!(
            card.send_blocker,
            Some(SendBlockerDto::MissingAnnouncements)
        );
        db.conn()
            .execute(
                "INSERT INTO story_node_prompts (story_id, node_id, audio_asset_id, spoken_text, voice_id, updated_at) \
                 VALUES (?1, 'n2', 'p2', 'Deux.', 'v', '2026-01-01T00:00:00Z')",
                rusqlite::params![&story.id],
            )
            .expect("prompt");
        let cards = super::read_stories(&db).expect("read");
        let card = cards.iter().find(|c| c.id == story.id).expect("card");
        assert!(card.sendable);
    }

    #[test]
    fn read_stories_projects_send_readiness_from_the_persisted_structure() {
        let mut db = fresh_db();
        let story = create_story(
            &mut db,
            CreateStoryInput {
                title: "Narrée".into(),
            },
        )
        .expect("create");
        // A fresh story: its single start node has no audio → blocked.
        let cards = super::read_stories(&db).expect("read");
        let card = cards.iter().find(|c| c.id == story.id).expect("card");
        assert!(!card.sendable);
        assert_eq!(card.send_blocker, Some(SendBlockerDto::MissingAudio));

        // Give every node an audio: the overview now reports it sendable.
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = ?1 WHERE id = ?2",
                rusqlite::params![
                    structure_json(&[("n1", Some("a1"), false), ("n2", Some("a2"), false)]),
                    &story.id
                ],
            )
            .expect("update");
        let cards = super::read_stories(&db).expect("read");
        let card = cards.iter().find(|c| c.id == story.id).expect("card");
        assert!(card.sendable);
        assert_eq!(card.send_blocker, None);
    }
}
