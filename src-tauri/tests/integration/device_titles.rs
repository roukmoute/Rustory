//! End-to-end title recognition against a REAL on-disk SQLite database.
//!
//! Proves the durability guarantees the in-memory unit tests cannot: the
//! v4 migration applies on a file-backed (WAL) database, and a user-typed
//! title SURVIVES a close/reopen — the on-disk equivalent of unplugging and
//! replugging the Lunii (AC2). Also exercises the full resolution priority
//! (User > Official > Unofficial) and the offline local-library inference
//! (Phase D) through the public application API.

use rustory_lib::application::device::title::{
    count_official_catalog, replace_official_catalog, resolve_local_truth, set_user_title,
    OfficialCatalogEntry,
};
use rustory_lib::domain::device::title::PackTitleSource;
use rustory_lib::infrastructure::db::{open_at, run_migrations, DbHandle};
use tempfile::TempDir;

const UUID_USER: &str = "11111111-1111-1111-1111-1111111111aa";
const UUID_OFFICIAL: &str = "22222222-2222-2222-2222-2222222222bb";
const UUID_IMPORTED: &str = "33333333-3333-3333-3333-3333333333cc";

fn open_db(dir: &TempDir) -> DbHandle {
    let path = dir.path().join("rustory.sqlite");
    let mut db = open_at(&path).expect("open db");
    run_migrations(&mut db).expect("migrate");
    db
}

fn seed_imported_story(db: &DbHandle, story_id: &str, pack_uuid: &str, title: &str) {
    db.conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, ?2, 1, '{\"schemaVersion\":1,\"nodes\":[]}', \
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
fn user_title_survives_database_reopen_like_an_unplug_replug() {
    let dir = TempDir::new().expect("tempdir");
    {
        let mut db = open_db(&dir);
        set_user_title(&mut db, UUID_USER, "Le château de cartes").expect("set");
    } // db dropped — the on-disk file is the only state left, like a power-off.

    // Reopen the SAME database (the app relaunched / the Lunii was replugged).
    let db = open_db(&dir);
    let truth = resolve_local_truth(&db, &[UUID_USER.to_string()]).expect("resolve");
    let title = truth.titles.get(UUID_USER).expect("title persisted");
    assert_eq!(title.title, "Le château de cartes");
    assert_eq!(title.source, PackTitleSource::User);
}

#[test]
fn full_priority_resolution_against_a_real_database() {
    let dir = TempDir::new().expect("tempdir");
    let mut db = open_db(&dir);

    // UUID_OFFICIAL: only an official catalog entry.
    replace_official_catalog(
        &mut db,
        &[OfficialCatalogEntry {
            pack_uuid: UUID_OFFICIAL.into(),
            title: "Suzanne et Gaston".into(),
            thumbnail: Some("https://example/cover.png".into()),
        }],
    )
    .expect("catalog");
    assert_eq!(count_official_catalog(&db).expect("count"), 1);

    // UUID_IMPORTED: only a local-library link (Phase D, offline).
    seed_imported_story(&db, "story-imported", UUID_IMPORTED, "Mon histoire à moi");

    // UUID_USER: an official entry AND a user override → user must win.
    replace_official_catalog(
        &mut db,
        &[
            OfficialCatalogEntry {
                pack_uuid: UUID_OFFICIAL.into(),
                title: "Suzanne et Gaston".into(),
                thumbnail: Some("https://example/cover.png".into()),
            },
            OfficialCatalogEntry {
                pack_uuid: UUID_USER.into(),
                title: "Titre officiel ignoré".into(),
                thumbnail: None,
            },
        ],
    )
    .expect("catalog refresh");
    set_user_title(&mut db, UUID_USER, "Je l'ai renommée").expect("user title");

    let truth = resolve_local_truth(
        &db,
        &[
            UUID_USER.to_string(),
            UUID_OFFICIAL.to_string(),
            UUID_IMPORTED.to_string(),
        ],
    )
    .expect("resolve");

    let user = truth.titles.get(UUID_USER).expect("user");
    assert_eq!(user.title, "Je l'ai renommée");
    assert_eq!(user.source, PackTitleSource::User);

    let official = truth.titles.get(UUID_OFFICIAL).expect("official");
    assert_eq!(official.title, "Suzanne et Gaston");
    assert_eq!(official.source, PackTitleSource::Official);
    assert_eq!(
        official.thumbnail.as_deref(),
        Some("https://example/cover.png")
    );

    let imported = truth.titles.get(UUID_IMPORTED).expect("imported");
    assert_eq!(imported.title, "Mon histoire à moi");
    assert_eq!(imported.source, PackTitleSource::Unofficial);
    assert!(truth.imported.contains(UUID_IMPORTED));
}

#[test]
fn refreshing_the_official_catalog_never_drops_user_titles() {
    let dir = TempDir::new().expect("tempdir");
    let mut db = open_db(&dir);

    set_user_title(&mut db, UUID_USER, "Nom durable").expect("user title");
    // A first then a second catalog snapshot (disposable cache replaced).
    replace_official_catalog(
        &mut db,
        &[OfficialCatalogEntry {
            pack_uuid: UUID_OFFICIAL.into(),
            title: "Ancien".into(),
            thumbnail: None,
        }],
    )
    .expect("first catalog");
    replace_official_catalog(
        &mut db,
        &[OfficialCatalogEntry {
            pack_uuid: UUID_OFFICIAL.into(),
            title: "Nouveau".into(),
            thumbnail: None,
        }],
    )
    .expect("second catalog");

    let truth = resolve_local_truth(&db, &[UUID_USER.to_string(), UUID_OFFICIAL.to_string()])
        .expect("resolve");
    assert_eq!(
        truth.titles.get(UUID_USER).expect("user").title,
        "Nom durable"
    );
    assert_eq!(
        truth.titles.get(UUID_OFFICIAL).expect("official").title,
        "Nouveau"
    );
}
