//! FR30 signature journeys — the structured-folder creation, end to end on
//! a real (in-memory) database + a real temp folder.
//!
//! 1. A CLEAN folder (manifest + usable media) analyzes `Clean`, accepts
//!    into a canonical story indistinguishable in nature from an
//!    interactive creation: recognized provenance, wired assets, FULL edit
//!    scope, complete editor detail, media served by the node-media read.
//! 2. A folder with a MISSING media + a broken option link analyzes
//!    `partial` (the first real `Missing` emitter), accepts with the
//!    durable marker, and REPAIRING the link in the editor settles the
//!    review (`resolved`, findings kept) — the repair-in-editor journey
//!    replayed on this format; empty media slots never hold it back.
//! 3. A BLOCKING folder (no manifest / future formatVersion / duplicate
//!    ids) refuses at accept and leaves the library STRICTLY intact.
//!
//! Orthogonality: a `structured-folder` story stays `NotTransferable` at
//! the write-plan gate (no pack files) — verdict and gate never mix.

use std::path::{Path, PathBuf};

use rustory_lib::application::import_export::{
    accept_structured_creation, analyze_structured_folder,
};
use rustory_lib::application::story::structure::{add_node, set_option_link};
use rustory_lib::application::story::{get_story_detail, node, scope};
use rustory_lib::domain::import::{ImportState, RecognitionQuality};
use rustory_lib::domain::story::{validate_canonical, CanonicalStoryFacts};
use rustory_lib::domain::transfer::{
    build_write_plan, PreparedArtifact, PreparedArtifactKind, TransferArtifactDescriptor,
    TransferFailureCause,
};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::filesystem::resolve_node_media_dir;
use tempfile::TempDir;

// A REAL, decodable 8×6 RGB PNG: the media store transcodes images to a
// display PNG, so a magic-only header is refused. Kept as literal bytes to
// avoid pulling the image crate into the integration test crate.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 6, 8, 2, 0,
    0, 0, 113, 103, 72, 172, 0, 0, 0, 17, 73, 68, 65, 84, 120, 218, 99, 16, 209, 176, 193, 138, 24,
    6, 82, 2, 0, 144, 240, 22, 129, 145, 56, 15, 101, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const MP3: &[u8] = b"ID3\x03\x00\x00\x00rustory-audio";

fn fresh_db() -> DbHandle {
    let mut handle = db::open_in_memory().expect("open");
    db::run_migrations(&mut handle).expect("migrate");
    handle
}

fn write_manifest(folder: &Path, manifest: &str) {
    std::fs::write(folder.join("histoire.json"), manifest).expect("manifest");
}

fn clean_folder(tmp: &TempDir) -> PathBuf {
    let folder = tmp.path().join("le-voyage-de-nour");
    std::fs::create_dir(&folder).expect("mkdir");
    write_manifest(
        &folder,
        r#"{
            "formatVersion": 1,
            "title": "Le voyage de Nour",
            "startNodeId": "debut",
            "nodes": [
                { "id": "debut", "text": "Il était une fois…", "label": "Départ",
                  "image": "couverture.png", "audio": "intro.mp3",
                  "options": [ { "label": "Aller à la mer", "target": "mer" } ] },
                { "id": "mer", "text": "La mer scintille.", "options": [] }
            ]
        }"#,
    );
    std::fs::write(folder.join("couverture.png"), PNG).expect("png");
    std::fs::write(folder.join("intro.mp3"), MP3).expect("mp3");
    folder
}

fn count(db: &DbHandle, table: &str) -> u32 {
    db.conn()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .expect("count")
}

