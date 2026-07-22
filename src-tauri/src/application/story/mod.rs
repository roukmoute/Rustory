pub mod node;
pub mod recovery;
pub mod review;
pub mod scope;
pub mod structure;

use rusqlite::OptionalExtension;
use time::format_description::well_known::iso8601::{
    Config as Iso8601Config, EncodedConfig, TimePrecision,
};
use time::format_description::well_known::Iso8601;
use time::OffsetDateTime;

use crate::domain::shared::AppError;
use crate::domain::story::{
    canonical_structure_json, content_checksum, map_error, normalize_title, validate_title,
    CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION,
};
use crate::infrastructure::db::DbHandle;
use crate::ipc::dto::{StoryCardDto, StoryDetailDto, UpdateStoryOutputDto};

/// Input accepted by the `create_story` application service. Kept separate
/// from the IPC DTO so the application layer never imports wire-format
/// concerns.
pub struct CreateStoryInput {
    pub title: String,
}

/// Input accepted by the `update_story` application service. Mirrors
/// `UpdateStoryInputDto` without the serde derives — the command layer is
/// the only place that converts between the two.
pub struct UpdateStoryInput {
    pub id: String,
    pub title: String,
}

/// ISO-8601 format config truncated to milliseconds. Nanosecond precision
/// is available on most platforms but would leak clock resolution into the
/// canonical timestamp, where it is not useful.
const ISO8601_MS_CONFIG: EncodedConfig = Iso8601Config::DEFAULT
    .set_time_precision(TimePrecision::Second {
        decimal_digits: core::num::NonZeroU8::new(3),
    })
    .encode();
const ISO8601_MS: Iso8601<ISO8601_MS_CONFIG> = Iso8601::<ISO8601_MS_CONFIG>;

pub(crate) fn now_iso_ms() -> Result<String, AppError> {
    OffsetDateTime::now_utc().format(&ISO8601_MS).map_err(|_| {
        // Formatting an ISO-8601 timestamp can only fail if the system
        // clock reports a value outside the type's representable range
        // (year > 9999, < 0, or similarly corrupted). The user-facing
        // action targets that specific cause — not the storage path,
        // which is fine in this error branch.
        AppError::local_storage_unavailable(
            "Rustory n'a pas pu lire l'horloge système.",
            "Vérifie la date et l'heure de ton ordinateur puis relance l'application.",
        )
        .with_details(serde_json::json!({ "source": "system_clock_invalid" }))
    })
}

/// Create a new story and persist it as a canonical, versioned draft.
///
/// Validation runs before any SQL statement so a rejected title never
/// produces a half-written row; the `INSERT` is intentionally single-row
/// and atomic by construction (no staged filesystem artifacts in this
/// baseline).
pub fn create_story(db: &mut DbHandle, input: CreateStoryInput) -> Result<StoryCardDto, AppError> {
    let normalized = normalize_title(&input.title);
    validate_title(&normalized).map_err(map_error)?;

    let id = uuid::Uuid::now_v7().to_string();
    let structure = CanonicalStructure::minimal();
    let structure_json = canonical_structure_json(&structure);
    let checksum = content_checksum(&structure_json);

    let now_iso = now_iso_ms()?;

    db.conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![
                &id,
                &normalized,
                CANONICAL_STORY_SCHEMA_VERSION,
                &structure_json,
                &checksum,
                &now_iso,
            ],
        )
        .map_err(map_insert_error)?;

    Ok(StoryCardDto::native(id, normalized))
}

