//! End-to-end integration of the story transfer (device write): the REAL system
//! scanner + REAL filesystem assembler + REAL device writer against temp mounts,
//! composed with a real on-disk SQLite store. Proves the whole `transfer_story`
//! pipeline (authoritative re-scan → `WriteStory` gate BEFORE any mutation →
//! scoped DB lock → fresh re-assembly + integrity re-check → safe atomic write →
//! outcome + events) on actual I/O, including the strongest check: a pack
//! imported by story 2.x's REAL import, then transferred, lands byte-for-byte on
//! the device with its UUID added to `.pi`.
//!
//! The `#[cfg(test)]` fixtures + mocks are not visible to this separate test
//! crate, so the mounts + a capturing emitter are built inline.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustory_lib::application::device::import::{import_device_story, ImportDeviceStoryRequest};
use rustory_lib::application::transfer::{
    read_transfer_state, transfer_story, PreparationEventEmitter, TransferOutcome,
    TransferStateView,
};
use rustory_lib::domain::device::{format_pack_uuid, pack_short_id, parse_pack_index};
use rustory_lib::domain::shared::AppError;
use rustory_lib::domain::story::content_checksum;
use rustory_lib::domain::transfer::{
    append_pack_uuid, pack_uuid_bytes, PackWritePlan, PreparationPhase, TransferCompleteness,
    TransferFailureCause, VerifyVerdict,
};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{
    compute_device_identifier, DevicePackWriter, DeviceScanReport, DeviceScanner,
    SystemDeviceLibraryReader, SystemDevicePackReader, SystemDevicePackWriter, SystemDeviceScanner,
    WriteFailure, WriteProgress,
};
use rustory_lib::infrastructure::diagnostics::transfer as transfer_log;
use rustory_lib::infrastructure::filesystem::SystemTransferArtifactSource;
use tempfile::TempDir;

const HEALTHY_JSON: &str = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";

fn uuid(tail: [u8; 4]) -> [u8; 16] {
    let mut b = [0xAB; 16];
    b[12..16].copy_from_slice(&tail);
    b
}

fn budget() -> Duration {
    Duration::from_secs(30)
}

#[derive(Default)]
struct CapturingEmitter {
    events: Mutex<Vec<String>>,
    fractions: Mutex<Vec<f32>>,
}

impl PreparationEventEmitter for CapturingEmitter {
    fn progress(&self, phase: PreparationPhase, progress: Option<f32>, sequence: u64) {
        self.events
            .lock()
            .unwrap()
            .push(format!("progress:{}:{}", phase.wire_tag(), sequence));
        if let Some(fraction) = progress {
            self.fractions.lock().unwrap().push(fraction);
        }
    }
    fn completed(&self, sequence: u64) {
        self.events
            .lock()
            .unwrap()
            .push(format!("completed:{sequence}"));
    }
    fn failed(&self, _message: &str, _user_action: &str, sequence: u64) {
        self.events
            .lock()
            .unwrap()
            .push(format!("failed:{sequence}"));
    }
}

impl CapturingEmitter {
    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
    /// The non-null in-flight fractions actually emitted (honest progress).
    fn fractions(&self) -> Vec<f32> {
        self.fractions.lock().unwrap().clone()
    }
}

/// Write a complete plausible pack (declared subset) into `pack_dir`.
fn write_pack(pack_dir: &Path) {
    std::fs::create_dir_all(pack_dir).expect("mkdir pack");
    std::fs::write(pack_dir.join("ni"), vec![0x4E; 512]).expect("ni");
    std::fs::write(pack_dir.join("li"), vec![0x4C; 256]).expect("li");
    std::fs::write(pack_dir.join("ri"), vec![0x52; 128]).expect("ri");
    std::fs::write(pack_dir.join("si"), vec![0x53; 128]).expect("si");
    std::fs::write(pack_dir.join("nm"), vec![0x6E; 32]).expect("nm");
    let rf = pack_dir.join("rf").join("000");
    std::fs::create_dir_all(&rf).expect("rf/000");
    std::fs::write(rf.join("AAAAAAAA"), vec![0xAA; 2048]).expect("rf asset");
}

