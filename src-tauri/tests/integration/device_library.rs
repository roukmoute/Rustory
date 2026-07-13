//! End-to-end integration of the device-library read: the REAL system
//! scanner + REAL filesystem reader exercised against a temp mount. This
//! proves the `mount_path` seam (scanner discovers the path → service
//! threads it → reader opens `.pi` / `.content` there) works on actual
//! file I/O, which the unit tests with mocks cannot.
//!
//! The `#[cfg(test)]` fixtures module is not visible to this separate
//! integration test crate, so the mount is built inline (the same reason
//! `device_scan.rs` mirrors the marker writes).

use std::path::PathBuf;
use std::time::Duration;

use rustory_lib::application::device::library::{read_device_library, DeviceLibraryOutcome};
use rustory_lib::domain::device::pack_short_id;
use rustory_lib::infrastructure::device::{
    compute_device_identifier, SystemDeviceLibraryReader, SystemDeviceScanner,
};
use tempfile::TempDir;

fn uuid(tail: [u8; 4]) -> [u8; 16] {
    let mut b = [0xAB; 16];
    b[12..16].copy_from_slice(&tail);
    b
}

/// Build a temp Lunii mount whose `.pi` lists `visible` packs and whose
/// `.content/<SHORT_ID>` folders exist for those flagged `true`. Returns
/// `(guard, root, expected_device_identifier)` — the identifier is the
/// hash the scanner derives from the `.pi` bytes (volume serial `None` on
/// this read path), recomputed here so the read can be addressed.
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

fn budget() -> Duration {
    Duration::from_secs(5)
}

#[test]
fn reads_real_inventory_through_scanner_and_reader() {
    let visible = [(uuid([1, 1, 1, 1]), true), (uuid([2, 2, 2, 2]), true)];
    let (_guard, root, identifier) = build_mount(7, &visible);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let reader = SystemDeviceLibraryReader;

    let outcome =
        read_device_library(&scanner, &reader, &identifier, budget()).expect("library read");
    match outcome {
        DeviceLibraryOutcome::Readable {
            device_identifier,
            library,
            ..
        } => {
            assert_eq!(device_identifier, identifier);
            assert_eq!(library.entries.len(), 2);
            assert!(library.entries.iter().all(|e| e.content_present));
            assert_eq!(library.entries[0].short_id, "01010101");
            assert_eq!(library.entries[1].short_id, "02020202");
        }
        other => panic!("expected Readable, got {other:?}"),
    }
}

#[test]
fn flags_orphan_pack_without_content_folder() {
    let visible = [(uuid([9, 9, 9, 9]), false)]; // referenced in .pi, no folder
    let (_guard, root, identifier) = build_mount(3, &visible);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let reader = SystemDeviceLibraryReader;

    let outcome = read_device_library(&scanner, &reader, &identifier, budget()).expect("read");
    match outcome {
        DeviceLibraryOutcome::Readable { library, .. } => {
            assert_eq!(library.entries.len(), 1);
            assert!(!library.entries[0].content_present);
        }
        other => panic!("expected Readable, got {other:?}"),
    }
}

#[test]
fn rejects_identifier_mismatch_as_recoverable_device_changed() {
    let visible = [(uuid([1, 1, 1, 1]), true)];
    let (_guard, root, _identifier) = build_mount(7, &visible);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let reader = SystemDeviceLibraryReader;

    // Ask for a different (valid-hex) device than the one present.
    let err = read_device_library(
        &scanner,
        &reader,
        "00000000000000000000000000000000",
        budget(),
    )
    .expect_err("identity mismatch must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
    assert_eq!(v["details"]["source"], "device_changed");
}

#[test]
fn no_device_when_mount_root_has_no_markers() {
    // An empty directory is not a Lunii: no `.md` → scanner yields no
    // candidate → the read resolves to None (local library stays intact).
    let dir = tempfile::tempdir().expect("tempdir");
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![dir.path().to_path_buf()]);
    let reader = SystemDeviceLibraryReader;

    let outcome =
        read_device_library(&scanner, &reader, "whatever", budget()).expect("read resolves");
    assert_eq!(outcome, DeviceLibraryOutcome::None);
}

// ---------------- FLAM inventory through the shared pipeline ----------------

mod flam_fixture {
    //! Thin per-file aliases over the SHARED harness fixture
    //! (`crate::flam_support`) — one FLAM mount construction for the
    //! whole integration crate.
    pub use crate::flam_support::temp_flam_mount_with_entries;

    pub const FLAM_UUID_A: &str = "12345678-9abc-def0-1122-334455667788";
    pub const FLAM_UUID_B: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeffff0000";
}

