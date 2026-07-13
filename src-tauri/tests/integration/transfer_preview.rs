//! End-to-end integration of the pre-transfer comparison: the REAL system
//! scanner + REAL filesystem reader against a temp mount, composed with a
//! real on-disk SQLite store seeded with a `story_imports` provenance row.
//! Proves the whole `read_transfer_preview` pipeline (authoritative re-scan →
//! inventory read → scoped DB lock → local↔device membership) on actual I/O,
//! which the unit tests with mocks cannot.
//!
//! The `#[cfg(test)]` fixtures module is not visible to this separate
//! integration test crate, so the mount is built inline (the same reason
//! `device_library.rs` mirrors the marker writes).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustory_lib::application::device::transfer::{read_transfer_preview, TransferPreviewOutcome};
use rustory_lib::domain::device::{format_pack_uuid, pack_short_id, DeviceLibrary};
use rustory_lib::domain::shared::AppError;
use rustory_lib::domain::story::content_checksum;
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{
    compute_device_identifier, DeviceLibraryReader, SystemDeviceLibraryReader, SystemDeviceScanner,
};
use tempfile::TempDir;

fn uuid(tail: [u8; 4]) -> [u8; 16] {
    let mut b = [0xAB; 16];
    b[12..16].copy_from_slice(&tail);
    b
}

/// Build a temp Lunii mount whose `.pi` lists `visible` packs and whose
/// `.content/<SHORT_ID>` folders exist for those flagged `true`. Returns
/// `(guard, root, expected_device_identifier)` — the identifier is the hash
/// the scanner derives from the `.pi` bytes (volume serial `None`), recomputed
/// here so the read can be addressed.
fn build_mount(version: u8, visible: &[([u8; 16], bool)]) -> (TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join(".md"), [version, 0xff, 0xaa]).expect("write .md");
    let mut pi = Vec::new();
    for (u, _present) in visible {
        pi.extend_from_slice(u);
    }
    std::fs::write(root.join(".pi"), &pi).expect("write .pi");
    for (u, present) in visible {
        if *present {
            std::fs::create_dir_all(root.join(".content").join(pack_short_id(u)))
                .expect("create content dir");
        }
    }
    let identifier = compute_device_identifier(&pi, None);
    (dir, root, identifier)
}

fn open_db(tmp: &TempDir) -> DbHandle {
    let path = tmp.path().join("rustory.sqlite");
    let mut handle = db::open_at(&path).expect("open");
    db::run_migrations(&mut handle).expect("migrate");
    handle
}

/// A COHERENT healthy v3 story: column version, structure bytes and checksum
/// agree — the preview outcomes under test must never ride on an accidental
/// integrity failure that would mask a real regression.
fn insert_story(db: &Mutex<DbHandle>, id: &str, title: &str) {
    let structure_json = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
    db.lock()
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, ?2, 3, ?3, ?4, '2026-06-16T00:00:00.000Z', '2026-06-16T00:00:00.000Z')",
            rusqlite::params![id, title, structure_json, content_checksum(structure_json)],
        )
        .expect("insert story");
}

fn insert_import(db: &Mutex<DbHandle>, story_id: &str, pack_uuid: &str) {
    db.lock()
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
             VALUES (?1, ?2, 'deadbeefdeadbeefdeadbeefdeadbeef', '2026-06-16T00:00:00.000Z', 8, 4096, ?3, 'lunii')",
            rusqlite::params![story_id, pack_uuid, "a".repeat(64)],
        )
        .expect("insert provenance");
}

fn budget() -> Duration {
    Duration::from_secs(5)
}

#[test]
fn reports_new_when_the_selected_pack_is_not_on_the_device() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    insert_story(&db, "s1", "Mon histoire"); // no import → never on device

    let visible = [(uuid([1, 1, 1, 1]), true), (uuid([2, 2, 2, 2]), true)];
    let (_guard, root, identifier) = build_mount(7, &visible);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let outcome = read_transfer_preview(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "s1",
        &identifier,
        budget(),
    )
    .expect("preview");
    match outcome {
        TransferPreviewOutcome::Ready {
            on_device,
            unchanged_count,
            transferable,
            story_title,
            ..
        } => {
            assert!(!on_device);
            assert_eq!(unchanged_count, 2);
            assert!(!transferable);
            assert_eq!(story_title, "Mon histoire");
        }
        other => panic!("expected Ready(new), got {other:?}"),
    }
}