/// Mount with one pack present (markers + `.pi` + `.content/<SHORT_ID>`).
fn build_mount_with_pack(
    metadata_version: u8,
    pack_uuid: [u8; 16],
) -> (TempDir, PathBuf, String, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join(".md"), [metadata_version, 0xff, 0xaa]).expect(".md");
    std::fs::write(root.join(".pi"), pack_uuid).expect(".pi");
    let short_id = pack_short_id(&pack_uuid);
    write_pack(&root.join(".content").join(&short_id));
    let identifier = compute_device_identifier(&pack_uuid, None);
    (dir, root, identifier, format_pack_uuid(&pack_uuid))
}

fn open_db(tmp: &TempDir) -> DbHandle {
    let path = tmp.path().join("rustory.sqlite");
    let mut handle = db::open_at(&path).expect("open");
    db::run_migrations(&mut handle).expect("migrate");
    handle
}

fn insert_native_story(db: &Mutex<DbHandle>, id: &str) {
    db.lock()
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, 'Mon histoire', 2, ?2, ?3, '2026-06-22T00:00:00.000Z', '2026-06-22T00:00:00.000Z')",
            rusqlite::params![id, HEALTHY_JSON, content_checksum(HEALTHY_JSON)],
        )
        .expect("insert story");
}

/// Import one story from a fresh V1 (md v3) source mount via the REAL import
/// path. Returns the local story id, the canonical pack UUID, the raw bytes and
/// the source mount guard (kept alive).
fn import_one(
    db: &Mutex<DbHandle>,
    app_data: &Path,
    pack: [u8; 16],
) -> (String, [u8; 16], TempDir) {
    let (guard, root, identifier, canonical) = build_mount_with_pack(3, pack);
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
    .expect("import");
    (imported.story.id, pack, guard)
}

#[test]
fn transfers_an_imported_pack_to_a_writable_device() {
    // import 2.4 (real) → prepare/assemble 3.3 (real) → transfer 3.4 (real).
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));

    let pack = uuid([0xFA, 0xC5, 0x56, 0x2D]);
    let (story_id, pack_bytes, _src) = import_one(&db, app_data.path(), pack);

    // A SEPARATE writable V1 target device, holding a different existing pack.
    let (_target, target_root, target_id, _) =
        build_mount_with_pack(3, uuid([0x11, 0x22, 0x33, 0x44]));
    let target_scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root.clone()]);
    let emitter = CapturingEmitter::default();

    let outcome = transfer_story(
        &db,
        &target_scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &emitter,
    );
    match outcome {
        TransferOutcome::Verified { summary, .. } => assert!(
            summary.unchanged.starts_with("1 autre histoire"),
            "the target's pre-existing pack stays unchanged alongside the new one: {summary:?}"
        ),
        other => panic!("expected Verified, got {other:?}"),
    }
    // The job reports preflight, HONEST in-flight progress during the write, the
    // FINAL verify phase, then the verified terminal.
    let events = emitter.events();
    assert_eq!(
        events.first().map(String::as_str),
        Some("progress:preflight:1")
    );
    assert!(
        events.iter().any(|e| e.starts_with("progress:verify:")),
        "the verify phase is emitted before the terminal: {events:?}"
    );
    assert!(
        events
            .last()
            .map(|e| e.starts_with("completed:"))
            .unwrap_or(false),
        "ends with the verified terminal: {events:?}"
    );
    let fractions = emitter.fractions();
    assert!(
        !fractions.is_empty(),
        "the write reports progress: {events:?}"
    );
    assert!(
        // The honesty invariant is the HIGH bound (< 1.0 = PROGRESS_CEILING); the low
        // bound is >= 0.0 because a zero-byte first file would legitimately emit 0.0.
        fractions.iter().all(|f| *f >= 0.0 && *f < 1.0),
        "honest fraction: never 100% before the completed terminal: {fractions:?}"
    );

    // The pack content landed byte-for-byte under `.content/<SHORT_ID>`.
    let short = pack_short_id(&pack_bytes);
    let landed = target_root.join(".content").join(&short);
    assert!(landed.join("ni").is_file(), "ni must be written");
    assert_eq!(
        std::fs::read(landed.join("ni")).unwrap(),
        vec![0x4E; 512],
        "ni must be reproduced verbatim"
    );
    assert!(landed.join("rf").join("000").join("AAAAAAAA").is_file());

    // The UUID is now in the device index (alongside the pre-existing one).
    let pi = std::fs::read(target_root.join(".pi")).expect("read .pi");
    assert!(
        parse_pack_index(&pi).uuids.iter().any(|u| u == &pack_bytes),
        "the transferred UUID must be in .pi"
    );
}

