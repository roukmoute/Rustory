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
use crate::ipc::dto::StoryCardDto;

/// Input accepted by the `create_story` application service. Kept separate
/// from the IPC DTO so the application layer never imports wire-format
/// concerns.
pub struct CreateStoryInput {
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

    let now_iso = OffsetDateTime::now_utc().format(&ISO8601_MS).map_err(|_| {
        AppError::local_storage_unavailable(
            "Rustory n'a pas pu générer un horodatage local valide.",
            "Vérifie l'horloge système puis relance l'application.",
        )
    })?;

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
}
