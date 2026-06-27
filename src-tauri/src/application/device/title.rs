//! Device-story title recognition — application service.
//!
//! Bridges the pure resolution rule ([`crate::domain::device::title`]) to
//! SQLite. Two responsibilities:
//!
//! 1. **Resolve** the title of each device pack UUID by gathering the
//!    per-source candidates and applying the priority
//!    `User > Official > Unofficial` ([`resolve_local_truth`]). The
//!    `Unofficial` candidate is derived OFFLINE from the local library:
//!    the title of a story already linked to that pack UUID through the
//!    `story_imports` provenance row (Phase D). This is what keeps a story
//!    the user imported (or, later, created and transferred) from ever
//!    showing as "non reconnue".
//! 2. **Persist** a user-typed title ([`set_user_title`]) and replace the
//!    official catalog cache wholesale ([`replace_official_catalog`]).
//!
//! Authority lives here, in Rust: the device DTO is enriched with the
//! resolved `title` + `source` at the boundary; the frontend never
//! recomposes the truth.

use std::collections::{HashMap, HashSet};

use crate::application::story::now_iso_ms;
use crate::domain::device::is_canonical_pack_uuid;
use crate::domain::device::title::{PackTitle, PackTitleCandidates, PackTitleSource, TitleValue};
use crate::domain::shared::AppError;
use crate::domain::story::{map_error, normalize_title, validate_title};
use crate::infrastructure::db::DbHandle;

/// One official catalog entry ready to be cached. The title is already
/// normalized + validated by the catalog parser; the UUID is canonical
/// lowercase. Defined here (not in the catalog module) so the persistence
/// layer owns the shape it writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialCatalogEntry {
    pub pack_uuid: String,
    pub title: String,
    pub thumbnail: Option<String>,
}

/// Local truth composed onto the device inventory at the read boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalTruth {
    /// Device pack UUIDs that already have a local copy (an import
    /// provenance row exists). Drives the `alreadyImported` stamp.
    pub imported: HashSet<String>,
    /// Resolved titles keyed by device pack UUID. A UUID absent from the
    /// map is genuinely unrecognized ("non reconnue").
    pub titles: HashMap<String, PackTitle>,
}

/// Gather every local fact about the given device pack UUIDs in ONE place,
/// under the caller's scoped DB lock: which packs are already imported, and
/// the resolved title (with provenance) of each.
///
/// Fail-closed on a DB read failure: a broken local store surfaces the
/// recoverable error rather than silently claiming "nothing imported / all
/// unknown", which would both mislead the user and invite a duplicate copy.
/// Max device UUIDs bound into one `IN (?,…)` statement. Comfortably under
/// SQLite's parameter limit (historically 999, 32766 on bundled builds) so
/// the resolution never trips it even on a maximal `.pi` inventory.
const MAX_UUIDS_PER_QUERY: usize = 900;

pub fn resolve_local_truth(db: &DbHandle, uuids: &[String]) -> Result<LocalTruth, AppError> {
    if uuids.is_empty() {
        return Ok(LocalTruth::default());
    }

    let mut candidates: HashMap<String, PackTitleCandidates> = HashMap::new();
    let mut imported: HashSet<String> = HashSet::new();

    // Query in bounded chunks: one bound parameter per device UUID, capped
    // well under SQLite's variable limit. A maximal `.pi` inventory (~4096
    // packs) would otherwise build a single 4096-parameter `IN (…)`; chunking
    // keeps each statement safe regardless of the bundled SQLite's limit.
    for chunk in uuids.chunks(MAX_UUIDS_PER_QUERY) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");

        // 1. Stored candidates: user-typed titles and the cached official
        //    catalog (and, in a later story, a community 'unofficial' index).
        {
            let sql = format!(
                "SELECT pack_uuid, source, title, thumbnail FROM pack_metadata \
                 WHERE pack_uuid IN ({placeholders})"
            );
            let mut stmt = db
                .conn()
                .prepare(&sql)
                .map_err(|_| read_error("prepare_metadata"))?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })
                .map_err(|_| read_error("query_metadata"))?;
            for row in rows {
                let (uuid, source, title, thumbnail) =
                    row.map_err(|_| read_error("row_metadata"))?;
                let entry = candidates.entry(uuid).or_default();
                let value = TitleValue { title, thumbnail };
                match PackTitleSource::from_tag(&source) {
                    Some(PackTitleSource::User) => entry.user = Some(value),
                    Some(PackTitleSource::Official) => entry.official = Some(value),
                    Some(PackTitleSource::Unofficial) => entry.unofficial = Some(value),
                    // A corrupt `source` token degrades that row to nothing —
                    // never panics, never mislabels.
                    None => {}
                }
            }
        }

        // 2. Phase D — unofficial title inferred from the local library: the
        //    title of a local story already linked to this pack UUID. This
        //    OVERRIDES any community 'unofficial' row gathered above: the
        //    user's own library is more trustworthy than a community guess.
        //    The same query yields the `imported` set for `alreadyImported`.
        {
            let sql = format!(
                "SELECT si.pack_uuid, s.title FROM story_imports si \
                 JOIN stories s ON s.id = si.story_id \
                 WHERE si.pack_uuid IN ({placeholders})"
            );
            let mut stmt = db
                .conn()
                .prepare(&sql)
                .map_err(|_| read_error("prepare_import"))?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|_| read_error("query_import"))?;
            for row in rows {
                let (uuid, title) = row.map_err(|_| read_error("row_import"))?;
                imported.insert(uuid.clone());
                candidates.entry(uuid).or_default().unofficial = Some(TitleValue {
                    title,
                    thumbnail: None,
                });
            }
        }
    }

    let mut titles = HashMap::new();
    for (uuid, candidate) in candidates {
        if let Some(title) = candidate.resolve() {
            titles.insert(uuid, title);
        }
    }

    Ok(LocalTruth { imported, titles })
}