#[test]
fn v3_blocks_the_transfer_before_any_device_mutation() {
    // AC2/FR34: a V3 (md v7) target is not write-authorized — the gate refuses
    // before any byte is written, and the device is left untouched.
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));

    let (story_id, pack_bytes, _src) =
        import_one(&db, app_data.path(), uuid([0xAA, 0xBB, 0xCC, 0xDD]));

    // V3 target with a pre-existing pack.
    let existing = uuid([0x09, 0x09, 0x09, 0x09]);
    let (_target, target_root, target_id, _) = build_mount_with_pack(7, existing);
    let pi_before = std::fs::read(target_root.join(".pi")).expect("read .pi");
    let target_scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root.clone()]);
    let emitter = CapturingEmitter::default();

    let outcome = transfer_story(
        &db,
        &target_scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &emitter,
    );
    assert_eq!(
        outcome,
        TransferOutcome::Retryable {
            cause: TransferFailureCause::WriteNotAuthorized,
            completeness: TransferCompleteness::Failed,
        }
    );
    // Never entered the transfer phase, and the device is byte-identical.
    assert_eq!(
        emitter.events(),
        vec!["progress:preflight:1".to_string(), "failed:2".to_string()]
    );
    assert!(
        !target_root
            .join(".content")
            .join(pack_short_id(&pack_bytes))
            .exists(),
        "the unauthorized pack must never be written"
    );
    assert_eq!(
        std::fs::read(target_root.join(".pi")).expect("read .pi"),
        pi_before,
        ".pi must be untouched on a blocked transfer"
    );
}

#[test]
fn a_native_story_is_not_transferable() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    insert_native_story(&db, "native"); // no story_imports row

    let (_target, target_root, target_id, _) =
        build_mount_with_pack(3, uuid([0x55, 0x55, 0x55, 0x55]));
    let pi_before = std::fs::read(target_root.join(".pi")).expect("read .pi");
    let target_scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root.clone()]);

    let outcome = transfer_story(
        &db,
        &target_scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        "native",
        &target_id,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert_eq!(
        outcome,
        TransferOutcome::Retryable {
            cause: TransferFailureCause::NotTransferable,
            completeness: TransferCompleteness::Failed,
        }
    );
    assert_eq!(
        std::fs::read(target_root.join(".pi")).expect("read .pi"),
        pi_before,
        ".pi must be untouched for a non-transferable story"
    );
}

#[test]
fn device_changed_when_no_supported_device_is_present() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let (story_id, _pack, _src) = import_one(&db, app_data.path(), uuid([1, 2, 3, 4]));

    let empty = tempfile::tempdir().expect("empty mount");
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![empty.path().to_path_buf()]);

    let outcome = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        "00000000000000000000000000000000",
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert_eq!(
        outcome,
        TransferOutcome::Retryable {
            cause: TransferFailureCause::DeviceChanged,
            completeness: TransferCompleteness::Failed,
        }
    );
}

