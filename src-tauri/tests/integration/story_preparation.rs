//! End-to-end integration of story preparation: the REAL system scanner + REAL
//! filesystem assembler against a temp mount, composed with a real on-disk
//! SQLite store. Proves the whole `prepare_story` / `read_preparation_state`
//! pipeline (authoritative re-scan → scoped DB lock → canonical preflight →
//! local artifact assembly + integrity re-check → outcome + events) on actual
//! I/O, including the strongest check: a pack imported by story 2.x's real
//! import re-checksums to the SAME aggregate (no `ArtifactCorrupt`).
//!
//! The `#[cfg(test)]` fixtures + mocks are not visible to this separate test
//! crate, so the mount + a capturing emitter are built inline.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustory_lib::application::device::check_operation_allowed;
use rustory_lib::application::device::import::{import_device_story, ImportDeviceStoryRequest};
use rustory_lib::application::transfer::{
    prepare_story, read_preparation_state, PreparationEventEmitter, PreparationOutcome,
    PreparationStateView,
};
use rustory_lib::domain::device::{
    classify_lunii, format_pack_uuid, pack_short_id, DeviceProfileClassification,
    SupportedOperation,
};
use rustory_lib::domain::shared::AppError;
use rustory_lib::domain::story::content_checksum;
use rustory_lib::domain::transfer::{PreparationFailureCause, PreparationPhase};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{
    compute_device_identifier, DeviceScanReport, DeviceScanner, SystemDeviceLibraryReader,
    SystemDevicePackReader, SystemDeviceScanner,
};
use rustory_lib::infrastructure::filesystem::{
    resolve_import_story_dir, SystemTransferArtifactSource,
};
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

/// A capturing emitter recording the phase/terminal sequence.
#[derive(Default)]
struct CapturingEmitter {
    events: Mutex<Vec<String>>,
}

impl PreparationEventEmitter for CapturingEmitter {
    fn progress(&self, phase: PreparationPhase, _progress: Option<f32>, sequence: u64) {
        self.events
            .lock()
            .unwrap()
            .push(format!("progress:{}:{}", phase.wire_tag(), sequence));
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
}

/// Markers-only mount (no pack) for native-story scenarios.
fn build_mount(version: u8, visible: &[[u8; 16]]) -> (TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join(".md"), [version, 0xff, 0xaa]).expect(".md");
    let mut pi = Vec::new();
    for u in visible {
        pi.extend_from_slice(u);
    }
    std::fs::write(root.join(".pi"), &pi).expect(".pi");
    let identifier = compute_device_identifier(&pi, None);
    (dir, root, identifier)
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

/// Mount with one importable pack (markers + `.pi` + `.content/<SHORT_ID>`).
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

fn insert_native_story(db: &Mutex<DbHandle>, id: &str, structure_json: &str, checksum: &str) {
    db.lock()
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, 'Mon histoire', 2, ?2, ?3, '2026-06-22T00:00:00.000Z', '2026-06-22T00:00:00.000Z')",
            rusqlite::params![id, structure_json, checksum],
        )
        .expect("insert story");
}

#[test]
fn prepared_for_a_healthy_native_story_on_a_supported_lunii() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    insert_native_story(&db, "s1", HEALTHY_JSON, &content_checksum(HEALTHY_JSON));

    let (_guard, root, identifier) = build_mount(7, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let emitter = CapturingEmitter::default();

    let outcome = prepare_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        "s1",
        &identifier,
        budget(),
        budget(),
        &emitter,
    );
    assert!(
        matches!(outcome, PreparationOutcome::Prepared { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        emitter.events(),
        vec![
            "progress:preflight:1".to_string(),
            "progress:prepare:2".to_string(),
            "completed:3".to_string(),
        ]
    );
}

#[test]
fn prepared_for_an_imported_pack_re_checksums_to_the_stored_aggregate() {
    // The strongest check: a pack acquired by story 2.x's REAL import must
    // re-checksum to the SAME aggregate the import recorded — otherwise the
    // preparation would (wrongly) report ArtifactCorrupt.
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));

    // Metadata v3 = OrigineV1, where import is allowed.
    let (_guard, root, identifier, pack_uuid) =
        build_mount_with_pack(3, uuid([0xFA, 0xC5, 0x56, 0x2D]));
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let imported = import_device_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemDevicePackReader,
        app_data.path(),
        &ImportDeviceStoryRequest {
            device_identifier: identifier.clone(),
            pack_uuid,
        },
        budget(),
    )
    .expect("import");
    let story_id = imported.story.id;

    let emitter = CapturingEmitter::default();
    let outcome = prepare_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        &story_id,
        &identifier,
        budget(),
        budget(),
        &emitter,
    );
    assert!(
        matches!(outcome, PreparationOutcome::Prepared { .. }),
        "the re-checksum must match the import's pack_checksum: {outcome:?}"
    );
}

