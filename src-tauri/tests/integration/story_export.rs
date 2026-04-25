use std::fs;

use rustory_lib::application::import_export::{export_story, ExportStoryInput};
use rustory_lib::application::story::{create_story, get_story_detail, CreateStoryInput};
use rustory_lib::domain::export::RustoryArtifactV1;
use rustory_lib::infrastructure::db;
use rustory_lib::infrastructure::filesystem::resolve_db_path;
use tempfile::TempDir;

#[test]
fn exports_persisted_story_and_produces_readable_artifact() {
    let storage = TempDir::new().expect("tempdir");
    let db_path = resolve_db_path(storage.path());
    let mut handle = db::open_at(&db_path).expect("open db");
    db::run_migrations(&mut handle).expect("migrate");

    let created = create_story(
        &mut handle,
        CreateStoryInput {
            title: "Le Soleil Couchant".into(),
        },
    )
    .expect("create story");
    let detail = get_story_detail(&handle, &created.id)
        .expect("read detail")
        .expect("detail present");

    let destination = storage.path().join("histoire.rustory");
    let output = export_story(ExportStoryInput {
        detail,
        destination_path: destination.clone(),
    })
    .expect("export");

    let bytes = fs::read(&destination).expect("read artifact");
    let parsed: RustoryArtifactV1 =
        serde_json::from_slice(&bytes).expect("artifact must parse as Rustory v1");

    assert_eq!(parsed.story.title, "Le Soleil Couchant");
    assert_eq!(parsed.story.schema_version, 1);
    assert_eq!(
        parsed.story.structure_json,
        "{\"schemaVersion\":1,\"nodes\":[]}"
    );
    assert_eq!(output.bytes_written as usize, bytes.len());
    // The canonicalized destination is the same file the test wrote to
    // — both sides canonicalize, so the comparison is stable regardless
    // of temp-dir path aliasing (e.g. `/tmp` symlinked to `/private/tmp`
    // on macOS).
    let expected = fs::canonicalize(&destination).expect("canonicalize");
    assert_eq!(output.destination_path, expected.to_string_lossy());
}

#[test]
fn export_does_not_touch_stories_row() {
    let storage = TempDir::new().expect("tempdir");
    let db_path = resolve_db_path(storage.path());
    let mut handle = db::open_at(&db_path).expect("open db");
    db::run_migrations(&mut handle).expect("migrate");

    let created = create_story(
        &mut handle,
        CreateStoryInput {
            title: "Immuable".into(),
        },
    )
    .expect("create");
    let detail = get_story_detail(&handle, &created.id)
        .expect("read detail")
        .expect("detail present");

    let before: (String, String, String, String) = handle
        .conn()
        .query_row(
            "SELECT title, structure_json, content_checksum, updated_at FROM stories WHERE id = ?1",
            rusqlite::params![&created.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("row before");

    let destination = storage.path().join("immuable.rustory");
    export_story(ExportStoryInput {
        detail,
        destination_path: destination,
    })
    .expect("export");

    let after: (String, String, String, String) = handle
        .conn()
        .query_row(
            "SELECT title, structure_json, content_checksum, updated_at FROM stories WHERE id = ?1",
            rusqlite::params![&created.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("row after");

    assert_eq!(
        before, after,
        "canonical row must remain invariant across an export"
    );
}

#[test]
fn export_twice_to_same_destination_overwrites_without_corruption() {
    let storage = TempDir::new().expect("tempdir");
    let db_path = resolve_db_path(storage.path());
    let mut handle = db::open_at(&db_path).expect("open db");
    db::run_migrations(&mut handle).expect("migrate");

    let created = create_story(
        &mut handle,
        CreateStoryInput {
            title: "Idempotent".into(),
        },
    )
    .expect("create");
    let detail = get_story_detail(&handle, &created.id)
        .expect("read detail")
        .expect("detail present");

    let destination = storage.path().join("stable.rustory");
    export_story(ExportStoryInput {
        detail: detail.clone(),
        destination_path: destination.clone(),
    })
    .expect("first export");

    // Put something different at the destination to prove the second
    // export overwrites cleanly without leaving stale bytes behind.
    fs::write(&destination, b"not-a-rustory-artifact").expect("mutate");

    export_story(ExportStoryInput {
        detail,
        destination_path: destination.clone(),
    })
    .expect("second export");

    let bytes = fs::read(&destination).expect("read");
    let parsed: RustoryArtifactV1 =
        serde_json::from_slice(&bytes).expect("second export must produce valid artifact");
    assert_eq!(parsed.story.title, "Idempotent");
}