#[test]
fn budget_exhaustion_writes_nothing_and_preserves_the_draft() {
    // A zero write budget aborts the write phase recoverably (the fresh
    // re-assembly runs out of budget first → NotPrepared), mutating neither the
    // device nor the canonical draft. The writer's own deadline path (staging
    // cleaned, `.pi` intact) is covered by its unit tests.
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let pack = uuid([0x77, 0x88, 0x99, 0xAA]);
    let (story_id, pack_bytes, _src) = import_one(&db, app_data.path(), pack);

    let canonical_before = read_story_row(&db, &story_id);
    let (_target, target_root, target_id, _) =
        build_mount_with_pack(3, uuid([0x01, 0x01, 0x01, 0x01]));
    let pi_before = std::fs::read(target_root.join(".pi")).expect("read .pi");
    let target_scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root.clone()]);

    let outcome = transfer_story(
        &db,
        &target_scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        Duration::ZERO, // write phase out of budget
        &CapturingEmitter::default(),
    );
    assert!(
        matches!(outcome, TransferOutcome::Retryable { .. }),
        "a budget exhaustion must be recoverable: {outcome:?}"
    );
    assert!(
        !target_root
            .join(".content")
            .join(pack_short_id(&pack_bytes))
            .exists(),
        "nothing must be written under a zero budget"
    );
    assert_eq!(
        std::fs::read(target_root.join(".pi")).expect("read .pi"),
        pi_before,
        ".pi must be untouched"
    );
    assert_eq!(
        read_story_row(&db, &story_id),
        canonical_before,
        "the canonical draft must be preserved"
    );
}

#[test]
fn the_canonical_story_is_unchanged_after_a_blocked_transfer() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let (story_id, _pack, _src) = import_one(&db, app_data.path(), uuid([2, 4, 6, 8]));

    let before = read_story_row(&db, &story_id);
    // V3 target → blocked.
    let (_target, target_root, target_id, _) = build_mount_with_pack(7, uuid([8, 6, 4, 2]));
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root]);
    let _ = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert_eq!(
        before,
        read_story_row(&db, &story_id),
        "transfer must never mutate the canonical row"
    );
}

#[test]
fn read_transfer_state_is_idle_when_the_pack_is_on_a_non_target_device() {
    // C1 — the authoritative re-read is PINNED to the requested device. A pack
    // present on a DIFFERENT writable device must read as `idle`, never
    // `transferred` (no false "écriture effectuée", no wrong-device attribution);
    // asking about the device that ACTUALLY holds it does report transferred.
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let pack = uuid([0x5A, 0x5A, 0x5A, 0x5A]);
    let (story_id, _pack_bytes, _src) = import_one(&db, app_data.path(), pack);

    // The connected device HOLDS this pack, but it is not the target we request.
    let (_dev, dev_root, dev_id, _) = build_mount_with_pack(3, pack);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![dev_root]);
    let other_target = "00000000000000000000000000000000";
    assert_ne!(dev_id.as_str(), other_target);

    let view = read_transfer_state(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        &story_id,
        other_target,
        budget(),
        budget(),
    )
    .expect("read state");
    assert_eq!(
        view,
        TransferStateView::Idle,
        "a pack on a non-target device must not read as transferred"
    );

    // Sanity: requesting the device that ACTUALLY holds the pack reports it.
    let view = read_transfer_state(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        &story_id,
        &dev_id,
        budget(),
        budget(),
    )
    .expect("read state");
    assert!(
        matches!(view, TransferStateView::Verified { .. }),
        "the targeted device that holds a byte-faithful pack reads as verified: {view:?}"
    );
}

fn read_story_row(db: &Mutex<DbHandle>, id: &str) -> (String, String, String) {
    db.lock()
        .unwrap()
        .conn()
        .query_row(
            "SELECT title, structure_json, content_checksum FROM stories WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read row")
}