#[test]
fn retryable_artifact_missing_when_a_pack_file_disappears() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));

    let (_guard, root, identifier, pack_uuid) =
        build_mount_with_pack(3, uuid([0x01, 0x02, 0x03, 0x04]));
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let imported = import_device_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemDevicePackReader,
        app_data.path(),
        &ImportDeviceStoryRequest {
            device_identifier: identifier.clone(),
            pack_uuid,
        },
        budget(),
    )
    .expect("import");
    let story_id = imported.story.id;

    // A required file vanishes from the promoted pack after import.
    std::fs::remove_file(resolve_import_story_dir(app_data.path(), &story_id).join("si"))
        .expect("drop required file");

    let emitter = CapturingEmitter::default();
    let outcome = prepare_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        &story_id,
        &identifier,
        budget(),
        budget(),
        &emitter,
    );
    assert_eq!(
        outcome,
        PreparationOutcome::Retryable {
            cause: PreparationFailureCause::ArtifactMissing
        }
    );
}

#[test]
fn retryable_preflight_not_passing_when_checksum_is_corrupt() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    // Healthy structure but a corrupt stored checksum.
    insert_native_story(&db, "s1", HEALTHY_JSON, &"0".repeat(64));

    let (_guard, root, identifier) = build_mount(3, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let emitter = CapturingEmitter::default();

    let outcome = prepare_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        "s1",
        &identifier,
        budget(),
        budget(),
        &emitter,
    );
    assert_eq!(
        outcome,
        PreparationOutcome::Retryable {
            cause: PreparationFailureCause::PreflightNotPassing
        }
    );
    // Never entered the prepare phase.
    assert_eq!(
        emitter.events(),
        vec!["progress:preflight:1".to_string(), "failed:2".to_string()]
    );
}

#[test]
fn retryable_device_changed_when_no_supported_lunii_is_present() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    insert_native_story(&db, "s1", HEALTHY_JSON, &content_checksum(HEALTHY_JSON));

    // An empty mount root → no device.
    let empty = tempfile::tempdir().expect("empty mount");
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![empty.path().to_path_buf()]);
    let emitter = CapturingEmitter::default();

    let outcome = prepare_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        "s1",
        "00000000000000000000000000000000",
        budget(),
        budget(),
        &emitter,
    );
    assert_eq!(
        outcome,
        PreparationOutcome::Retryable {
            cause: PreparationFailureCause::DeviceChanged
        }
    );
}

#[test]
fn the_canonical_story_is_unchanged_after_a_failed_preparation() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    insert_native_story(&db, "s1", HEALTHY_JSON, &"0".repeat(64)); // corrupt → fails

    let read_row = |db: &Mutex<DbHandle>| -> (String, String, String) {
        db.lock()
            .unwrap()
            .conn()
            .query_row(
                "SELECT title, structure_json, content_checksum FROM stories WHERE id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read row")
    };

    let before = read_row(&db);
    let (_guard, root, identifier) = build_mount(3, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let _ = prepare_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        "s1",
        &identifier,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert_eq!(
        before,
        read_row(&db),
        "preparation must never mutate the canonical row"
    );
}