/// Persist (or replace) a user-typed title for a device pack. Reuses the
/// local-story title rules (NFC + trim + denylist + ≤120) so a device name
/// is held to exactly the same bar. A previously-stored title is updated in
/// place; a user title is `source = User`, so the resolution order
/// guarantees it is never silently overwritten by later recognition.
pub fn set_user_title(
    db: &mut DbHandle,
    pack_uuid: &str,
    raw_title: &str,
) -> Result<PackTitle, AppError> {
    if !is_canonical_pack_uuid(pack_uuid) {
        return Err(invalid_pack_uuid_error());
    }
    let title = normalize_title(raw_title);
    validate_title(&title).map_err(map_error)?;
    let now = now_iso_ms()?;

    db.conn()
        .execute(
            "INSERT INTO pack_metadata (pack_uuid, source, title, thumbnail, updated_at) \
             VALUES (?1, 'user', ?2, NULL, ?3) \
             ON CONFLICT(pack_uuid, source) \
             DO UPDATE SET title = excluded.title, updated_at = excluded.updated_at",
            rusqlite::params![pack_uuid, &title, &now],
        )
        .map_err(|_| write_error("set_user_title"))?;

    Ok(PackTitle {
        title,
        source: PackTitleSource::User,
        thumbnail: None,
    })
}

/// Replace the cached official catalog wholesale: drop every
/// `source = 'official'` row, then insert the supplied entries in a single
/// transaction. The cache is DISPOSABLE — a refresh fully supersedes the
/// previous snapshot — and never touches `user` rows. Returns the number of
/// official entries now cached.
pub fn replace_official_catalog(
    db: &mut DbHandle,
    entries: &[OfficialCatalogEntry],
) -> Result<u32, AppError> {
    let now = now_iso_ms()?;
    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| write_error("catalog_begin"))?;

    tx.execute("DELETE FROM pack_metadata WHERE source = 'official'", [])
        .map_err(|_| write_error("catalog_clear"))?;

    let mut inserted = 0u32;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO pack_metadata (pack_uuid, source, title, thumbnail, updated_at) \
                 VALUES (?1, 'official', ?2, ?3, ?4) \
                 ON CONFLICT(pack_uuid, source) \
                 DO UPDATE SET title = excluded.title, thumbnail = excluded.thumbnail, \
                               updated_at = excluded.updated_at",
            )
            .map_err(|_| write_error("catalog_prepare"))?;
        for entry in entries {
            stmt.execute(rusqlite::params![
                &entry.pack_uuid,
                &entry.title,
                &entry.thumbnail,
                &now,
            ])
            .map_err(|_| write_error("catalog_insert"))?;
            inserted += 1;
        }
    }

    tx.commit().map_err(|_| write_error("catalog_commit"))?;
    Ok(inserted)
}

/// Count the official catalog entries currently cached. Drives the
/// "X titres officiels en cache" hint without surfacing the whole table.
pub fn count_official_catalog(db: &DbHandle) -> Result<u32, AppError> {
    db.conn()
        .query_row(
            "SELECT COUNT(*) FROM pack_metadata WHERE source = 'official'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| read_error("count_official"))
}

fn read_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Reconnaissance des titres indisponible: vérifie le disque local et réessaie.",
        "Réessaie la lecture ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "pack_metadata_read",
        "stage": stage,
    }))
}

