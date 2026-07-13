//! End-to-end proof of the content-source CONVERGENCE contract
//! (`ui-states.md#Content Source Activation Contract`): a story created
//! from the enabled RSS source is an ORDINARY canonical story that enters
//! the EXISTING `validate → prepare → transfer → verify` pipeline through
//! the same application facades as a native twin — no special path, no
//! duplicated command, and NO discrimination: at every stage the RSS-born
//! story and a byte-identical native twin receive the SAME verdicts. In
//! the current distribution a locally-born story carries no device-format
//! pack, so the shared transfer gate refuses BOTH honestly
//! (`NotTransferable`, the device left untouched) — while an
//! imported-pack witness driven through the SAME facade reaches the
//! VERIFIED terminal, proving the shared pipeline runs to verification
//! whenever a pack exists. The policy-refusal journey proves the other
//! half of the governance: ZERO network dispatch and ZERO mutation.
//!
//! Real I/O throughout: the REAL system scanner + filesystem assembler +
//! device writer against temp writable mounts, a real on-disk SQLite
//! store — only the feed source is scripted (no network in tests).

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rustory_lib::application::device::import::{import_device_story, ImportDeviceStoryRequest};
use rustory_lib::application::device::preflight::{
    read_story_validation, StoryValidationOutcome, Verdict,
};
use rustory_lib::application::import_export::{
    accept_rss_story_creation, preview_rss_source, RssCreationOutcome,
};
use rustory_lib::application::transfer::{
    prepare_story, read_preparation_state, transfer_story, PreparationEventEmitter,
    PreparationOutcome, PreparationStateView, TransferOutcome,
};
use rustory_lib::domain::import::{
    official_content_sources, parse_rss, rss_item_fingerprint, ContentSourceActivation,
    ContentSourceKind, ContentSourceLine, RssItemRef,
};
use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::domain::transfer::{PreparationPhase, TransferCompleteness, TransferFailureCause};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{
    SystemDeviceLibraryReader, SystemDevicePackReader, SystemDevicePackWriter, SystemDeviceScanner,
};
use rustory_lib::infrastructure::filesystem::SystemTransferArtifactSource;
use tempfile::TempDir;

use crate::rss_creation::ScriptedRssSource;
use crate::story_transfer::build_mount_with_pack;

const FEED_URL: &str = "https://exemple.fr/flux.xml";

fn budget() -> Duration {
    Duration::from_secs(30)
}

fn uuid(tail: [u8; 4]) -> [u8; 16] {
    let mut b = [0xAB; 16];
    b[12..16].copy_from_slice(&tail);
    b
}

/// Silent emitter — these journeys assert on outcomes and device bytes,
/// not on the event stream (covered by the transfer/preparation suites).
struct SilentEmitter;

impl PreparationEventEmitter for SilentEmitter {
    fn progress(&self, _phase: PreparationPhase, _progress: Option<f32>, _sequence: u64) {}
    fn completed(&self, _sequence: u64) {}
    fn failed(&self, _message: &str, _user_action: &str, _sequence: u64) {}
}

fn open_db(tmp: &TempDir) -> DbHandle {
    let path = tmp.path().join("rustory.sqlite");
    let mut handle = db::open_at(&path).expect("open");
    db::run_migrations(&mut handle).expect("migrate");
    handle
}

fn feed_xml(items: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\"><channel><title>Mon flux</title>{items}</channel></rss>"
    )
}

fn nominal_feed() -> String {
    feed_xml(
        "<item><title>Episode convergence</title><description>Texte ingéré depuis le flux.</description><guid>g-c</guid></item>",
    )
}

fn fingerprint_in(feed: &str, guid: &str) -> String {
    let analysis = parse_rss(feed.as_bytes());
    let item = analysis
        .items
        .iter()
        .find(|item| item.guid.as_deref() == Some(guid))
        .expect("previewed item");
    rss_item_fingerprint(item)
}

/// Create ONE story through the REAL RSS acceptance (official matrix,
/// scripted feed) and return its id.
fn create_rss_story(db: &Mutex<DbHandle>) -> String {
    let source = ScriptedRssSource::new();
    source.enqueue_body(nominal_feed());
    let fingerprint = fingerprint_in(&nominal_feed(), "g-c");
    let outcome = {
        let mut guard = db.lock().expect("db lock");
        accept_rss_story_creation(
            &mut guard,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-c".into()),
            &fingerprint,
            budget(),
        )
        .expect("accept")
    };
    let RssCreationOutcome::Created { story } = outcome else {
        panic!("expected a creation");
    };
    story.id
}