#[test]
fn read_preparation_state_matches_a_prepared_native_story() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    insert_native_story(&db, "s1", HEALTHY_JSON, &content_checksum(HEALTHY_JSON));

    let (_guard, root, _identifier) = build_mount(6, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let view = read_preparation_state(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        "s1",
        budget(),
        budget(),
    )
    .expect("read state");
    match view {
        PreparationStateView::Prepared {
            story_title,
            target_cohort,
            ..
        } => {
            assert_eq!(story_title, "Mon histoire");
            assert_eq!(target_cohort, "mid_gen_v2");
        }
        other => panic!("expected Prepared, got {other:?}"),
    }
}

#[test]
fn read_preparation_state_is_idle_without_a_device() {
    let db_tmp = tempfile::tempdir().expect("db dir");
    let app_data = tempfile::tempdir().expect("app data");
    let db = Mutex::new(open_db(&db_tmp));
    insert_native_story(&db, "s1", HEALTHY_JSON, &content_checksum(HEALTHY_JSON));

    let empty = tempfile::tempdir().expect("empty mount");
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![empty.path().to_path_buf()]);

    let view = read_preparation_state(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        "s1",
        budget(),
        budget(),
    )
    .expect("read state");
    assert_eq!(view, PreparationStateView::Idle);
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
    insert_native_story(&db, "s1", HEALTHY_JSON, &content_checksum(HEALTHY_JSON));

    let (_guard, root, identifier) = build_mount(7, &[uuid([1, 1, 1, 1])]);
    let lock_free = Arc::new(AtomicBool::new(false));
    let scanner = LockProbeScanner {
        inner: SystemDeviceScanner::with_explicit_mount_roots(vec![root]),
        db: db.clone(),
        lock_free_during_scan: lock_free.clone(),
    };

    let outcome = prepare_story(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        &SystemTransferArtifactSource,
        app_data.path(),
        "s1",
        &identifier,
        budget(),
        budget(),
        &CapturingEmitter::default(),
    );
    assert!(matches!(outcome, PreparationOutcome::Prepared { .. }));
    assert!(
        lock_free.load(Ordering::SeqCst),
        "the DB mutex must be free during the device scan"
    );
}

#[test]
fn preparation_does_not_change_the_write_gate_for_any_mvp_cohort() {
    // FR34: preparation is ORTHOGONAL to the send gate — the preparation flow
    // never consults `WriteStory`, and after a successful prepare the write
    // capability is still exactly what the cohort gate dictates (V1/V2 writable
    // since Epic 3, V3 refused), unchanged by the preparation.
    for (version, expected_cohort, writable) in [
        (3u8, "origine_v1", true),
        (6, "mid_gen_v2", true),
        (7, "v3", false),
    ] {
        let db_tmp = tempfile::tempdir().expect("db dir");
        let app_data = tempfile::tempdir().expect("app data");
        let db = Mutex::new(open_db(&db_tmp));
        insert_native_story(&db, "s1", HEALTHY_JSON, &content_checksum(HEALTHY_JSON));

        let (_guard, root, identifier) = build_mount(version, &[uuid([1, 1, 1, 1])]);
        let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
        let outcome = prepare_story(
            &db,
            &scanner,
            &SystemDeviceLibraryReader,
            &SystemTransferArtifactSource,
            app_data.path(),
            "s1",
            &identifier,
            budget(),
            budget(),
            &CapturingEmitter::default(),
        );
        assert!(
            matches!(outcome, PreparationOutcome::Prepared { .. }),
            "md v{version} should prepare"
        );

        // Preparation did not alter the gate: write capability is still exactly
        // what the cohort dictates (V1/V2 writable, V3 refused).
        let profile = match classify_lunii(version, true, true, &identifier) {
            DeviceProfileClassification::Supported(p) => p,
            other => panic!("expected Supported, got {other:?}"),
        };
        assert_eq!(profile.firmware_cohort.diagnostic_tag(), expected_cohort);
        assert_eq!(
            check_operation_allowed(&profile, SupportedOperation::WriteStory).is_ok(),
            writable,
            "md v{version}: preparation must not change the write gate"
        );
    }
}