/// Update a persisted story's title.
///
/// Re-normalizes and re-validates the supplied title authoritatively: a
/// frontend bug that would let an invalid title through must fail at the
/// domain, not at a SQL CHECK constraint. The UPDATE runs inside a
/// `BEGIN IMMEDIATE` transaction so a second writer racing this call is
/// serialized up-front instead of discovering a conflict mid-commit.
///
/// Intentionally does NOT touch `structure_json` or `content_checksum`:
/// the canonical body is invariant while only metadata changes. Any future
/// canonical extension (e.g. description, language tag) must live under
/// `structure_json` behind a `schema_version` bump, not be smuggled into
/// this path.
pub fn update_story(
    db: &mut DbHandle,
    input: UpdateStoryInput,
) -> Result<UpdateStoryOutputDto, AppError> {
    let normalized = normalize_title(&input.title);
    validate_title(&normalized).map_err(map_error)?;

    let now_iso = now_iso_ms()?;

    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| map_update_transport_error(&err, "begin_transaction", &input.id))?;

    let rows_affected = tx
        .execute(
            "UPDATE stories SET title = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![&normalized, &now_iso, &input.id],
        )
        .map_err(|err| map_update_transport_error(&err, "update", &input.id))?;

    if rows_affected == 0 {
        // An UPDATE that matched no row: the id is not in the table.
        // Roll back so the transaction does not linger in the WAL.
        // Best-effort: if the rollback itself fails, we keep the
        // `LIBRARY_INCONSISTENT` diagnosis because it is still the most
        // useful explanation for the caller (the row is missing) and
        // attach the rollback failure as a secondary detail.
        let rollback_err = tx.rollback().err();
        return Err(AppError::library_inconsistent(
            "Histoire introuvable, recharge la bibliothèque.",
            "Retourne à la bibliothèque et recharge la liste.",
        )
        .with_details(serde_json::json!({
            "source": "story_missing",
            "id": input.id,
            "rollback": rollback_err.as_ref().map(|e| sqlite_kind_label(e)),
        })));
    }

    // Defense in depth: `id` is the PRIMARY KEY so a well-formed schema
    // cannot make this UPDATE touch more than one row. If it ever does,
    // something is wrong with the migration state (duplicate PK, schema
    // rewrite). Fail the update explicitly and refuse to commit rather
    // than silently mutate multiple rows.
    if rows_affected > 1 {
        let _ = tx.rollback();
        return Err(AppError::library_inconsistent(
            "La bibliothèque locale est incohérente.",
            "Relance Rustory pour reconstruire la vue cohérente.",
        )
        .with_details(serde_json::json!({
            "source": "story_duplicate",
            "id": input.id,
            "rowsAffected": rows_affected,
        })));
    }

    // A successful autosave consumes any pending recovery draft for this
    // story: the canonical row now reflects the user's latest committed
    // intent, so the buffered keystroke value has nothing left to recover.
    // Running the DELETE in the same transaction keeps the invariant
    // atomic — a failed commit rolls back both the UPDATE and the DELETE,
    // preserving the draft so the next session can still propose it.
    tx.execute(
        "DELETE FROM story_drafts WHERE story_id = ?1",
        rusqlite::params![&input.id],
    )
    .map_err(|err| map_update_transport_error(&err, "delete_draft", &input.id))?;

    // A title write is a REAL write: it settles a pending import review when
    // the canonical story is fully sound (AC3) — early-out inside, so a
    // native autosave never pays a structure parse. The ACK carries the
    // state read POST-UPDATE in this same transaction.
    let import_state = review::settle_review_after_title_write(&tx, &input.id)?;

    tx.commit()
        .map_err(|err| map_update_transport_error(&err, "commit", &input.id))?;

    Ok(UpdateStoryOutputDto {
        id: input.id,
        title: normalized,
        updated_at: now_iso,
        import_state,
    })
}

/// Input of [`delete_stories`] — the selection the user confirmed, verbatim.
pub struct DeleteStoriesInput {
    pub ids: Vec<String>,
}

/// Delete stories from the local library, all-or-nothing.
///
/// One `BEGIN IMMEDIATE` transaction deletes every requested row; a single
/// missing id rolls the whole batch back — what the user confirmed is
/// exactly what happens, never a partial removal. The schema's
/// `ON DELETE CASCADE` chains remove the DB children (drafts, provenance,
/// transfer memory, asset rows) inside the same transaction.
///
/// Node-media files are content-addressed and SHARED across stories, so
/// they are reclaimed AFTER commit through the refcounted GC
/// ([`node::gc_unreferenced_media_file`]): a file survives as long as any
/// remaining `assets` row references its content. The per-story
/// `imports/<id>` directory is filesystem-only and is the CALLER's cleanup
/// (off the DB lock) — a leftover is inert and swept at next boot.
pub fn delete_stories(
    db: &mut DbHandle,
    input: DeleteStoriesInput,
    node_media_dir: &std::path::Path,
) -> Result<Vec<String>, AppError> {
    if input.ids.is_empty() {
        return Ok(Vec::new());
    }

    // Captured BEFORE the rows disappear: the (content_hash, file_name)
    // pairs whose files may become unreferenced once the cascade fires.
    let mut media_candidates: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();

    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| map_delete_transport_error(&err, "begin_transaction", "<batch>"))?;

    for id in &input.ids {
        {
            let mut stmt = tx
                .prepare("SELECT content_hash, file_name FROM assets WHERE story_id = ?1")
                .map_err(|err| map_delete_transport_error(&err, "select_assets", id))?;
            let rows = stmt
                .query_map(rusqlite::params![id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|err| map_delete_transport_error(&err, "select_assets", id))?;
            for row in rows {
                let pair =
                    row.map_err(|err| map_delete_transport_error(&err, "select_assets", id))?;
                media_candidates.insert(pair);
            }
        }

        let rows_affected = tx
            .execute("DELETE FROM stories WHERE id = ?1", rusqlite::params![id])
            .map_err(|err| map_delete_transport_error(&err, "delete", id))?;

        if rows_affected == 0 {
            // One missing row invalidates the whole confirmed batch: roll
            // back so no story disappears under a stale confirmation.
            let rollback_err = tx.rollback().err();
            return Err(AppError::library_inconsistent(
                "Histoire introuvable, recharge la bibliothèque.",
                "Retourne à la bibliothèque et recharge la liste.",
            )
            .with_details(serde_json::json!({
                "source": "story_missing",
                "id": id,
                "rollback": rollback_err.as_ref().map(|e| sqlite_kind_label(e)),
            })));
        }

        if rows_affected > 1 {
            // Defense in depth, mirroring `update_story`: `id` is the
            // PRIMARY KEY — more than one row means a broken schema state.
            let _ = tx.rollback();
            return Err(AppError::library_inconsistent(
                "La bibliothèque locale est incohérente.",
                "Relance Rustory pour reconstruire la vue cohérente.",
            )
            .with_details(serde_json::json!({
                "source": "story_duplicate",
                "id": id,
                "rowsAffected": rows_affected,
            })));
        }
    }

    tx.commit()
        .map_err(|err| map_delete_transport_error(&err, "commit", "<batch>"))?;

    // Post-commit, still under the command's DB lock: unlink every media
    // file whose content is no longer referenced by ANY remaining asset
    // row. Best-effort by design — a missed unlink is reclaimed by the
    // boot sweep (`node::sweep_orphan_node_media`), never a user error.
    for (content_hash, file_name) in media_candidates {
        node::gc_unreferenced_media_file(db, node_media_dir, Some((content_hash, file_name)));
    }

    Ok(input.ids)
}