/// Insert a NATIVE twin carrying the exact canonical bytes of `story_id`
/// (the title-path birth, minus the dialog): the convergence proof then
/// compares the two stories stage by stage.
fn insert_native_twin(db: &Mutex<DbHandle>, rss_story_id: &str, twin_id: &str) {
    let guard = db.lock().expect("db lock");
    let (structure_json, checksum): (String, String) = guard
        .conn()
        .query_row(
            "SELECT structure_json, content_checksum FROM stories WHERE id = ?1",
            rusqlite::params![rss_story_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("rss story row");
    guard
        .conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, 'Jumelle native', 3, ?2, ?3, '2026-07-13T00:00:00.000Z', '2026-07-13T00:00:00.000Z')",
            rusqlite::params![twin_id, structure_json, checksum],
        )
        .expect("insert twin");
}

/// Recursive snapshot of every file under `root` (relative path → bytes):
/// the proof that a refused transfer wrote NOTHING on the device.
fn snapshot_mount(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("under root")
                    .to_string_lossy()
                    .to_string();
                out.push((rel, std::fs::read(&path).expect("read file")));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Acquire ONE pack-carrying story through the REAL device import (a
/// separate V1 source mount): the witness that the shared transfer
/// facade runs to the verified terminal when a pack exists. Returns the
/// local story id and the source-mount guard (kept alive).
fn import_pack_witness(db: &Mutex<DbHandle>, app_data: &Path) -> (String, TempDir) {
    let (guard, root, identifier, canonical) =
        build_mount_with_pack(3, uuid([0xFA, 0xC5, 0x56, 0x2D]));
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let imported = import_device_story(
        db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemDevicePackReader,
        app_data,
        &ImportDeviceStoryRequest {
            device_identifier: identifier,
            pack_uuid: canonical,
        },
        budget(),
    )
    .expect("witness import");
    (imported.story.id, guard)
}

/// The `(verdict, blocker_count)` face of a validation outcome.
fn validation_face(outcome: StoryValidationOutcome) -> (Verdict, usize) {
    match outcome {
        StoryValidationOutcome::Ready {
            verdict, blockers, ..
        } => (verdict, blockers.len()),
        other => panic!("expected Ready, got {other:?}"),
    }
}

/// The convergence proof. An RSS-born story travels the EXISTING
/// pipeline through the same facades as a byte-identical native twin and
/// receives the SAME verdicts at every stage: validation (presumed
/// transferable), preparation (prepared, `transferable: false` — the
/// assembled artifacts are the native baseline, no device-format pack),
/// and the transfer gate's honest refusal (`NotTransferable`, device
/// untouched). An imported-pack witness then reaches the VERIFIED
/// terminal through the SAME transfer facade on the SAME device — the
/// refusal of the two locally-born stories is a pack policy, never a
/// dead pipeline. Equality at every stage IS the "no special path"
/// claim made falsifiable: any source-specific branch in the pipeline
/// would break this test.
#[test]
fn an_rss_created_story_converges_through_the_existing_pipeline_like_a_native_twin() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));

    // Stage 0 — the REAL creation from the enabled source (official matrix).
    let rss_story_id = create_rss_story(&db);
    insert_native_twin(&db, &rss_story_id, "twin-native");

    // One writable V1 target mount, shared by every stage — the SAME
    // fixture as the transfer journeys (`story_transfer::build_mount_with_pack`).
    let (_guard, root, device_identifier, _) =
        build_mount_with_pack(3, uuid([0x11, 0x22, 0x33, 0x44]));
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root.clone()]);

    // Stage 1 — VALIDATION through the read_story_validation facade.
    let rss_validation = read_story_validation(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &rss_story_id,
        &device_identifier,
        budget(),
    )
    .expect("rss validation");
    let twin_validation = read_story_validation(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "twin-native",
        &device_identifier,
        budget(),
    )
    .expect("twin validation");
    let rss_face = validation_face(rss_validation);
    assert_eq!(
        rss_face,
        (Verdict::PresumedTransferable, 0),
        "an ingested story validates like any healthy canonical story"
    );
    assert_eq!(rss_face, validation_face(twin_validation), "same verdict");

    // Stage 2 — PREPARATION through the prepare_story facade (real
    // assembler): both stories PREPARE (the native baseline artifacts).
    for story_id in [rss_story_id.as_str(), "twin-native"] {
        let outcome = prepare_story(
            &db,
            &scanner,
            &SystemDeviceLibraryReader,
            &SystemTransferArtifactSource,
            app_data.path(),
            story_id,
            &device_identifier,
            budget(),
            budget(),
            &SilentEmitter,
        );
        assert!(
            matches!(outcome, PreparationOutcome::Prepared { .. }),
            "{story_id}: {outcome:?}"
        );
        // The authoritative re-read declares BOTH prepared and NOT
        // transferable — the send gate disables `Envoyer` upstream for a
        // pack-less story, RSS-born or native alike.
        let view = read_preparation_state(
            &db,
            &scanner,
            &SystemDeviceLibraryReader,
            &SystemTransferArtifactSource,
            app_data.path(),
            story_id,
            budget(),
            budget(),
        )
        .expect("read preparation state");
        match view {
            PreparationStateView::Prepared { transferable, .. } => {
                assert!(!transferable, "{story_id}: no device-format pack")
            }
            other => panic!("{story_id}: expected Prepared, got {other:?}"),
        }
    }

    // Stage 3 — the TRANSFER gate through the transfer_story facade: the
    // SAME honest refusal for both (`NotTransferable`, device untouched —
    // the refusal fires BEFORE any mutation, so no verification pass has
    // anything to confirm). The device-byte snapshot proves nothing
    // changed; stage 4 below proves the same facade DOES run to the
    // verified terminal when a pack exists.
    let before = snapshot_mount(&root);
    for story_id in [rss_story_id.as_str(), "twin-native"] {
        let outcome = transfer_story(
            &db,
            &scanner,
            &SystemDeviceLibraryReader,
            &SystemTransferArtifactSource,
            &SystemDevicePackWriter,
            app_data.path(),
            story_id,
            &device_identifier,
            budget(),
            budget(),
            &SilentEmitter,
        );
        match outcome {
            TransferOutcome::Retryable {
                cause,
                completeness,
            } => {
                assert_eq!(cause, TransferFailureCause::NotTransferable, "{story_id}");
                assert_eq!(completeness, TransferCompleteness::Failed, "{story_id}");
            }
            other => panic!("{story_id}: expected the honest refusal, got {other:?}"),
        }
    }
    assert_eq!(
        before,
        snapshot_mount(&root),
        "a refused transfer writes ZERO byte on the device"
    );

    // Stage 4 — the VERIFIED witness: a story that DOES carry a
    // device-format pack (acquired through the real device import)
    // reaches the verified terminal through the SAME transfer facade on
    // the SAME target device. The shared pipeline runs all the way to
    // the verification pass whenever a pack exists — the stage-3 refusal
    // is a pack policy, not a dead end.
    let (witness_id, _src_guard) = import_pack_witness(&db, app_data.path());
    let outcome = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &witness_id,
        &device_identifier,
        budget(),
        budget(),
        &SilentEmitter,
    );
    assert!(
        matches!(outcome, TransferOutcome::Verified { .. }),
        "the shared facade reaches the verified terminal for a pack-carrying story: {outcome:?}"
    );

    // The canonical drafts survive the whole journey untouched.
    let count: i64 = db
        .lock()
        .expect("db lock")
        .conn()
        .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 3);
}