#[test]
fn transfer_trace_channel_records_a_closed_pii_free_event_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = transfer_log::log_path_for(dir.path());
    let story_ref = transfer_log::story_ref("0197a5d0-0000-7000-8000-000000000000");
    transfer_log::record_event_at_path(
        &path,
        transfer_log::Event::TransferStarted {
            story_ref: story_ref.clone(),
        },
    )
    .expect("start");
    transfer_log::record_event_at_path(
        &path,
        transfer_log::Event::TransferCompleted {
            story_ref: story_ref.clone(),
            verify_verdict: "verified",
            elapsed_ms: 12,
        },
    )
    .expect("completed");
    transfer_log::record_event_at_path(
        &path,
        transfer_log::Event::TransferFailed {
            story_ref: story_ref.clone(),
            cause: Some("write_not_authorized"),
            completeness: Some("failed"),
            verify_verdict: None,
            elapsed_ms: 3,
        },
    )
    .expect("failed");
    transfer_log::record_event_at_path(
        &path,
        transfer_log::Event::TransferFailed {
            story_ref,
            cause: None,
            completeness: None,
            verify_verdict: Some("partial"),
            elapsed_ms: 4,
        },
    )
    .expect("verify partial");

    let contents = std::fs::read_to_string(&path).expect("read log");
    assert_eq!(contents.lines().count(), 4);
    assert!(contents.contains("transfer_started"));
    assert!(contents.contains("transfer_completed"));
    assert!(contents.contains("transfer_failed"));
    assert!(contents.contains("\"verify_verdict\":\"verified\""));
    assert!(contents.contains("\"verify_verdict\":\"partial\""));
    // PII-free: the raw story id never appears, only its short hash.
    assert!(!contents.contains("0197a5d0-0000-7000-8000-000000000000"));
}

/// Scanner wrapper proving the DB mutex is FREE during the device scan — the
/// service never holds the lock across the device I/O.
struct LockProbeScanner {
    inner: SystemDeviceScanner,
    db: Arc<Mutex<DbHandle>>,
    lock_free_during_scan: Arc<AtomicBool>,
}

impl DeviceScanner for LockProbeScanner {
    fn scan(&self, budget: Duration) -> Result<DeviceScanReport, AppError> {
        if self.db.try_lock().is_ok() {
            self.lock_free_during_scan.store(true, Ordering::SeqCst);
        }
        self.inner.scan(budget)
    }
}

#[test]
fn holds_no_db_lock_during_scan() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Arc::new(Mutex::new(open_db(&db_tmp)));
    let (story_id, _pack, _src) = import_one(&db, app_data.path(), uuid([3, 3, 3, 3]));

    let (_target, target_root, target_id, _) = build_mount_with_pack(3, uuid([4, 4, 4, 4]));
    let lock_free = Arc::new(AtomicBool::new(false));
    let scanner = LockProbeScanner {
        inner: SystemDeviceScanner::with_explicit_mount_roots(vec![target_root]),
        db: db.clone(),
        lock_free_during_scan: lock_free.clone(),
    };

    let outcome = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert!(
        matches!(outcome, TransferOutcome::Verified { .. }),
        "{outcome:?}"
    );
    assert!(
        lock_free.load(Ordering::SeqCst),
        "the DB mutex must be free during the device scan"
    );
}

/// A writer that performs the content promotion (mutating the device) then fails
/// the durability/index step — modelling a power loss / device yank right after
/// the rename. Reports `reached_device_mutation = true`, so the service yields the
/// honest `Incomplete` (`transfert incomplet`). A real deterministic post-promote
/// failure is impossible (staging, promote and `.pi` share the mount's
/// writability), so this instrument exercises that branch end-to-end.
struct IncompleteAfterPromoteWriter;

impl DevicePackWriter for IncompleteAfterPromoteWriter {
    fn write_pack(
        &self,
        mount_path: &Path,
        _source_pack_dir: &Path,
        _pack_uuid: &str,
        plan: &PackWritePlan,
        _budget: Duration,
        _progress: &dyn Fn(WriteProgress),
    ) -> Result<(), WriteFailure> {
        // Promote real content under `.content/<SHORT_ID>` — the device IS now
        // mutated — but never touch `.pi` (the durability/index step "failed").
        let target = mount_path.join(".content").join(&plan.short_id);
        std::fs::create_dir_all(&target).expect("create promoted dir");
        for file in &plan.files {
            let dst = target.join(&file.rel_path);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(&dst, b"PROMOTED").expect("write promoted file");
        }
        Err(WriteFailure {
            cause: TransferFailureCause::WriteRejected,
            reached_device_mutation: true,
        })
    }
}