fn map_delete_transport_error(
    err: &rusqlite::Error,
    stage: &'static str,
    story_id: &str,
) -> AppError {
    // Same PII discipline as `map_update_transport_error`: drop the raw
    // rusqlite message, keep a stable `source`/`stage` for support.
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu supprimer la sélection.",
        "Réessaie dans un instant ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_delete",
        "table": "stories",
        "stage": stage,
        "kind": sqlite_kind_label(err),
        "id": story_id,
    }))
}

/// Read a single story by id for the edit surface.
///
/// Returns `Ok(None)` when the row is missing — an informational case the
/// UI renders as "Histoire introuvable". A transport-level failure (SQLite
/// IO, schema mismatch, etc.) crosses the boundary as a normalized
/// [`AppError`] so the UI never has to distinguish "row absent" from
/// "storage broken" from shared plumbing.
///
/// `node_id` targets the SELECTED node's full content projection: `None` =
/// the start node; a stale id over a HEALTHY graph falls back gracefully to
/// the start node (the graph stays projected — never a blank editor over a
/// sound structure).
pub fn get_story_detail(
    db: &DbHandle,
    app_data_dir: &std::path::Path,
    story_id: &str,
    node_id: Option<&str>,
) -> Result<Option<StoryDetailDto>, AppError> {
    // Read the raw row first; the projections + editability flag need
    // follow-up reads (asset lookups, provenance) that cannot live inside the
    // single `query_row` closure.
    let row: Option<(String, String, u32, String, String, String, String)> = db
        .conn()
        .query_row(
            "SELECT id, title, schema_version, structure_json, content_checksum, created_at, updated_at \
             FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(map_detail_read_error)?;

    let Some((id, title, schema_version, structure_json, content_checksum, created_at, updated_at)) =
        row
    else {
        return Ok(None);
    };

    // The declared edit scope (FR21) + the durable import review state — the
    // ACK side shares the exact same derivations (`scope::story_edit_scope`,
    // `review::read_import_state`), so detail and acknowledgements can never
    // diverge, even on forged data. Both stay projected under a Blocking
    // degradation: they are story metadata, not canonical content. The
    // review read is RE-MAPPED here to a read-flavored error: this is a
    // story OPEN, so the copy must never speak about a modification that was
    // never attempted (the ACK paths keep the write copy — their write
    // transaction really is rolled back there).
    let edit_scope = scope::story_edit_scope(db.conn(), &id);
    let editable = edit_scope == scope::StoryEditScope::Full;
    let import_state = review::read_import_state(db.conn(), &id, edit_scope)
        .map_err(|_| map_review_read_error(&id))?;
    // Project the graph + the selected node FROM Rust. The gate is "no
    // BLOCKING blocker" — NOT "no blocker at all": a Fixable issue (a broken
    // option link, an invalid persisted title) MUST leave the structure and
    // node projected, because the user repairs it IN the editor; hiding the
    // graph would make the flagged spot unreachable. A Blocking issue
    // (unsupported schema, corrupt structure, checksum mismatch, duplicate
    // ids, invalid start) degrades BOTH to `None`, which the UI renders as
    // the named "Structure illisible" state. Never project (hence never
    // expose as editable) a structure we would not vouch for.
    let media_dir = crate::infrastructure::filesystem::resolve_node_media_dir(app_data_dir);
    let persisted_facts = crate::domain::story::CanonicalStoryFacts {
        title: title.clone(),
        schema_version,
        structure_json: structure_json.clone(),
        content_checksum: content_checksum.clone(),
    };
    let has_blocking = crate::domain::story::validate_canonical(&persisted_facts)
        .iter()
        .any(|b| b.severity == crate::domain::story::Severity::Blocking);
    let (structure_dto, node) = if has_blocking {
        (None, None)
    } else {
        match serde_json::from_str::<CanonicalStructure>(&structure_json) {
            Ok(parsed) if parsed.schema_version == CANONICAL_STORY_SCHEMA_VERSION => {
                let projected = structure::project_structure(&parsed);
                // The selected node: the requested id when it exists, else the
                // start node (guaranteed present on a non-blocking graph —
                // `StartNodeInvalid` is Blocking).
                let selected = node_id
                    .and_then(|want| parsed.nodes.iter().find(|n| n.id == want))
                    .or_else(|| parsed.nodes.iter().find(|n| n.id == parsed.start_node_id));
                let node_dto =
                    selected.map(|n| node::project_node_content(db.conn(), &media_dir, n));
                (Some(projected), node_dto)
            }
            _ => (None, None),
        }
    };

    Ok(Some(StoryDetailDto {
        id,
        title,
        schema_version,
        structure_json,
        content_checksum,
        created_at,
        updated_at,
        editable,
        edit_scope: edit_scope.wire_tag().to_string(),
        import_state: import_state.map(|s| s.wire_tag().to_string()),
        structure: structure_dto,
        node,
    }))
}

fn map_insert_error(err: rusqlite::Error) -> AppError {
    // Surface the minimum useful diagnostic without leaking the raw
    // rusqlite message, which can embed table names or filesystem info.
    let kind = match err {
        rusqlite::Error::SqliteFailure(ref code, _) => match code.code {
            rusqlite::ErrorCode::ConstraintViolation => "constraint_violation",
            _ => "other",
        },
        _ => "other",
    };
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu enregistrer ta nouvelle histoire.",
        "Relance l'application ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_insert",
        "table": "stories",
        "kind": kind,
    }))
}