/// The policy refusal is enforced BEFORE any I/O: with a custom
/// distribution whose `rss` line is not enabled, BOTH facades refuse with
/// `CONTENT_SOURCE_UNAVAILABLE`, the scripted recorder proves ZERO
/// network dispatch, and nothing is created.
#[test]
fn a_policy_refusal_fetches_nothing_and_creates_nothing() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let db = Mutex::new(open_db(&db_tmp));
    let disabled = [ContentSourceLine {
        kind: ContentSourceKind::Rss,
        activation: ContentSourceActivation::NotActivated,
    }];

    let source = ScriptedRssSource::new();
    source.enqueue_body(nominal_feed());

    let preview_err =
        preview_rss_source(&disabled, &source, FEED_URL, budget()).expect_err("policy refusal");
    assert_eq!(preview_err.code, AppErrorCode::ContentSourceUnavailable);

    let accept_err = {
        let mut guard = db.lock().expect("db lock");
        accept_rss_story_creation(
            &mut guard,
            &disabled,
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-c".into()),
            &"0".repeat(64),
            budget(),
        )
        .expect_err("policy refusal")
    };
    assert_eq!(accept_err.code, AppErrorCode::ContentSourceUnavailable);
    let v = serde_json::to_value(&accept_err).expect("ser");
    assert_eq!(v["details"]["kind"], "rss");

    assert_eq!(source.request_count(), 0, "ZERO network dispatch");
    let count: i64 = db
        .lock()
        .expect("db lock")
        .conn()
        .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 0, "nothing is created on a policy refusal");
}