#[test]
fn an_incomplete_transfer_leaves_promoted_content_unindexed_and_preserves_the_draft() {
    // AC2 `incomplet`: a durability/index failure AFTER the content promotion
    // surfaces the honest `transfert incomplet` — the device may hold an unindexed
    // partial copy, and the canonical draft is never touched (FR18).
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let pack = uuid([0x1C, 0x1C, 0x1C, 0x1C]);
    let (story_id, pack_bytes, _src) = import_one(&db, app_data.path(), pack);

    let (_target, target_root, target_id, _) =
        build_mount_with_pack(3, uuid([0x02, 0x02, 0x02, 0x02]));
    let pi_before = std::fs::read(target_root.join(".pi")).expect("read .pi");
    let canonical_before = read_story_row(&db, &story_id);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root.clone()]);
    let emitter = CapturingEmitter::default();

    let outcome = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &IncompleteAfterPromoteWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &emitter,
    );
    assert_eq!(
        outcome,
        TransferOutcome::Retryable {
            cause: TransferFailureCause::WriteRejected,
            completeness: TransferCompleteness::Incomplete,
        }
    );
    // The promoted content is present on the device but NOT indexed in `.pi`.
    let short = pack_short_id(&pack_bytes);
    assert!(
        target_root.join(".content").join(&short).exists(),
        "the promoted (partial) content is present"
    );
    let pi_after = std::fs::read(target_root.join(".pi")).expect("read .pi");
    assert!(
        !parse_pack_index(&pi_after)
            .uuids
            .iter()
            .any(|u| u == &pack_bytes),
        "the incomplete pack must NOT be indexed"
    );
    assert_eq!(
        pi_after, pi_before,
        ".pi is unchanged (the index step never ran)"
    );
    assert_eq!(
        read_story_row(&db, &story_id),
        canonical_before,
        "the canonical draft is preserved after an incomplete transfer"
    );
    assert!(
        emitter.events().iter().any(|e| e.starts_with("failed:")),
        "a failure terminal was emitted"
    );
}

#[test]
fn relaunching_a_transfer_converges_without_clobber() {
    // A relaunch is a FRESH full cycle (never a hidden partial resume): the
    // writer's prove-or-refuse reuse path converges on the healthy pack
    // idempotently — no duplicate index entry, no clobbered content.
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let pack = uuid([0x2C, 0x2C, 0x2C, 0x2C]);
    let (story_id, pack_bytes, _src) = import_one(&db, app_data.path(), pack);

    let (_target, target_root, target_id, _) =
        build_mount_with_pack(3, uuid([0x03, 0x03, 0x03, 0x03]));
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root.clone()]);

    let first = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert!(
        matches!(first, TransferOutcome::Verified { .. }),
        "{first:?}"
    );
    let short = pack_short_id(&pack_bytes);
    let content_after_first =
        std::fs::read(target_root.join(".content").join(&short).join("ni")).expect("ni");
    let pi_after_first = std::fs::read(target_root.join(".pi")).expect(".pi");

    // A write changes `.pi`, hence the device identifier Rustory derives from it;
    // a relaunch re-detects the device (as the UI does) and uses the fresh id.
    let target_id_after = compute_device_identifier(&pi_after_first, None);

    let second = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id_after,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert!(
        matches!(second, TransferOutcome::Verified { .. }),
        "a relaunch converges to the verified terminal: {second:?}"
    );
    assert_eq!(
        std::fs::read(target_root.join(".content").join(&short).join("ni")).expect("ni"),
        content_after_first,
        "the content is not clobbered on relaunch"
    );
    assert_eq!(
        std::fs::read(target_root.join(".pi")).expect(".pi"),
        pi_after_first,
        "no duplicate index entry on relaunch"
    );
    let count = parse_pack_index(&pi_after_first)
        .uuids
        .iter()
        .filter(|u| **u == pack_bytes)
        .count();
    assert_eq!(count, 1, "the transferred UUID is indexed exactly once");
}

/// A writer that promotes content with bytes that DIVERGE from the prepared pack
/// AND indexes the UUID, then succeeds — so the `verify` re-read finds the pack
/// present + indexed but byte-DIVERGENT, yielding the honest `Partial` verdict.
struct ByteDivergentWriter;

