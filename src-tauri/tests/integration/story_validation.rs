//! End-to-end integration of the story-validation `preflight`: the REAL system
//! scanner + REAL filesystem reader against a temp mount, composed with a real
//! on-disk SQLite store seeded with a `stories` row. Proves the whole
//! `read_story_validation` pipeline (authoritative re-scan → scoped DB lock →
//! canonical re-verification → verdict composition) on actual I/O, which the
//! unit tests with mocks cannot.
//!
//! The `#[cfg(test)]` fixtures module is not visible to this separate
//! integration test crate, so the mount is built inline (the same reason
//! `transfer_preview.rs` mirrors the marker writes).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustory_lib::application::device::preflight::{
    read_story_validation, BlockerCause, StoryValidationOutcome, Verdict,
};
use rustory_lib::domain::device::DeviceLibrary;
use rustory_lib::domain::shared::AppError;
use rustory_lib::domain::story::{content_checksum, Axis, CanonicalCause};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{
    compute_device_identifier, DeviceLibraryReader, SystemDeviceLibraryReader, SystemDeviceScanner,
};
use tempfile::TempDir;

const HEALTHY_JSON: &str = "{\"schemaVersion\":2,\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null}]}";

fn uuid(tail: [u8; 4]) -> [u8; 16] {
    let mut b = [0xAB; 16];
    b[12..16].copy_from_slice(&tail);
    b
}

/// Build a temp Lunii mount whose `.md` first byte is `version` and whose `.pi`
/// lists `visible` packs. Returns `(guard, root, expected_device_identifier)`.
fn build_mount(version: u8, visible: &[[u8; 16]]) -> (TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join(".md"), [version, 0xff, 0xaa]).expect("write .md");
    let mut pi = Vec::new();
    for u in visible {
        pi.extend_from_slice(u);
    }
    std::fs::write(root.join(".pi"), &pi).expect("write .pi");
    let identifier = compute_device_identifier(&pi, None);
    (dir, root, identifier)
}

fn open_db(tmp: &TempDir) -> DbHandle {
    let path = tmp.path().join("rustory.sqlite");
    let mut handle = db::open_at(&path).expect("open");
    db::run_migrations(&mut handle).expect("migrate");
    handle
}

fn insert_story(db: &Mutex<DbHandle>, id: &str, title: &str, structure_json: &str, checksum: &str) {
    db.lock()
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, ?2, 2, ?3, ?4, '2026-06-19T00:00:00.000Z', '2026-06-19T00:00:00.000Z')",
            rusqlite::params![id, title, structure_json, checksum],
        )
        .expect("insert story");
}

fn insert_healthy(db: &Mutex<DbHandle>, id: &str, title: &str) {
    insert_story(db, id, title, HEALTHY_JSON, &content_checksum(HEALTHY_JSON));
}

fn budget() -> Duration {
    Duration::from_secs(5)
}