pub(crate) fn sqlite_kind_label(err: &rusqlite::Error) -> &'static str {
    match err {
        rusqlite::Error::SqliteFailure(code, _) => match code.code {
            rusqlite::ErrorCode::ConstraintViolation => "constraint_violation",
            rusqlite::ErrorCode::DatabaseBusy => "busy",
            rusqlite::ErrorCode::DatabaseLocked => "locked",
            _ => "other",
        },
        _ => "other",
    }
}

fn map_update_transport_error(
    err: &rusqlite::Error,
    stage: &'static str,
    story_id: &str,
) -> AppError {
    // Same PII discipline as `map_insert_error`: drop the raw rusqlite
    // message, keep a stable `source`/`stage` for support.
    let kind = sqlite_kind_label(err);
    // A CHECK constraint violation on UPDATE means the candidate title
    // slipped past Rust-side validation and was still refused by the
    // schema (e.g. blank after `trim`). Surface it as INVALID_STORY_TITLE
    // so the UI maps it to the same canonical reason as `create_story`
    // — inconsistency between the two paths would let the frontend show
    // "Réessaie dans un instant" for a bug the user can actually fix by
    // changing the title.
    if kind == "constraint_violation" {
        return AppError::invalid_story_title(
            "Création impossible: titre contient des caractères non autorisés",
            "Supprime les sauts de ligne, tabulations et caractères invisibles.",
        )
        .with_details(serde_json::json!({
            "source": "sqlite_update",
            "table": "stories",
            "stage": stage,
            "kind": kind,
            "id": story_id,
        }));
    }
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu enregistrer ta modification.",
        "Réessaie dans un instant ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_update",
        "table": "stories",
        "stage": stage,
        "kind": kind,
        "id": story_id,
    }))
}

fn map_detail_read_error(_err: rusqlite::Error) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu relire ton brouillon local.",
        "Relance l'application ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_select",
        "table": "stories",
    }))
}

