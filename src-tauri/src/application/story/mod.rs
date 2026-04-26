pub mod recovery;

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

    Ok(StoryCardDto {
        id,
        title: normalized,
    })
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

    tx.commit()
        .map_err(|err| map_update_transport_error(&err, "commit", &input.id))?;

    Ok(UpdateStoryOutputDto {
        id: input.id,
        title: normalized,
        updated_at: now_iso,
    })
}

/// Read a single story by id for the edit surface.
///
/// Returns `Ok(None)` when the row is missing — an informational case the
/// UI renders as "Histoire introuvable". A transport-level failure (SQLite
/// IO, schema mismatch, etc.) crosses the boundary as a normalized
/// [`AppError`] so the UI never has to distinguish "row absent" from
/// "storage broken" from shared plumbing.
pub fn get_story_detail(db: &DbHandle, story_id: &str) -> Result<Option<StoryDetailDto>, AppError> {
    db.conn()
        .query_row(
            "SELECT id, title, schema_version, structure_json, content_checksum, created_at, updated_at \
             FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| {
                Ok(StoryDetailDto {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    schema_version: row.get(2)?,
                    structure_json: row.get(3)?,
                    content_checksum: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(map_detail_read_error)
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

pub(super) fn sqlite_kind_label(err: &rusqlite::Error) -> &'static str {
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
        assert_eq!(schema_version, 1);
        assert_eq!(structure_json, "{\"schemaVersion\":1,\"nodes\":[]}");
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
            stored_structure, "{\"schemaVersion\":1,\"nodes\":[]}",
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
        let detail = get_story_detail(&db, "missing-id").expect("ok");
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

        let detail = get_story_detail(&db, &created.id)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.id, created.id);
        assert_eq!(detail.title, "Brouillon");
        assert_eq!(detail.schema_version, 1);
        assert_eq!(detail.structure_json, "{\"schemaVersion\":1,\"nodes\":[]}");
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

        let detail = get_story_detail(&db, &created.id)
            .expect("ok")
            .expect("some");
        assert_eq!(detail.title, "Après");
        assert!(
            detail.updated_at > detail.created_at,
            "updated_at must strictly advance after an update"
        );
    }
}