#[test]
fn presumed_transferable_for_a_healthy_story_on_a_supported_lunii() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    insert_healthy(&db, "s1", "Mon histoire");

    let (_guard, root, identifier) = build_mount(7, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let outcome = read_story_validation(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "s1",
        &identifier,
        budget(),
    )
    .expect("validation");
    match outcome {
        StoryValidationOutcome::Ready {
            verdict,
            blockers,
            story_title,
            device_identifier,
            ..
        } => {
            assert_eq!(verdict, Verdict::PresumedTransferable);
            assert!(blockers.is_empty());
            assert_eq!(story_title, "Mon histoire");
            assert_eq!(device_identifier, identifier);
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn blocked_when_the_stored_checksum_no_longer_matches() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    // Healthy structure but a corrupt stored checksum — silent on-disk
    // corruption the integrity re-check must catch.
    insert_story(&db, "s1", "Corrompue", HEALTHY_JSON, &"0".repeat(64));

    let (_guard, root, identifier) = build_mount(3, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let outcome = read_story_validation(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "s1",
        &identifier,
        budget(),
    )
    .expect("validation");
    match outcome {
        StoryValidationOutcome::Ready {
            verdict, blockers, ..
        } => {
            assert_eq!(verdict, Verdict::Blocked);
            assert!(blockers.iter().any(|b| matches!(
                b.cause,
                BlockerCause::Canonical(CanonicalCause::ChecksumMismatch)
            )));
            // The block is on the canonical axis, not the device profile.
            assert!(blockers.iter().all(|b| b.axis == Axis::Structure));
        }
        other => panic!("expected Ready(blocked), got {other:?}"),
    }
}

#[test]
fn unsupported_lunii_rescan_yields_recoverable_device_changed() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    insert_healthy(&db, "s1", "Saine");

    // `.md` first byte 99 → unsupported metadata version. A non-empty `.pi`
    // keeps it out of the "corrupt markers" branch. The re-scan no longer
    // resolves to a readable supported device whose identity we can confirm, so
    // the validation surfaces a recoverable `device_changed` rather than a
    // verdict about an unconfirmed device — the `device_profile` axis stays
    // declared but unemitted in MVP.
    let (_guard, root, _identifier) = build_mount(99, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let err = read_story_validation(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "s1",
        "00000000000000000000000000000000",
        budget(),
    )
    .expect_err("an unsupported re-scan must fail recoverably");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
    assert_eq!(v["details"]["source"], "device_changed");
}

#[test]
fn no_device_when_no_supported_lunii_is_present() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    insert_healthy(&db, "s1", "Saine");

    // An empty mount root → no candidate → no device.
    let empty = tempfile::tempdir().expect("empty mount");
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![empty.path().to_path_buf()]);

    let outcome = read_story_validation(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "s1",
        "00000000000000000000000000000000",
        budget(),
    )
    .expect("validation");
    assert_eq!(outcome, StoryValidationOutcome::NoDevice);
}

#[test]
fn rejects_identifier_mismatch_as_recoverable_device_changed() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    insert_healthy(&db, "s1", "Saine");

    let (_guard, root, _identifier) = build_mount(7, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let err = read_story_validation(
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
fn missing_story_yields_recoverable_library_inconsistent() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Mutex::new(open_db(&db_tmp));
    // No story seeded.
    let (_guard, root, identifier) = build_mount(7, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let err = read_story_validation(
        &db,
        &scanner,
        &SystemDeviceLibraryReader,
        "ghost",
        &identifier,
        budget(),
    )
    .expect_err("a vanished story must fail recoverably");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "LIBRARY_INCONSISTENT");
    assert_eq!(v["details"]["source"], "story_validation");
    assert_eq!(v["details"]["cause"], "story_missing");
}

/// Reader wrapper that proves the DB mutex is FREE while the device inventory
/// is read — i.e. the service does not hold the lock across the device I/O.
struct LockProbeReader {
    db: Arc<Mutex<DbHandle>>,
    lock_free_during_read: Arc<AtomicBool>,
}

impl DeviceLibraryReader for LockProbeReader {
    fn read_library(&self, mount_path: &Path, budget: Duration) -> Result<DeviceLibrary, AppError> {
        if self.db.try_lock().is_ok() {
            self.lock_free_during_read.store(true, Ordering::SeqCst);
        }
        SystemDeviceLibraryReader.read_library(mount_path, budget)
    }
}

#[test]
fn holds_no_db_lock_during_scan() {
    let db_tmp = tempfile::tempdir().expect("db tempdir");
    let db = Arc::new(Mutex::new(open_db(&db_tmp)));
    insert_healthy(&db, "s1", "Saine");

    let (_guard, root, identifier) = build_mount(7, &[uuid([1, 1, 1, 1])]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);

    let lock_free = Arc::new(AtomicBool::new(false));
    let reader = LockProbeReader {
        db: db.clone(),
        lock_free_during_read: lock_free.clone(),
    };

    let outcome = read_story_validation(&db, &scanner, &reader, "s1", &identifier, budget())
        .expect("validation");
    assert!(matches!(outcome, StoryValidationOutcome::Ready { .. }));
    assert!(
        lock_free.load(Ordering::SeqCst),
        "the DB mutex must be free during the device inventory read"
    );
}