/// A transient failure while reading the import-review provenance during a
/// story OPEN (`get_story_detail`): read-flavored copy, never a message
/// about a write that was never attempted (guardrail: no label may be false
/// in one of its contexts). Same PII discipline as the other read mappers.
fn map_review_read_error(story_id: &str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu ouvrir cette histoire.",
        "Réessaie dans un instant ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_import_review",
        "stage": "detail_read",
        "id": story_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;
    use crate::infrastructure::db;

    fn fresh_db() -> DbHandle {
        let mut db = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut db).expect("migrate");
        db
    }

    fn insert_asset(db: &DbHandle, story_id: &str, content_hash: &str, file_name: &str) {
        db.conn()
            .execute(
                "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at)
                 VALUES (?1, ?2, ?3, 'image', 'png', 1, ?4, '2026-07-22T00:00:00.000Z')",
                rusqlite::params![
                    format!("asset-{story_id}-{file_name}"),
                    story_id,
                    content_hash,
                    file_name
                ],
            )
            .expect("insert asset row");
    }

    fn story_count(db: &DbHandle) -> i64 {
        db.conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |r| r.get(0))
            .expect("count stories")
    }

    #[test]
    fn delete_stories_removes_the_requested_rows_and_returns_them() {
        let mut db = fresh_db();
        let kept = create_story(
            &mut db,
            CreateStoryInput {
                title: "Gardée".into(),
            },
        )
        .expect("create kept");
        let doomed = create_story(
            &mut db,
            CreateStoryInput {
                title: "Condamnée".into(),
            },
        )
        .expect("create doomed");
        let media_dir = tempfile::tempdir().expect("tempdir");

        let deleted = delete_stories(
            &mut db,
            DeleteStoriesInput {
                ids: vec![doomed.id.clone()],
            },
            media_dir.path(),
        )
        .expect("delete");

        assert_eq!(deleted, vec![doomed.id]);
        assert_eq!(story_count(&db), 1);
        let remaining: String = db
            .conn()
            .query_row("SELECT id FROM stories", [], |r| r.get(0))
            .expect("read survivor");
        assert_eq!(remaining, kept.id);
    }

    #[test]
    fn delete_stories_is_all_or_nothing_when_an_id_is_missing() {
        let mut db = fresh_db();
        let existing = create_story(
            &mut db,
            CreateStoryInput {
                title: "Présente".into(),
            },
        )
        .expect("create");
        let media_dir = tempfile::tempdir().expect("tempdir");

        let err = delete_stories(
            &mut db,
            DeleteStoriesInput {
                ids: vec![
                    existing.id.clone(),
                    "0197a5d0-dead-7000-8000-000000000000".into(),
                ],
            },
            media_dir.path(),
        )
        .expect_err("missing id must fail the batch");

        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "story_missing");
        // The confirmed batch was rolled back whole: the existing story
        // survived — never a partial removal under a stale confirmation.
        assert_eq!(story_count(&db), 1);
    }

    #[test]
    fn delete_stories_reclaims_media_files_only_when_unreferenced() {
        let mut db = fresh_db();
        let a = create_story(&mut db, CreateStoryInput { title: "A".into() }).expect("a");
        let b = create_story(&mut db, CreateStoryInput { title: "B".into() }).expect("b");

        let shared_hash = "a".repeat(64);
        let solo_hash = "b".repeat(64);
        insert_asset(&db, &a.id, &shared_hash, "shared.png");
        insert_asset(&db, &b.id, &shared_hash, "shared.png");
        insert_asset(&db, &a.id, &solo_hash, "solo.png");

        let media_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(media_dir.path().join("shared.png"), b"x").expect("shared file");
        std::fs::write(media_dir.path().join("solo.png"), b"x").expect("solo file");

        delete_stories(
            &mut db,
            DeleteStoriesInput { ids: vec![a.id] },
            media_dir.path(),
        )
        .expect("delete a");

        // The content B still references survives; A's exclusive content is
        // reclaimed by the refcounted GC.
        assert!(media_dir.path().join("shared.png").exists());
        assert!(!media_dir.path().join("solo.png").exists());

        delete_stories(
            &mut db,
            DeleteStoriesInput { ids: vec![b.id] },
            media_dir.path(),
        )
        .expect("delete b");
        assert!(!media_dir.path().join("shared.png").exists());
    }

    #[test]
    fn delete_stories_with_an_empty_selection_is_a_noop() {
        let mut db = fresh_db();
        create_story(
            &mut db,
            CreateStoryInput {
                title: "Intacte".into(),
            },
        )
        .expect("create");
        let media_dir = tempfile::tempdir().expect("tempdir");

        let deleted = delete_stories(
            &mut db,
            DeleteStoriesInput { ids: Vec::new() },
            media_dir.path(),
        )
        .expect("noop");

        assert!(deleted.is_empty());
        assert_eq!(story_count(&db), 1);
    }

    #[test]
    fn create_story_persists_canonical_row() {
        let mut db = fresh_db();
        let dto = create_story(
            &mut db,
            CreateStoryInput {
                title: "Le soleil couchant".into(),
            },
        )
        .expect("create");

        // Row shape
        let (
            id,
            title,
            schema_version,
            structure_json,
            content_checksum,
            created_at,
            updated_at,
        ): (String, String, u32, String, String, String, String) = db
            .conn()
            .query_row(
                "SELECT id, title, schema_version, structure_json, content_checksum, created_at, updated_at FROM stories",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .expect("row present");

        assert_eq!(id, dto.id);
        assert_eq!(title, "Le soleil couchant");
        assert_eq!(schema_version, 3);
        assert_eq!(
            structure_json,
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}"
        );
        assert_eq!(content_checksum.len(), 64);
        assert!(content_checksum.chars().all(|c| c.is_ascii_hexdigit()));
        // UUIDv7 hex canonical form, version nibble = 7, variant nibble ∈ {8,9,a,b}
        assert!(
            id.len() == 36 && id.chars().nth(14) == Some('7'),
            "expected UUIDv7 at position 14, got {id}"
        );
        assert_eq!(created_at, updated_at, "first write uses same timestamp");
        // ISO-8601 millisecond UTC shape `YYYY-MM-DDTHH:MM:SS.sssZ`
        assert!(
            created_at.ends_with('Z') && created_at.contains('.') && created_at.len() == 24,
            "unexpected ISO-8601 shape: {created_at}"
        );
    }

    #[test]
    fn create_story_normalizes_title_before_persisting() {
        let mut db = fresh_db();
        let dto = create_story(
            &mut db,
            CreateStoryInput {
                title: "  Café  ".into(),
            },
        )
        .expect("create");
        assert_eq!(dto.title, "Café");

        let stored: String = db
            .conn()
            .query_row("SELECT title FROM stories", [], |row| row.get(0))
            .expect("row");
        assert_eq!(stored, "Café");
    }

    #[test]
    fn create_story_rejects_empty_title_without_inserting() {
        let mut db = fresh_db();
        let err = create_story(
            &mut db,
            CreateStoryInput {
                title: "   ".into(),
            },
        )
        .expect_err("must fail");
        assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);

        let count: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn create_story_rejects_too_long_title_without_inserting() {
        let mut db = fresh_db();
        let long = "a".repeat(121);
        let err = create_story(&mut db, CreateStoryInput { title: long }).expect_err("must fail");
        assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);
        let count: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn create_story_rejects_control_chars_without_inserting() {
        let mut db = fresh_db();
        let err = create_story(
            &mut db,
            CreateStoryInput {
                title: "ligne1\nligne2".into(),
            },
        )
        .expect_err("must fail");
        assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);
        let count: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn create_story_two_consecutive_generate_unique_ids_ordered_chronologically() {
        let mut db = fresh_db();
        let first = create_story(&mut db, CreateStoryInput { title: "A".into() }).expect("first");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = create_story(&mut db, CreateStoryInput { title: "B".into() }).expect("second");

        assert_ne!(first.id, second.id);
        assert!(
            first.id < second.id,
            "UUIDv7 must be monotonically ascending: {} vs {}",
            first.id,
            second.id,
        );
    }

    // ---------------- update_story ----------------

    #[test]
    fn update_story_persists_new_title_and_bumps_updated_at() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Avant".into(),
            },
        )
        .expect("create");

        // Ensure the next ISO-8601 ms timestamp is strictly greater.
        std::thread::sleep(std::time::Duration::from_millis(2));

        let updated = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "Après".into(),
            },
        )
        .expect("update");

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.title, "Après");

        let (stored_title, stored_created_at, stored_updated_at, stored_structure, stored_checksum): (String, String, String, String, String) = db
            .conn()
            .query_row(
                "SELECT title, created_at, updated_at, structure_json, content_checksum FROM stories WHERE id = ?1",
                [&created.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .expect("row");

        assert_eq!(stored_title, "Après");
        assert!(
            stored_updated_at > stored_created_at,
            "updated_at must strictly advance: {stored_updated_at} > {stored_created_at}"
        );
        assert_eq!(
            stored_structure,
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}",
            "structure_json must remain invariant under a title-only update"
        );
        assert_eq!(
            stored_checksum.len(),
            64,
            "content_checksum must remain a 64-char SHA-256 digest"
        );
        assert_eq!(
            stored_updated_at, updated.updated_at,
            "the DTO returns the same updatedAt as what landed in the row"
        );
    }

    #[test]
    fn update_story_is_idempotent_on_identical_title() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Fixe".into(),
            },
        )
        .expect("create");

        std::thread::sleep(std::time::Duration::from_millis(2));
        let first_update = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "Fixe".into(),
            },
        )
        .expect("first update");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second_update = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "Fixe".into(),
            },
        )
        .expect("second update");

        assert_eq!(first_update.title, "Fixe");
        assert_eq!(second_update.title, "Fixe");
        assert!(
            second_update.updated_at > first_update.updated_at,
            "timestamp must still advance on a rewrite of the same title"
        );
    }

    #[test]
    fn update_story_normalizes_title_before_persisting() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Avant".into(),
            },
        )
        .expect("create");
        let updated = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "  Café  ".into(),
            },
        )
        .expect("update");
        assert_eq!(updated.title, "Café");
        let stored: String = db
            .conn()
            .query_row(
                "SELECT title FROM stories WHERE id = ?1",
                [&created.id],
                |row| row.get(0),
            )
            .expect("row");
        assert_eq!(stored, "Café");
    }

    #[test]
    fn update_story_rejects_empty_title_without_touching_row() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Intact".into(),
            },
        )
        .expect("create");
        let (initial_title, initial_updated): (String, String) = db
            .conn()
            .query_row(
                "SELECT title, updated_at FROM stories WHERE id = ?1",
                [&created.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row");

        let err = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "   ".into(),
            },
        )
        .expect_err("must reject empty title");
        assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);

        let (current_title, current_updated): (String, String) = db
            .conn()
            .query_row(
                "SELECT title, updated_at FROM stories WHERE id = ?1",
                [&created.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row");
        assert_eq!(current_title, initial_title);
        assert_eq!(current_updated, initial_updated);
    }

    #[test]
    fn update_story_rejects_too_long_title_without_touching_row() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Court".into(),
            },
        )
        .expect("create");
        let long = "a".repeat(121);
        let err = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: long,
            },
        )
        .expect_err("must reject");
        assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);
        let stored: String = db
            .conn()
            .query_row(
                "SELECT title FROM stories WHERE id = ?1",
                [&created.id],
                |row| row.get(0),
            )
            .expect("row");
        assert_eq!(stored, "Court");
    }

    #[test]
    fn update_story_rejects_control_chars_without_touching_row() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Propre".into(),
            },
        )
        .expect("create");
        let err = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "ligne1\nligne2".into(),
            },
        )
        .expect_err("must reject");
        assert_eq!(err.code, AppErrorCode::InvalidStoryTitle);
        let stored: String = db
            .conn()
            .query_row(
                "SELECT title FROM stories WHERE id = ?1",
                [&created.id],
                |row| row.get(0),
            )
            .expect("row");
        assert_eq!(stored, "Propre");
    }

    #[test]
    fn update_story_returns_library_inconsistent_when_id_missing() {
        let mut db = fresh_db();
        let err = update_story(
            &mut db,
            UpdateStoryInput {
                id: "does-not-exist".into(),
                title: "Titre".into(),
            },
        )
        .expect_err("must fail on missing id");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details populated");
        assert_eq!(details["source"], "story_missing");
        // The `rollback` field is present (the rollback path ran) and
        // absent-of-value means it succeeded. A non-null value would
        // describe the SQLite kind that made the rollback itself fail.
        assert!(details.get("rollback").is_some());
    }

    // AC3: a title write is a REAL write — it settles a pending import
    // review when the canonical story is fully sound.
    #[test]
    fn update_story_resolves_a_pending_review_when_canonical_is_sound() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Avant".into(),
            },
        )
        .expect("create");
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'a.rustory', ?2, 'needs_review', '[{\"aspect\":\"title\",\"category\":\"ambiguous\"}]', '2026-07-06T00:00:00.000Z')",
                rusqlite::params![created.id, "a".repeat(64)],
            )
            .expect("seed pending review");

        let updated = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "Titre corrigé".into(),
            },
        )
        .expect("update");
        assert_eq!(updated.import_state.as_deref(), Some("resolved"));

        let (state, summary): (String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT import_state, findings_summary FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![created.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("provenance row");
        assert_eq!(state, "resolved");
        assert!(summary.is_some(), "findings trace kept");
    }

    // Early-out: a native story (no provenance row) never pays a canonical
    // validation for its title autosave — proven by a corrupt structure the
    // early-out must never parse. The ACK carries an explicit null.
    #[test]
    fn update_story_on_a_native_early_outs_and_acks_null() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Native".into(),
            },
        )
        .expect("create");
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = 'not json' WHERE id = ?1",
                rusqlite::params![created.id],
            )
            .expect("corrupt structure");

        let updated = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id,
                title: "Toujours modifiable".into(),
            },
        )
        .expect("a native title save never validates the canonical body");
        assert_eq!(updated.import_state, None);
    }

    // The forged two-table case: the pack takes precedence (TitleOnly) — the
    // title stays editable but the forged local review is NOT settled.
    #[test]
    fn update_story_on_a_forged_two_table_story_does_not_resolve() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Forgé".into(),
            },
        )
        .expect("create");
        db.conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
                 VALUES (?1, '019739b2-0000-7000-8000-000000000000', '0123456789abcdef0123456789abcdef', '2026-07-06T00:00:00.000Z', 5, 18, ?2, 'lunii')",
                rusqlite::params![created.id, "ab".repeat(32)],
            )
            .expect("pack provenance");
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'a.rustory', ?2, 'needs_review', '[{\"aspect\":\"title\",\"category\":\"ambiguous\"}]', '2026-07-06T00:00:00.000Z')",
                rusqlite::params![created.id, "a".repeat(64)],
            )
            .expect("forged local provenance");

        let updated = update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "Renommée localement".into(),
            },
        )
        .expect("the title stays editable on a pack");
        assert_eq!(updated.import_state, None, "None unless FULL scope");

        let state: String = db
            .conn()
            .query_row(
                "SELECT import_state FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![created.id],
                |r| r.get(0),
            )
            .expect("row");
        assert_eq!(state, "needs_review", "the forged review is NOT settled");
    }

    #[test]
    fn update_story_preserves_structure_json_and_checksum() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Intact".into(),
            },
        )
        .expect("create");
        let (initial_structure, initial_checksum): (String, String) = db
            .conn()
            .query_row(
                "SELECT structure_json, content_checksum FROM stories WHERE id = ?1",
                [&created.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row");

        update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "Nouveau titre".into(),
            },
        )
        .expect("update");

        let (structure, checksum): (String, String) = db
            .conn()
            .query_row(
                "SELECT structure_json, content_checksum FROM stories WHERE id = ?1",
                [&created.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row");
        assert_eq!(structure, initial_structure);
        assert_eq!(checksum, initial_checksum);
    }

    // ---------------- get_story_detail ----------------

    #[test]
    fn get_story_detail_returns_none_for_missing_id() {
        let db = fresh_db();
        let detail = get_story_detail(&db, &std::env::temp_dir(), "missing-id", None).expect("ok");
        assert!(detail.is_none());
    }

    #[test]
    fn get_story_detail_returns_persisted_row() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Brouillon".into(),
            },
        )
        .expect("create");

        let detail = get_story_detail(&db, &std::env::temp_dir(), &created.id, None)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.id, created.id);
        assert_eq!(detail.title, "Brouillon");
        assert_eq!(detail.schema_version, 3);
        assert_eq!(
            detail.structure_json,
            "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}"
        );
        assert_eq!(detail.content_checksum.len(), 64);
        assert!(detail
            .content_checksum
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
        assert_eq!(detail.created_at, detail.updated_at);
    }

    #[test]
    fn update_story_constraint_violation_maps_to_invalid_story_title() {
        // Defense-in-depth path: the Rust validator already rejects empty
        // / control-char / denylist titles before touching SQL, so the
        // SQLite CHECK guard should never trip in practice. This test
        // proves the mapping is consistent with `create_story` when the
        // guard does trip — both paths surface INVALID_STORY_TITLE so
        // the UI shows the same canonical reason text.
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Intact".into(),
            },
        )
        .expect("create");
        // Drive a CHECK failure by bypassing the Rust validator via a
        // direct SQL statement that mirrors what `update_story` runs but
        // with a blank title — proves the schema still catches it.
        let err = db.conn().execute(
            "UPDATE stories SET title = '   ', updated_at = '2026-04-24T00:00:00.000Z' WHERE id = ?1",
            rusqlite::params![&created.id],
        ).expect_err("blank UPDATE must trip CHECK");
        let mapped = map_update_transport_error(&err, "update", &created.id);
        assert_eq!(mapped.code, AppErrorCode::InvalidStoryTitle);
        let details = mapped.details.as_ref().expect("details");
        assert_eq!(details["source"], "sqlite_update");
        assert_eq!(details["kind"], "constraint_violation");
        assert_eq!(details["stage"], "update");
        assert_eq!(details["id"], created.id);
    }

    // Producer-side lock of the FR21 projection: editScope + importState per
    // provenance, the None-unless-Full rule, and their survival under a
    // Blocking canonical degradation.
    #[test]
    fn get_story_detail_projects_edit_scope_and_import_state_per_provenance() {
        let mut db = fresh_db();
        let tmp = std::env::temp_dir();

        // (a) Native: full scope, no import state.
        let native = create_story(
            &mut db,
            CreateStoryInput {
                title: "Native".into(),
            },
        )
        .expect("create")
        .id;
        let detail = get_story_detail(&db, &tmp, &native, None)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.edit_scope, "full");
        assert!(detail.editable);
        assert_eq!(detail.import_state, None);

        // (b) `.rustory` import with a pending review: full + needsReview.
        let imported = create_story(
            &mut db,
            CreateStoryInput {
                title: "Importée".into(),
            },
        )
        .expect("create")
        .id;
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'a.rustory', ?2, 'needs_review', '[{\"aspect\":\"title\",\"category\":\"ambiguous\"}]', '2026-07-06T00:00:00.000Z')",
                rusqlite::params![imported, "a".repeat(64)],
            )
            .expect("local provenance");
        let detail = get_story_detail(&db, &tmp, &imported, None)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.edit_scope, "full");
        assert!(detail.editable, "a .rustory import is fully editable");
        assert_eq!(detail.import_state.as_deref(), Some("needsReview"));

        // (c) Device pack: titleOnly, no import state.
        let pack = create_story(
            &mut db,
            CreateStoryInput {
                title: "Pack".into(),
            },
        )
        .expect("create")
        .id;
        db.conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
                 VALUES (?1, '019739b2-0000-7000-8000-000000000001', '0123456789abcdef0123456789abcdef', '2026-07-06T00:00:00.000Z', 5, 18, ?2, 'lunii')",
                rusqlite::params![pack, "ab".repeat(32)],
            )
            .expect("pack provenance");
        let detail = get_story_detail(&db, &tmp, &pack, None)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.edit_scope, "titleOnly");
        assert!(!detail.editable);
        assert_eq!(detail.import_state, None);

        // (d) Forged two-table story: the pack takes precedence — importState
        // is NEVER projected outside the full scope, even with a local row.
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'b.rustory', ?2, 'needs_review', '[{\"aspect\":\"title\",\"category\":\"ambiguous\"}]', '2026-07-06T00:00:00.000Z')",
                rusqlite::params![pack, "b".repeat(64)],
            )
            .expect("forged local provenance");
        let detail = get_story_detail(&db, &tmp, &pack, None)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.edit_scope, "titleOnly");
        assert_eq!(detail.import_state, None, "None unless FULL scope");

        // (e) A Blocking canonical degradation keeps BOTH FR21 fields
        // projected (story metadata, not canonical content).
        db.conn()
            .execute(
                "UPDATE stories SET content_checksum = ?1 WHERE id = ?2",
                rusqlite::params!["0".repeat(64), imported],
            )
            .expect("corrupt checksum");
        let detail = get_story_detail(&db, &tmp, &imported, None)
            .expect("ok")
            .expect("some");
        assert!(detail.structure.is_none(), "canonical degrades");
        assert!(detail.node.is_none(), "canonical degrades together");
        assert_eq!(detail.edit_scope, "full", "metadata survives");
        assert_eq!(
            detail.import_state.as_deref(),
            Some("needsReview"),
            "the review chip stays honest even degraded"
        );
    }

    // A transient failure of the review-provenance read during a story OPEN
    // surfaces a READ-flavored error — never the write copy of the ACK
    // paths (no label may be false in one of its contexts).
    #[test]
    fn get_story_detail_maps_a_review_read_failure_to_a_read_error() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Ouverture".into(),
            },
        )
        .expect("create");
        db.conn()
            .execute("DROP TABLE story_local_imports", [])
            .expect("drop the provenance table");

        let err = get_story_detail(&db, &std::env::temp_dir(), &created.id, None)
            .expect_err("the review read fails");
        assert_eq!(err.code, AppErrorCode::LocalStorageUnavailable);
        assert_eq!(
            err.message, "Rustory n'a pas pu ouvrir cette histoire.",
            "a story OPEN must never speak about a write"
        );
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "sqlite_import_review");
        assert_eq!(details["stage"], "detail_read");
    }

    #[test]
    fn get_story_detail_reflects_latest_update() {
        let mut db = fresh_db();
        let created = create_story(
            &mut db,
            CreateStoryInput {
                title: "Avant".into(),
            },
        )
        .expect("create");
        std::thread::sleep(std::time::Duration::from_millis(2));
        update_story(
            &mut db,
            UpdateStoryInput {
                id: created.id.clone(),
                title: "Après".into(),
            },
        )
        .expect("update");

        let detail = get_story_detail(&db, &std::env::temp_dir(), &created.id, None)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.title, "Après");
        assert!(
            detail.updated_at > detail.created_at,
            "updated_at must strictly advance after an update"
        );
    }
}
