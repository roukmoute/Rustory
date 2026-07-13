//! FR21 signature journeys — the declared edit scope + the write-path
//! review resolution, end to end on a real (in-memory) database.
//!
//! 1. A `.rustory` artifact with a broken option link imports as
//!    `needs_review`, opens FULLY editable, and REPAIRING the link inside
//!    the editor settles the review (`resolved`) — the findings trace stays
//!    in base, the acknowledgement carries the settled state.
//! 2. A device-pack story is `titleOnly`: content and structure writes are
//!    refused with the revised pack messages, the TITLE stays editable and
//!    the pack provenance row is untouched.
//! 3. Orthogonality: the corrected `.rustory` story stays NotTransferable
//!    at the write-plan gate (no pack files), while a locally renamed pack
//!    keeps its transferable material — verdict and gate never mix.

use rustory_lib::application::import_export::{accept_import, analyze_artifact};
use rustory_lib::application::story::structure::{add_node, set_option_link};
use rustory_lib::application::story::{
    create_story, get_story_detail, node, update_story, CreateStoryInput, UpdateStoryInput,
};
use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::domain::story::content_checksum;
use rustory_lib::domain::transfer::{
    build_write_plan, PreparedArtifact, PreparedArtifactKind, TransferArtifactDescriptor,
    TransferFailureCause,
};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::ipc::dto::{AcceptArtifactImportInputDto, ImportableContentDto};

const PACK_REFUSAL_MESSAGE: &str =
    "Le contenu de cette histoire est porté par le pack copié depuis l'appareil et ne peut pas être modifié ici.";
const PACK_REFUSAL_ACTION: &str =
    "Tu peux modifier le titre depuis l'éditeur ; le contenu du pack reste celui de l'appareil.";

fn fresh_db() -> DbHandle {
    let mut db = db::open_in_memory().expect("open");
    db::run_migrations(&mut db).expect("migrate");
    db
}