impl DevicePackWriter for ByteDivergentWriter {
    fn write_pack(
        &self,
        mount_path: &Path,
        _source_pack_dir: &Path,
        pack_uuid: &str,
        plan: &PackWritePlan,
        _budget: Duration,
        _progress: &dyn Fn(WriteProgress),
    ) -> Result<(), WriteFailure> {
        let target = mount_path.join(".content").join(&plan.short_id);
        std::fs::create_dir_all(&target).expect("create promoted dir");
        for file in &plan.files {
            let dst = target.join(&file.rel_path);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            // Non-empty (keeps the structure valid) but DIVERGENT from the prepared
            // bytes, so the re-checksum disagrees with the baseline.
            std::fs::write(&dst, vec![0x00u8; 8]).expect("write divergent file");
        }
        // Index the UUID so the verify re-read surfaces the pack (indexed + content
        // present) — only the bytes diverge.
        let pi_path = mount_path.join(".pi");
        let pi = std::fs::read(&pi_path).unwrap_or_default();
        let uuid_bytes = pack_uuid_bytes(pack_uuid).expect("canonical pack uuid");
        std::fs::write(&pi_path, append_pack_uuid(&pi, &uuid_bytes)).expect("write .pi");
        Ok(())
    }
}

#[test]
fn a_byte_divergent_write_verifies_as_partial() {
    // AC3 `partial`: the write lands and the pack is present + indexed, but the
    // device bytes do not re-checksum to the prepared baseline → `état partiel`,
    // never a silent success; the canonical draft stays intact (FR18).
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let (story_id, _pack, _src) = import_one(&db, app_data.path(), uuid([0x3C, 0x3C, 0x3C, 0x3C]));

    let (_target, target_root, target_id, _) =
        build_mount_with_pack(3, uuid([0x04, 0x04, 0x04, 0x04]));
    let canonical_before = read_story_row(&db, &story_id);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root]);

    let outcome = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &ByteDivergentWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert_eq!(
        outcome,
        TransferOutcome::Unverified {
            verdict: VerifyVerdict::Partial
        }
    );
    assert_eq!(
        read_story_row(&db, &story_id),
        canonical_before,
        "verify never mutates the canonical draft"
    );
}

/// A scanner that resolves the device for the preflight + the pre-write
/// re-validation, then reports NO device for the `verify` re-scan — modelling a
/// Lunii unplugged in the window between the write and the verification.
struct VanishBeforeVerifyScanner {
    inner: SystemDeviceScanner,
    scans: AtomicUsize,
}

impl DeviceScanner for VanishBeforeVerifyScanner {
    fn scan(&self, budget: Duration) -> Result<DeviceScanReport, AppError> {
        // Scans 1 (preflight) + 2 (F5 re-validation) see the device; scan 3
        // (verify) sees nothing.
        let n = self.scans.fetch_add(1, Ordering::SeqCst);
        if n >= 2 {
            return Ok(DeviceScanReport::empty(Duration::from_millis(1)));
        }
        self.inner.scan(budget)
    }
}

#[test]
fn a_device_gone_during_verify_yields_failed() {
    // AC3 `failed`: the write may have landed, but the device is gone before the
    // verify re-read can confirm it → `échec récupérable` (never a false success).
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let (story_id, _pack, _src) = import_one(&db, app_data.path(), uuid([0x4D, 0x4D, 0x4D, 0x4D]));

    let (_target, target_root, target_id, _) =
        build_mount_with_pack(3, uuid([0x05, 0x05, 0x05, 0x05]));
    let scanner = VanishBeforeVerifyScanner {
        inner: SystemDeviceScanner::with_explicit_mount_roots(vec![target_root]),
        scans: AtomicUsize::new(0),
    };

    let outcome = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert_eq!(
        outcome,
        TransferOutcome::Unverified {
            verdict: VerifyVerdict::Failed
        }
    );
}

/// A writer that promotes BYTE-FAITHFUL content under `.content/<SHORT_ID>` but
/// NEVER updates `.pi` (the index step "did not run"), then succeeds — models a
/// pack present + faithful on the device yet UNINDEXED.
struct PromoteWithoutIndexWriter;

