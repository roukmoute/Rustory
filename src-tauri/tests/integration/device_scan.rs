use std::time::Duration;

use rustory_lib::infrastructure::device::{DeviceScanner, SystemDeviceScanner};

mod fixture {
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Mirror the markers manually here so the integration test does not
    // depend on the cfg(test)-only `fixtures.rs` module of the lib (which
    // is not visible from the integration crate boundary).
    pub const LUNII_PRIMARY_MARKER: &str = ".md";
    pub const LUNII_DEVICE_ID_MARKER: &str = ".pi";
    pub const LUNII_BINARY_TOKEN_MARKER: &str = ".bt";

    pub fn temp_lunii_mount(metadata_version: u8) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(
            root.join(LUNII_PRIMARY_MARKER),
            [metadata_version, 0xff, 0xaa],
        )
        .expect(".md");
        fs::write(root.join(LUNII_DEVICE_ID_MARKER), b"FIXTURE_PI_PAYLOAD").expect(".pi");
        fs::write(root.join(LUNII_BINARY_TOKEN_MARKER), b"FIXTURE_BT").expect(".bt");
        (dir, root)
    }

    pub fn temp_lunii_mount_no_pi(metadata_version: u8) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(LUNII_PRIMARY_MARKER), [metadata_version, 0xff]).expect(".md");
        fs::write(root.join(LUNII_BINARY_TOKEN_MARKER), b"FIXTURE_BT").expect(".bt");
        (dir, root)
    }

    pub fn temp_lunii_mount_no_bt(metadata_version: u8) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(LUNII_PRIMARY_MARKER), [metadata_version, 0xff]).expect(".md");
        fs::write(root.join(LUNII_DEVICE_ID_MARKER), b"FIXTURE_PI").expect(".pi");
        (dir, root)
    }

    pub fn temp_random_volume() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join("README.txt"), b"not a lunii").expect("write");
        (dir, root)
    }

    pub fn temp_lunii_mount_oversize_md() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        // 8 KB payload, well above the MAX_METADATA_FILE_BYTES = 4 KB
        // bound. The scan must cap the read.
        let payload = vec![3u8; 8 * 1024];
        fs::write(root.join(LUNII_PRIMARY_MARKER), &payload).expect(".md");
        fs::write(root.join(LUNII_DEVICE_ID_MARKER), b"FIXTURE_PI").expect(".pi");
        fs::write(root.join(LUNII_BINARY_TOKEN_MARKER), b"FIXTURE_BT").expect(".bt");
        (dir, root)
    }
}

#[test]
fn system_scanner_detects_lunii_v3_marker_under_temp_mount() {
    let (_g, root) = fixture::temp_lunii_mount(3);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root.clone()]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].mount_path, root);
    assert_eq!(report.candidates[0].metadata_payload[0], 3);
    assert!(report.candidates[0].has_bt);
    assert!(!report.truncated_due_to_timeout);
}

#[test]
fn system_scanner_detects_lunii_v6_marker_under_temp_mount() {
    let (_g, root) = fixture::temp_lunii_mount(6);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].metadata_payload[0], 6);
}

#[test]
fn system_scanner_detects_lunii_v7_marker_under_temp_mount() {
    let (_g, root) = fixture::temp_lunii_mount(7);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].metadata_payload[0], 7);
}

#[test]
fn system_scanner_skips_volume_without_dot_md() {
    let (_g, root) = fixture::temp_random_volume();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert!(report.candidates.is_empty());
}

#[test]
fn system_scanner_surfaces_volume_without_dot_pi_with_empty_pi_payload() {
    // `.md` present without `.pi` is a corrupt Lunii signal — the
    // user plugged in a device whose primary marker matches but the
    // device-id is missing. Surface the candidate so the application
    // classifier renders `MetadataCorrupt` instead of pretending no
    // device is connected at all.
    let (_g, root) = fixture::temp_lunii_mount_no_pi(3);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    assert!(report.candidates[0].pi_payload.is_empty());
}

#[test]
fn system_scanner_returns_candidate_with_has_bt_false_when_dot_bt_missing() {
    let (_g, root) = fixture::temp_lunii_mount_no_bt(3);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    // The candidate IS surfaced because `.md` + `.pi` are present;
    // domain classification (`classify_lunii(..., has_bt=false, ...)`)
    // is what converts it to MetadataCorrupt downstream.
    assert_eq!(report.candidates.len(), 1);
    assert!(!report.candidates[0].has_bt);
}

#[test]
fn system_scanner_rejects_oversize_metadata_marker() {
    // A genuine `.md` is < 200 B; an 8 KB payload means either disk
    // corruption or a non-Lunii device family planting decoy markers.
    // The scanner must drop the candidate rather than truncate-and-
    // accept it, otherwise the classifier would happily read a fake
    // version byte from the first byte of garbage.
    let (_g, root) = fixture::temp_lunii_mount_oversize_md();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert!(report.candidates.is_empty());
}

#[test]
fn system_scanner_returns_empty_when_no_volume_present() {
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert!(report.candidates.is_empty());
    assert!(!report.truncated_due_to_timeout);
}

#[test]
fn system_scanner_returns_multiple_candidates_when_two_lunii_volumes_present() {
    let (_g1, r1) = fixture::temp_lunii_mount(3);
    let (_g2, r2) = fixture::temp_lunii_mount(6);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![r1, r2]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 2);
    let versions: Vec<u8> = report
        .candidates
        .iter()
        .map(|c| c.metadata_payload[0])
        .collect();
    assert!(versions.contains(&3));
    assert!(versions.contains(&6));
}

#[test]
fn system_scanner_returns_truncated_when_budget_zero_and_roots_present() {
    let (_g, r1) = fixture::temp_lunii_mount(3);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![r1]);
    // A zero budget causes the very first elapsed check inside the
    // scan loop to trip the truncation flag without probing any root.
    let report = scanner.scan(Duration::from_millis(0)).expect("scan");
    assert!(report.truncated_due_to_timeout);
    assert!(report.candidates.is_empty());
}

#[test]
fn system_scanner_handles_disappeared_mount_between_enum_and_read() {
    let (g, root) = fixture::temp_lunii_mount(3);
    // Drop the TempDir before the scan : the path is enumerated but the
    // markers no longer exist. The scanner must not panic, must not
    // bubble a fatal error, and must report zero candidates.
    drop(g);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert!(report.candidates.is_empty());
    assert!(!report.truncated_due_to_timeout);
}

#[test]
fn system_scanner_with_default_construction_uses_sysinfo_enumeration() {
    // We only exercise the default constructor smoke path: we call
    // `default()` and expect a successful scan call (CI containers
    // never have a Lunii so the result is "empty"). The point is to
    // assert that the production code path compiles and does not
    // panic.
    let scanner = SystemDeviceScanner::default();
    let report = scanner.scan(Duration::from_millis(500)).expect("scan");
    // No assertion on candidate count — depends on host disks.
    let _ = report;
}