#[test]
fn reads_a_real_flam_inventory_in_index_order_through_scanner_and_reader() {
    // Signature path: a conforming fake FLAM with a two-story index
    // resolves through the REAL scanner + reader to a readable outcome —
    // index order preserved, shortId = uppercase last 8 hex, wire flags
    // honest (`alreadyImported` stays composed at the command layer from
    // an EMPTY provenance set here).
    use std::collections::{HashMap, HashSet};
    let (_guard, root, identifier) = flam_fixture::temp_flam_mount_with_entries(&[
        (flam_fixture::FLAM_UUID_A, false, true),
        (flam_fixture::FLAM_UUID_B, false, true),
    ]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let reader = SystemDeviceLibraryReader;

    let outcome = read_device_library(&scanner, &reader, &identifier, budget()).expect("read");
    let dto = rustory_lib::ipc::dto::DeviceLibraryDto::from_outcome(
        outcome,
        &HashSet::new(),
        &HashMap::new(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["kind"], "readable");
    assert_eq!(v["deviceIdentifier"], identifier);
    assert_eq!(v["stories"][0]["uuid"], flam_fixture::FLAM_UUID_A);
    assert_eq!(v["stories"][0]["shortId"], "55667788");
    assert_eq!(v["stories"][0]["hidden"], false);
    assert_eq!(v["stories"][0]["contentPresent"], true);
    assert_eq!(v["stories"][0]["alreadyImported"], false);
    assert_eq!(v["stories"][1]["uuid"], flam_fixture::FLAM_UUID_B);
}

#[test]
fn flam_hidden_entry_reads_hidden_true_from_the_hidden_root() {
    let (_guard, root, identifier) = flam_fixture::temp_flam_mount_with_entries(&[
        (flam_fixture::FLAM_UUID_A, false, true),
        (flam_fixture::FLAM_UUID_B, true, true),
    ]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let outcome = read_device_library(&scanner, &SystemDeviceLibraryReader, &identifier, budget())
        .expect("read");
    match outcome {
        DeviceLibraryOutcome::Readable { library, .. } => {
            assert_eq!(library.entries.len(), 2);
            assert!(!library.entries[0].hidden);
            assert!(library.entries[1].hidden);
            assert!(
                library.entries[1].content_present,
                "the hidden payload lives under str.hidden/"
            );
        }
        other => panic!("expected Readable, got {other:?}"),
    }
}

#[test]
fn flam_index_entry_without_story_folder_reads_content_absent() {
    let (_guard, root, identifier) =
        flam_fixture::temp_flam_mount_with_entries(&[(flam_fixture::FLAM_UUID_A, false, false)]);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let outcome = read_device_library(&scanner, &SystemDeviceLibraryReader, &identifier, budget())
        .expect("read");
    match outcome {
        DeviceLibraryOutcome::Readable { library, .. } => {
            assert_eq!(library.entries.len(), 1);
            assert!(!library.entries[0].content_present);
        }
        other => panic!("expected Readable, got {other:?}"),
    }
}

#[test]
fn flam_without_index_reads_a_legitimately_empty_inventory() {
    // `list` absent is NOT an error: a recognized FLAM without the index
    // file resolves to an EMPTY readable inventory.
    let (_guard, root, identifier) = flam_fixture::temp_flam_mount_with_entries(&[]);
    std::fs::remove_file(root.join("etc/library/list")).expect("drop list");
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let outcome = read_device_library(&scanner, &SystemDeviceLibraryReader, &identifier, budget())
        .expect("absent index must read");
    match outcome {
        DeviceLibraryOutcome::Readable { library, .. } => assert!(library.entries.is_empty()),
        other => panic!("expected Readable(empty), got {other:?}"),
    }
}

#[test]
fn flam_malformed_index_line_is_ignored_and_the_healthy_lines_still_list() {
    let (_guard, root, identifier) =
        flam_fixture::temp_flam_mount_with_entries(&[(flam_fixture::FLAM_UUID_A, false, true)]);
    std::fs::write(
        root.join("etc/library/list"),
        format!("not-a-uuid\n{}\n", flam_fixture::FLAM_UUID_A),
    )
    .expect("rewrite index");
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let outcome = read_device_library(&scanner, &SystemDeviceLibraryReader, &identifier, budget())
        .expect("read");
    match outcome {
        DeviceLibraryOutcome::Readable { library, .. } => {
            assert_eq!(library.entries.len(), 1);
            assert_eq!(library.entries[0].uuid, flam_fixture::FLAM_UUID_A);
            assert!(library.had_trailing_bytes, "the malformed line is flagged");
        }
        other => panic!("expected Readable, got {other:?}"),
    }
}