/// Import a `.rustory` artifact whose start node carries an option pointing
/// at a ghost node — the Fixable `BrokenOptionLink` that makes the artifact
/// land as an importable `needs_review`.
fn import_broken_link_artifact(db: &mut DbHandle) -> String {
    let broken = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[{\"label\":\"Perdu\",\"target\":\"ghost\"}]}]}";
    let artifact = serde_json::json!({
        "rustoryArtifact": {
            "formatVersion": 1,
            "exportedAt": "2026-07-06T10:00:00.000Z",
            "exportedBy": "rustory/0.1.0",
        },
        "story": {
            "schemaVersion": 3,
            "title": "Lien à corriger",
            "structureJson": broken,
            "contentChecksum": content_checksum(broken),
            "createdAt": "2026-06-20T10:00:00.000Z",
            "updatedAt": "2026-06-24T14:15:00.000Z",
        },
    });
    let bytes = serde_json::to_vec(&artifact).expect("artifact bytes");

    let analysis = analyze_artifact(&bytes, "casse.rustory".into());
    let importable = analysis
        .analysis
        .importable
        .clone()
        .expect("a broken option link stays importable (needs_review)");
    let input = AcceptArtifactImportInputDto {
        content: ImportableContentDto {
            title: importable.title,
            structure_json: importable.structure_json,
            content_checksum: importable.content_checksum,
            created_at: importable.created_at,
            updated_at: importable.updated_at,
        },
        source_name: analysis.source_name,
        artifact_checksum: analysis.artifact_checksum,
    };
    accept_import(db, &input)
        .expect("accept the needs_review import")
        .id
}

fn provenance_row(db: &DbHandle, story_id: &str) -> (String, Option<String>) {
    db.conn()
        .query_row(
            "SELECT import_state, findings_summary FROM story_local_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("provenance row")
}

fn seed_device_pack(db: &mut DbHandle, title: &str) -> String {
    let id = create_story(
        db,
        CreateStoryInput {
            title: title.into(),
        },
    )
    .expect("create")
    .id;
    db.conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
             VALUES (?1, '019739b2-0000-7000-8000-00000000abcd', '0123456789abcdef0123456789abcdef', '2026-07-06T00:00:00.000Z', 5, 18, ?2, 'lunii')",
            rusqlite::params![id, "ab".repeat(32)],
        )
        .expect("insert pack provenance");
    id
}

#[test]
fn a_broken_link_rustory_import_is_repaired_in_the_editor_and_settles_its_review() {
    let mut db = fresh_db();
    let tmp = std::env::temp_dir();
    let story_id = import_broken_link_artifact(&mut db);

    // The imported story lands with its pending review.
    let (state, summary) = provenance_row(&db, &story_id);
    assert_eq!(state, "needs_review");
    assert!(summary.is_some(), "the findings report is durable");

    // FR21: the detail opens FULLY editable with the honest review state and
    // the flagged link visible.
    let detail = get_story_detail(&db, &tmp, &story_id, None)
        .expect("read")
        .expect("present");
    assert!(detail.editable);
    assert_eq!(detail.edit_scope, "full");
    assert_eq!(detail.import_state.as_deref(), Some("needsReview"));
    let structure = detail
        .structure
        .expect("a Fixable link keeps the graph projected");
    assert_eq!(structure.nodes[0].options[0].state, "broken");

    // Repair: create a real destination, then re-link the flagged option.
    // Adding the node alone leaves the link broken — the review must NOT
    // settle yet (the oracle is the COMPLETE blocker list, any severity).
    let added = add_node(&mut db, &story_id, None).expect("add destination node");
    assert_eq!(
        added.import_state.as_deref(),
        Some("needsReview"),
        "a write that leaves the link broken keeps the review pending"
    );
    let target_id = added
        .structure
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .find(|id| *id != "n1")
        .expect("the new node id")
        .to_string();

    let repaired =
        set_option_link(&mut db, &story_id, "n1", 0, Some(&target_id)).expect("repair the link");
    assert_eq!(
        repaired.import_state.as_deref(),
        Some("resolved"),
        "the acknowledgement carries the settled review"
    );
    assert_eq!(repaired.structure.nodes[0].options[0].state, "linked");

    // Re-read: the detail projects the settled state; the findings trace is
    // STILL in base (never erased — the card side goes quiet, which the
    // library producer locks separately).
    let detail = get_story_detail(&db, &tmp, &story_id, None)
        .expect("read")
        .expect("present");
    assert_eq!(detail.import_state.as_deref(), Some("resolved"));
    let (state, summary) = provenance_row(&db, &story_id);
    assert_eq!(state, "resolved");
    assert!(summary.is_some(), "the findings trace is KEPT in base");
}

#[test]
fn a_device_pack_stays_title_only_with_the_revised_refusals() {
    let mut db = fresh_db();
    let tmp = std::env::temp_dir();
    let story_id = seed_device_pack(&mut db, "Pack de l'appareil");

    // The detail declares the titleOnly scope and never projects an import
    // state (a pack has no `.rustory` review).
    let detail = get_story_detail(&db, &tmp, &story_id, None)
        .expect("read")
        .expect("present");
    assert!(!detail.editable);
    assert_eq!(detail.edit_scope, "titleOnly");
    assert_eq!(detail.import_state, None);

    // A node content write is refused with the REVISED pack message.
    let err = node::save_node_content(
        &mut db,
        &tmp,
        node::SaveNodeContentInput {
            story_id: story_id.clone(),
            node_id: "n1".into(),
            text: "refusé".into(),
            label: String::new(),
        },
    )
    .expect_err("content write refused");
    assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
    assert_eq!(err.message, PACK_REFUSAL_MESSAGE);
    assert_eq!(err.user_action.as_deref(), Some(PACK_REFUSAL_ACTION));
    assert_eq!(err.details.unwrap()["source"], "node_not_editable");

    // A structural write is refused the same way (dedicated source marker).
    let err = add_node(&mut db, &story_id, None).expect_err("structure write refused");
    assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
    assert_eq!(err.message, PACK_REFUSAL_MESSAGE);
    assert_eq!(err.details.unwrap()["source"], "structure_not_editable");

    // The TITLE stays editable — a local Rustory metadata (the established
    // local rename behavior, contractualized). The ACK carries an explicit
    // null import state.
    let renamed = update_story(
        &mut db,
        UpdateStoryInput {
            id: story_id.clone(),
            title: "Pack renommé".into(),
        },
    )
    .expect("the title write goes through");
    assert_eq!(renamed.title, "Pack renommé");
    assert_eq!(renamed.import_state, None);

    // The pack provenance row is INTACT (uuid + checksum untouched).
    let (count, pack_uuid): (u32, String) = db
        .conn()
        .query_row(
            "SELECT COUNT(*), MAX(pack_uuid) FROM story_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("pack row");
    assert_eq!(count, 1);
    assert_eq!(pack_uuid, "019739b2-0000-7000-8000-00000000abcd");
}

#[test]
fn review_resolution_never_changes_the_write_plan_gate() {
    let mut db = fresh_db();
    let tmp = std::env::temp_dir();

    // Journey 1 again, condensed: import + repair → resolved.
    let resolved_id = import_broken_link_artifact(&mut db);
    let added = add_node(&mut db, &resolved_id, None).expect("add");
    let target_id = added
        .structure
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .find(|id| *id != "n1")
        .expect("new node")
        .to_string();
    set_option_link(&mut db, &resolved_id, "n1", 0, Some(&target_id)).expect("repair");
    let detail = get_story_detail(&db, &tmp, &resolved_id, None)
        .expect("read")
        .expect("present");
    assert_eq!(detail.import_state.as_deref(), Some("resolved"));

    // The corrected story STILL has no device-pack provenance: the transfer
    // facts read `story_imports` alone, so its assembly stays Native — no
    // pack file ever reaches the write plan.
    let pack_rows: u32 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM story_imports WHERE story_id = ?1",
            rusqlite::params![resolved_id],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(
        pack_rows, 0,
        "resolving a review never creates pack material"
    );

    // A native-assembly descriptor (canonical structure only, exactly what a
    // `.rustory` story would prepare) is refused at the gate — the verdict
    // (`resolved`) and the gate (`NotTransferable`) stay orthogonal.
    let native_descriptor = TransferArtifactDescriptor {
        story_id: resolved_id.clone(),
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
        build_write_plan(&native_descriptor, "AAAAAAAA").expect_err("refused"),
        TransferFailureCause::NotTransferable
    );

    // A locally RENAMED device pack keeps its pack material transferable:
    // the rename touched `stories.title` only, the pack row survives (locked
    // by the journey above), and a pack-file descriptor passes the gate.
    let pack_id = seed_device_pack(&mut db, "Pack de l'appareil");
    update_story(
        &mut db,
        UpdateStoryInput {
            id: pack_id.clone(),
            title: "Pack renommé".into(),
        },
    )
    .expect("rename");
    let pack_descriptor = TransferArtifactDescriptor {
        story_id: pack_id,
        target_cohort: "v3".into(),
        pipeline_version: 1,
        artifacts: vec![PreparedArtifact {
            kind: PreparedArtifactKind::PackFile,
            relative_ref: "ni".into(),
            byte_len: 18,
            checksum: "c".repeat(64),
        }],
        aggregate_checksum: "d".repeat(64),
    };
    let plan = build_write_plan(&pack_descriptor, "AAAAAAAA").expect("transferable");
    assert_eq!(plan.files.len(), 1);
}