fn write_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Enregistrement du titre impossible: vérifie le disque local et réessaie.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "pack_metadata_write",
        "stage": stage,
    }))
}

fn invalid_pack_uuid_error() -> AppError {
    AppError::library_inconsistent(
        "Action impossible: histoire d'appareil introuvable.",
        "Relance la lecture de la bibliothèque de l'appareil puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "device_story_title",
        "cause": "invalid_pack_uuid",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db;

    const UUID_A: &str = "11111111-1111-1111-1111-1111111111aa";
    const UUID_B: &str = "22222222-2222-2222-2222-2222222222bb";
    const UUID_C: &str = "33333333-3333-3333-3333-3333333333cc";

    fn fresh_db() -> DbHandle {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        handle
    }

    fn insert_official(db: &DbHandle, uuid: &str, title: &str) {
        db.conn()
            .execute(
                "INSERT INTO pack_metadata (pack_uuid, source, title, thumbnail, updated_at) \
                 VALUES (?1, 'official', ?2, NULL, '2026-06-16T00:00:00.000Z')",
                rusqlite::params![uuid, title],
            )
            .expect("insert official");
    }

    /// Seed a `stories` row + a `story_imports` provenance row linking it to
    /// `pack_uuid`, mirroring what the import service writes.
    fn insert_imported_story(db: &DbHandle, story_id: &str, pack_uuid: &str, title: &str) {
        db.conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES (?1, ?2, 1, '{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}', \
                 '0000000000000000000000000000000000000000000000000000000000000000', \
                 '2026-06-16T00:00:00.000Z', '2026-06-16T00:00:00.000Z')",
                rusqlite::params![story_id, title],
            )
            .expect("insert story");
        db.conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
                 VALUES (?1, ?2, '0123456789abcdef0123456789abcdef', '2026-06-16T00:00:00.000Z', 5, 18, ?3)",
                rusqlite::params![story_id, pack_uuid, "ab".repeat(32)],
            )
            .expect("insert provenance");
    }

    #[test]
    fn empty_uuid_set_resolves_to_empty_local_truth() {
        let db = fresh_db();
        let truth = resolve_local_truth(&db, &[]).expect("resolve");
        assert_eq!(truth, LocalTruth::default());
    }

    #[test]
    fn unknown_pack_has_no_title_and_is_not_imported() {
        let db = fresh_db();
        let truth = resolve_local_truth(&db, &[UUID_A.to_string()]).expect("resolve");
        assert!(truth.titles.is_empty());
        assert!(truth.imported.is_empty());
    }

    #[test]
    fn imported_story_title_is_surfaced_as_unofficial_and_marks_imported() {
        let db = fresh_db();
        insert_imported_story(&db, "story-1", UUID_A, "La Sorcière du placard");
        let truth = resolve_local_truth(&db, &[UUID_A.to_string()]).expect("resolve");
        let title = truth.titles.get(UUID_A).expect("title");
        assert_eq!(title.title, "La Sorcière du placard");
        assert_eq!(title.source, PackTitleSource::Unofficial);
        assert!(truth.imported.contains(UUID_A));
    }

    #[test]
    fn official_catalog_title_is_surfaced_when_no_local_link() {
        let db = fresh_db();
        insert_official(&db, UUID_B, "Le Loup");
        let truth = resolve_local_truth(&db, &[UUID_B.to_string()]).expect("resolve");
        let title = truth.titles.get(UUID_B).expect("title");
        assert_eq!(title.title, "Le Loup");
        assert_eq!(title.source, PackTitleSource::Official);
        assert!(!truth.imported.contains(UUID_B));
    }

    #[test]
    fn resolves_across_more_than_one_query_chunk() {
        // More device UUIDs than fit in a single IN(...) chunk: the official
        // title for a pack in a LATER chunk must still resolve, proving the
        // chunking covers the whole input.
        let db = fresh_db();
        let mut uuids: Vec<String> = (0..2000)
            .map(|i| format!("00000000-0000-0000-0000-{i:012x}"))
            .collect();
        let target = uuids[1500].clone();
        insert_official(&db, &target, "Trouvé au-delà du premier lot");
        // A UUID present nowhere in the DB to confirm misses stay absent.
        uuids.push("ffffffff-ffff-ffff-ffff-ffffffffffff".to_string());

        let truth = resolve_local_truth(&db, &uuids).expect("resolve");
        assert_eq!(truth.titles.len(), 1);
        assert_eq!(
            truth.titles.get(&target).expect("target").title,
            "Trouvé au-delà du premier lot"
        );
    }

    #[test]
    fn user_title_outranks_official_for_the_same_pack() {
        let db = fresh_db();
        insert_official(&db, UUID_A, "Titre officiel");
        let mut handle = db;
        set_user_title(&mut handle, UUID_A, "  Mon préféré  ").expect("set");
        let truth = resolve_local_truth(&handle, &[UUID_A.to_string()]).expect("resolve");
        let title = truth.titles.get(UUID_A).expect("title");
        assert_eq!(title.title, "Mon préféré"); // normalized (trimmed)
        assert_eq!(title.source, PackTitleSource::User);
    }

    #[test]
    fn user_title_outranks_imported_local_title() {
        let mut db = fresh_db();
        insert_imported_story(&db, "story-1", UUID_A, "Titre importé");
        set_user_title(&mut db, UUID_A, "Mon titre").expect("set");
        let truth = resolve_local_truth(&db, &[UUID_A.to_string()]).expect("resolve");
        let title = truth.titles.get(UUID_A).expect("title");
        assert_eq!(title.title, "Mon titre");
        assert_eq!(title.source, PackTitleSource::User);
        // The import link still marks the pack as already imported.
        assert!(truth.imported.contains(UUID_A));
    }

    #[test]
    fn set_user_title_is_idempotent_update_not_insert() {
        let mut db = fresh_db();
        set_user_title(&mut db, UUID_A, "Premier").expect("first");
        set_user_title(&mut db, UUID_A, "Second").expect("second");
        let count: u32 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM pack_metadata WHERE pack_uuid = ?1 AND source = 'user'",
                rusqlite::params![UUID_A],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 1, "a re-name must update, never add a row");
        let truth = resolve_local_truth(&db, &[UUID_A.to_string()]).expect("resolve");
        assert_eq!(truth.titles.get(UUID_A).expect("title").title, "Second");
    }

    #[test]
    fn set_user_title_rejects_invalid_title_and_uuid() {
        let mut db = fresh_db();
        let too_long = "a".repeat(121);
        let err = set_user_title(&mut db, UUID_A, &too_long).expect_err("too long");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::InvalidStoryTitle
        );

        let err = set_user_title(&mut db, "not-a-uuid", "Titre").expect_err("bad uuid");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::LibraryInconsistent
        );
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["cause"], "invalid_pack_uuid");
    }

    #[test]
    fn replace_official_catalog_supersedes_previous_snapshot_and_keeps_user_rows() {
        let mut db = fresh_db();
        set_user_title(&mut db, UUID_A, "Nommée par moi").expect("user title");
        insert_official(&db, UUID_B, "Ancien titre officiel");

        let inserted = replace_official_catalog(
            &mut db,
            &[
                OfficialCatalogEntry {
                    pack_uuid: UUID_B.into(),
                    title: "Nouveau titre".into(),
                    thumbnail: Some("https://example/cover.png".into()),
                },
                OfficialCatalogEntry {
                    pack_uuid: UUID_C.into(),
                    title: "Autre".into(),
                    thumbnail: None,
                },
            ],
        )
        .expect("replace");
        assert_eq!(inserted, 2);
        assert_eq!(count_official_catalog(&db).expect("count"), 2);

        let truth = resolve_local_truth(
            &db,
            &[UUID_A.to_string(), UUID_B.to_string(), UUID_C.to_string()],
        )
        .expect("resolve");
        // User row untouched by the catalog refresh.
        assert_eq!(
            truth.titles.get(UUID_A).expect("a").source,
            PackTitleSource::User
        );
        // Official row replaced (new title + cover).
        let b = truth.titles.get(UUID_B).expect("b");
        assert_eq!(b.title, "Nouveau titre");
        assert_eq!(b.thumbnail.as_deref(), Some("https://example/cover.png"));
        assert_eq!(b.source, PackTitleSource::Official);
        assert_eq!(truth.titles.get(UUID_C).expect("c").title, "Autre");
    }

    #[test]
    fn replace_official_catalog_with_no_entries_clears_the_cache() {
        let mut db = fresh_db();
        insert_official(&db, UUID_B, "Titre");
        let inserted = replace_official_catalog(&mut db, &[]).expect("replace empty");
        assert_eq!(inserted, 0);
        assert_eq!(count_official_catalog(&db).expect("count"), 0);
    }
}