#[test]
fn reports_replace_and_excludes_the_matched_pack_from_unchanged_count() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    let on_device_pack = uuid([1, 1, 1, 1]);
    insert_story(&db, "s1", "Déjà transférée");
    // Provenance links the local story to a pack that IS on the device.
    insert_import(&db, "s1", &format_pack_uuid(&on_device_pack));

    let visible = [
        (on_device_pack, true),
        (uuid([2, 2, 2, 2]), true),
        (uuid([3, 3, 3, 3]), true),
    ];
    let (_guard, root, identifier) = build_mount(3, &visible);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let outcome = read_transfer_preview(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "s1",
        &identifier,
        budget(),
    )
    .expect("preview");
    match outcome {
        TransferPreviewOutcome::Ready {
            on_device,
            unchanged_count,
            ..
        } => {
            assert!(on_device, "the linked pack is in the inventory");
            assert_eq!(unchanged_count, 2, "3 packs minus the matched one");
        }
        other => panic!("expected Ready(replace), got {other:?}"),
    }
}

#[test]
fn rejects_identifier_mismatch_as_recoverable_device_changed() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    insert_story(&db, "s1", "Mon histoire");

    let visible = [(uuid([1, 1, 1, 1]), true)];
    let (_guard, root, _identifier) = build_mount(7, &visible);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let err = read_transfer_preview(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "s1",
        "00000000000000000000000000000000",
        budget(),
    )
    .expect_err("identity mismatch must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
    assert_eq!(v["details"]["source"], "device_changed");
}

#[test]
fn transferable_is_false_for_every_supported_cohort() {
    // V1 = md v3, V2 = md v6, V3 = md v7 — all supported, all WriteStory=false.
    for version in [3u8, 6, 7] {
        let db_tmp = tempfile::tempdir().expect("db tempdir");
        let db = Mutex::new(open_db(&db_tmp));
        insert_story(&db, "s1", "Mon histoire");
        let visible = [(uuid([1, 1, 1, 1]), true)];
        let (_guard, root, identifier) = build_mount(version, &visible);
        let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

        let outcome = read_transfer_preview(
            &db,
            &scanner,
            &SystemDeviceLibraryReader,
            "s1",
            &identifier,
            budget(),
        )
        .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready { transferable, .. } => {
                assert!(
                    !transferable,
                    "md v{version} must not be transferable in MVP"
                );
            }
            other => panic!("expected Ready for md v{version}, got {other:?}"),
        }
    }
}

/// Reader wrapper that proves the DB mutex is FREE while the device inventory
/// is read — i.e. the service does not hold the lock across the device I/O.
struct LockProbeReader {
    db: Arc<Mutex<DbHandle>>,
    lock_free_during_read: Arc<AtomicBool>,
}

impl DeviceLibraryReader for LockProbeReader {
    fn read_library(
        &self,
        mount_path: &Path,
        family: rustory_lib::domain::device::DeviceFamily,
        budget: Duration,
    ) -> Result<DeviceLibrary, AppError> {
        // `try_lock` succeeds only if no other holder is in the lock right now.
        // The guard (if acquired) drops at the end of this statement.
        if self.db.try_lock().is_ok() {
            self.lock_free_during_read.store(true, Ordering::SeqCst);
        }
        SystemDeviceLibraryReader.read_library(mount_path, family, budget)
    }
}

#[test]
fn holds_no_db_lock_during_scan() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Arc::new(Mutex::new(open_db(&db_tmp)));
    insert_story(&db, "s1", "Mon histoire");

    let visible = [(uuid([1, 1, 1, 1]), true)];
    let (_guard, root, identifier) = build_mount(7, &visible);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let lock_free = Arc::new(AtomicBool::new(false));
    let reader = LockProbeReader {
        db: db.clone(),
        lock_free_during_read: lock_free.clone(),
    };

    let outcome = read_transfer_preview(&db, &scanner, &reader, "s1", &identifier, budget())
        .expect("preview");
    assert!(matches!(outcome, TransferPreviewOutcome::Ready { .. }));
    assert!(
        lock_free.load(Ordering::SeqCst),
        "the DB mutex must be free during the device inventory read"
    );
}