impl DevicePackWriter for PromoteWithoutIndexWriter {
    fn write_pack(
        &self,
        mount_path: &Path,
        _source_pack_dir: &Path,
        _pack_uuid: &str,
        plan: &PackWritePlan,
        _budget: Duration,
        _progress: &dyn Fn(WriteProgress),
    ) -> Result<(), WriteFailure> {
        // The standard plausible pack equals the import baseline, so the verify
        // re-checksum reads it back as byte-faithful — only the index is missing.
        write_pack(&mount_path.join(".content").join(&plan.short_id));
        Ok(())
    }
}

#[test]
fn content_promoted_but_unindexed_verifies_as_partial() {
    // F3 (real): a byte-faithful `.content/<short>` whose UUID is NOT in `.pi` is
    // the device "mutated + present but incoherent" case ⇒ `état partiel`, never a
    // `Failed` (the verify probes the content folder independently of the index).
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let pack = uuid([0x6C, 0x6C, 0x6C, 0x6C]);
    let (story_id, pack_bytes, _src) = import_one(&db, app_data.path(), pack);

    let (_target, target_root, target_id, _) =
        build_mount_with_pack(3, uuid([0x06, 0x06, 0x06, 0x06]));
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![target_root.clone()]);

    let outcome = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &PromoteWithoutIndexWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert_eq!(
        outcome,
        TransferOutcome::Unverified {
            verdict: VerifyVerdict::Partial
        }
    );
    // The promoted content IS present on the device, but its UUID is NOT indexed.
    let short = pack_short_id(&pack_bytes);
    assert!(target_root.join(".content").join(&short).is_dir());
    let pi = std::fs::read(target_root.join(".pi")).expect("read .pi");
    assert!(
        !parse_pack_index(&pi).uuids.iter().any(|u| u == &pack_bytes),
        "the promoted-but-unindexed pack must NOT be in .pi"
    );
}

/// A scanner that resolves device A for the preflight + the pre-write
/// re-validation, then resolves a DIFFERENT device B for the `verify` re-scan —
/// models a Lunii swapped after the write for another supported device.
struct SwapBeforeVerifyScanner {
    target: SystemDeviceScanner,
    swapped: SystemDeviceScanner,
    scans: AtomicUsize,
}

impl DeviceScanner for SwapBeforeVerifyScanner {
    fn scan(&self, budget: Duration) -> Result<DeviceScanReport, AppError> {
        let n = self.scans.fetch_add(1, Ordering::SeqCst);
        if n >= 2 {
            self.swapped.scan(budget)
        } else {
            self.target.scan(budget)
        }
    }
}

#[test]
fn a_device_swapped_before_verify_yields_failed() {
    // F2 (real): after the write lands on device A, the Lunii is swapped for ANOTHER
    // supported device B that ALREADY holds the same pack + bytes. Without the
    // continuity check the three proofs would pass on B and emit a false `verified`
    // for the wrong device; the verify binds to the written device (mount/serial),
    // so the swap ends `Failed`.
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    let pack = uuid([0x7C, 0x7C, 0x7C, 0x7C]);
    let (story_id, _pack_bytes, _src) = import_one(&db, app_data.path(), pack);

    // Device A: the write target (a different pre-existing pack).
    let (_a, root_a, target_id, _) = build_mount_with_pack(3, uuid([0x07, 0x07, 0x07, 0x07]));
    // Device B: a DIFFERENT Lunii that already holds the SAME pack + bytes.
    let (_b, root_b, _b_id, _) = build_mount_with_pack(3, pack);
    assert_ne!(root_a, root_b);

    let scanner = SwapBeforeVerifyScanner {
        target: SystemDeviceScanner::with_explicit_mount_roots(vec![root_a]),
        swapped: SystemDeviceScanner::with_explicit_mount_roots(vec![root_b]),
        scans: AtomicUsize::new(0),
    };

    let outcome = transfer_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        &SystemDevicePackWriter,
        app_data.path(),
        &story_id,
        &target_id,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert_eq!(
        outcome,
        TransferOutcome::Unverified {
            verdict: VerifyVerdict::Failed
        },
        "a swap to another device — even one holding the same pack — is not the written device"
    );
}