fn provenance_row(db: &DbHandle, story_id: &str) -> (String, String, Option<String>) {
    db.conn()
        .query_row(
            "SELECT source_format, import_state, findings_summary \
             FROM story_local_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("provenance row")
}

fn store_file_count(app_data_dir: &Path) -> usize {
    match std::fs::read_dir(resolve_node_media_dir(app_data_dir)) {
        Ok(entries) => entries.flatten().filter(|e| e.path().is_file()).count(),
        Err(_) => 0,
    }
}

#[test]
fn journey_1_a_clean_folder_creates_a_fully_editable_canonical_story() {
    let tmp = TempDir::new().expect("tmp");
    let app_data = TempDir::new().expect("app data");
    let mut db = fresh_db();
    let folder = clean_folder(&tmp);

    // Phase 1 — the analysis is clean and mutates NOTHING.
    let outcome = analyze_structured_folder(&folder).expect("analyze");
    assert_eq!(outcome.analysis.quality, RecognitionQuality::Clean);
    assert_eq!(outcome.analysis.state, ImportState::Recognized);
    assert_eq!(count(&db, "stories"), 0);
    assert_eq!(store_file_count(app_data.path()), 0);

    // Phase 2 — accept: one canonical story + provenance + wired assets.
    let card = accept_structured_creation(&mut db, app_data.path(), &folder).expect("accept");
    assert_eq!(card.title, "Le voyage de Nour");

    let (format, state, summary) = provenance_row(&db, &card.id);
    assert_eq!(format, "structured-folder");
    assert_eq!(state, "recognized");
    assert!(summary.is_none(), "a clean creation has no durable report");

    // The persisted canonical facts pass the SAME validation a transfer
    // runs — the folder flow and the editor agree on "canonically valid".
    let (title, schema_version, structure_json, checksum): (String, u32, String, String) = db
        .conn()
        .query_row(
            "SELECT title, schema_version, structure_json, content_checksum \
             FROM stories WHERE id = ?1",
            rusqlite::params![card.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("story row");
    let blockers = validate_canonical(&CanonicalStoryFacts {
        title,
        schema_version,
        structure_json,
        content_checksum: checksum,
    });
    assert!(
        blockers.is_empty(),
        "the created story is canonically sound"
    );

    // FULL edit scope BY CONSTRUCTION (the derivation reads story_imports
    // only) — the story opens exactly like a native one.
    assert_eq!(
        scope::story_edit_scope(db.conn(), &card.id),
        scope::StoryEditScope::Full
    );
    let detail = get_story_detail(&db, app_data.path(), &card.id, None)
        .expect("read detail")
        .expect("present");
    assert!(detail.editable);
    assert_eq!(detail.edit_scope, "full");
    assert_eq!(detail.import_state.as_deref(), Some("recognized"));
    let structure = detail.structure.expect("full editor structure");
    assert_eq!(structure.start_node_id, "debut");
    assert_eq!(structure.nodes.len(), 2);

    // The wired media slots are READY and their bytes are served by the
    // node-media read (content-addressed store).
    let start = detail
        .node
        .expect("current node projected for a full-scope story");
    let image = start.image.expect("image slot wired");
    assert_eq!(image.state, "ready");
    let audio = start.audio.expect("audio slot wired");
    assert_eq!(audio.state, "ready");
    let media_dir = resolve_node_media_dir(app_data.path());
    let (image_bytes, image_mime) =
        node::read_node_media(&db, &media_dir, &card.id, &image.asset_id).expect("read image");
    // The stored image is the TRANSCODED display PNG (not the source bytes):
    // it is a PNG (magic) and non-empty.
    assert_eq!(image_mime, "image/png");
    assert!(
        image_bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) && !image_bytes.is_empty(),
        "stored image is a PNG"
    );
    let (audio_bytes, audio_mime) =
        node::read_node_media(&db, &media_dir, &card.id, &audio.asset_id).expect("read audio");
    assert_eq!(audio_bytes, MP3);
    assert_eq!(audio_mime, "audio/mpeg");
}

#[test]
fn journey_2_a_partial_folder_repairs_in_the_editor_and_settles_its_review() {
    let tmp = TempDir::new().expect("tmp");
    let app_data = TempDir::new().expect("app data");
    let mut db = fresh_db();
    let folder = tmp.path().join("a-reparer");
    std::fs::create_dir(&folder).expect("mkdir");
    // A referenced media ABSENT from the folder (→ Missing → `partial`,
    // the first real emitter) + an option pointing at a ghost node
    // (Fixable → preserved broken, repairable in the editor).
    write_manifest(
        &folder,
        r#"{
            "formatVersion": 1,
            "title": "À réparer",
            "nodes": [
                { "id": "n1", "text": "Départ",
                  "image": "absente.png",
                  "options": [ { "label": "Perdu", "target": "ghost" } ] }
            ]
        }"#,
    );

    let outcome = analyze_structured_folder(&folder).expect("analyze");
    assert_eq!(outcome.analysis.state, ImportState::Partial);

    let card = accept_structured_creation(&mut db, app_data.path(), &folder).expect("accept");
    let (_, state, summary) = provenance_row(&db, &card.id);
    assert_eq!(state, "partial", "the durable partial marker landed");
    assert!(summary.is_some(), "the durable report backs the marker");
    assert_eq!(count(&db, "assets"), 0, "the discarded media wired nothing");

    // The editor opens FULLY editable with the flagged link and the empty
    // image slot (the durable, intended state — repairable with the
    // node-media controls).
    let detail = get_story_detail(&db, app_data.path(), &card.id, None)
        .expect("read")
        .expect("present");
    assert_eq!(detail.edit_scope, "full");
    assert_eq!(detail.import_state.as_deref(), Some("partial"));
    let structure = detail.structure.expect("graph projected");
    assert_eq!(structure.nodes[0].options[0].state, "broken");
    let start = detail.node.expect("current node");
    assert!(
        start.image.is_none(),
        "the discarded media left the slot empty"
    );

    // Repair the link in the editor (structure spine): add a destination,
    // then re-link. The empty media slot NEVER holds the resolution back
    // (media are never part of the oracle — inherited resolution rule).
    let added = add_node(&mut db, &card.id, None).expect("add destination");
    assert_eq!(
        added.import_state.as_deref(),
        Some("partial"),
        "a write that leaves the link broken keeps the review pending"
    );
    let target_id = added
        .structure
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .find(|id| *id != "n1")
        .expect("new node id")
        .to_string();
    let repaired =
        set_option_link(&mut db, &card.id, "n1", 0, Some(&target_id)).expect("repair link");
    assert_eq!(
        repaired.import_state.as_deref(),
        Some("resolved"),
        "the acknowledgement carries the settled review"
    );

    // The findings trace is KEPT in base; the durable state is settled.
    let (_, state, summary) = provenance_row(&db, &card.id);
    assert_eq!(state, "resolved");
    assert!(
        summary.is_some(),
        "findings are the review's trace, never erased"
    );
}

#[test]
fn journey_3_a_blocking_folder_leaves_the_library_strictly_intact() {
    let tmp = TempDir::new().expect("tmp");
    let app_data = TempDir::new().expect("app data");
    let mut db = fresh_db();

    // Variant 1 — no manifest at all.
    let empty = tmp.path().join("vide");
    std::fs::create_dir(&empty).expect("mkdir");
    // Variant 2 — a future format version.
    let future = tmp.path().join("futur");
    std::fs::create_dir(&future).expect("mkdir");
    write_manifest(
        &future,
        r#"{ "formatVersion": 2, "title": "Futur", "nodes": [ { "id": "n1" } ] }"#,
    );
    // Variant 3 — duplicate node ids (the UX `dupliqué`).
    let duplicated = tmp.path().join("doublon");
    std::fs::create_dir(&duplicated).expect("mkdir");
    write_manifest(
        &duplicated,
        r#"{ "formatVersion": 1, "title": "Doublon", "nodes": [ { "id": "n1" }, { "id": "n1" } ] }"#,
    );

    for folder in [&empty, &future, &duplicated] {
        let outcome = analyze_structured_folder(folder).expect("analyze");
        assert_eq!(
            outcome.analysis.state,
            ImportState::Blocked,
            "{folder:?} must analyze blocked"
        );
        accept_structured_creation(&mut db, app_data.path(), folder)
            .expect_err("a blocked folder must refuse the accept");
    }

    // STRICTLY intact: zero row anywhere, zero promoted file.
    assert_eq!(count(&db, "stories"), 0);
    assert_eq!(count(&db, "story_local_imports"), 0);
    assert_eq!(count(&db, "assets"), 0);
    assert_eq!(store_file_count(app_data.path()), 0);
}

#[test]
fn a_structured_folder_story_stays_not_transferable_at_the_write_plan() {
    // Orthogonality: the creation writes NO pack material (`story_imports`
    // stays empty), so a native-assembly descriptor is refused at the gate
    // exactly like a native / `.rustory` story — `transfer.rs` unchanged.
    let tmp = TempDir::new().expect("tmp");
    let app_data = TempDir::new().expect("app data");
    let mut db = fresh_db();
    let folder = clean_folder(&tmp);
    let card = accept_structured_creation(&mut db, app_data.path(), &folder).expect("accept");

    let pack_rows: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM story_imports WHERE story_id = ?1",
            rusqlite::params![card.id],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(pack_rows, 0, "a folder creation never writes pack material");

    let descriptor = TransferArtifactDescriptor {
        story_id: card.id.clone(),
        target_cohort: "v3".into(),
        pipeline_version: 1,
        artifacts: vec![PreparedArtifact {
            kind: PreparedArtifactKind::CanonicalStructure,
            relative_ref: "canonical/structure.json".into(),
            byte_len: 10,
            checksum: "a".repeat(64),
        }],
        aggregate_checksum: "b".repeat(64),
    };
    assert_eq!(
        build_write_plan(&descriptor, "AAAAAAAA").expect_err("refused"),
        TransferFailureCause::NotTransferable
    );
}
