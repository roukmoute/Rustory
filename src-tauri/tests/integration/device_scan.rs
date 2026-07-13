use std::time::Duration;

use rustory_lib::infrastructure::device::{
    CandidateFacts, DeviceCandidate, DeviceScanner, SystemDeviceScanner,
};

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
    pub const FLAM_PRIMARY_MARKER: &str = ".mdf";
    pub const FLAM_STORY_DIR: &str = "str";
    pub const FLAM_CONFIG_DIR: &str = "etc";

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

    pub fn temp_flam_mount() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"FIXTURE_MDF_PAYLOAD").expect(".mdf");
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }

    pub fn temp_flam_mount_empty_mdf() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"").expect(".mdf");
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }

    pub fn temp_flam_mount_missing_dir(missing: &str) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"FIXTURE_MDF").expect(".mdf");
        for d in [FLAM_STORY_DIR, FLAM_CONFIG_DIR] {
            if d != missing {
                fs::create_dir(root.join(d)).expect("dir");
            }
        }
        (dir, root)
    }

    pub fn temp_flam_mount_oversize_mdf() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        let payload = vec![0x4Du8; 8 * 1024];
        fs::write(root.join(FLAM_PRIMARY_MARKER), &payload).expect(".mdf");
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }

    /// A volume carrying BOTH the Lunii markers (`.md` + `.pi`) AND the
    /// FLAM marker set: family precedence must classify it as a LUNII
    /// candidate, verbatim.
    pub fn temp_bimarker_mount(metadata_version: u8) -> (TempDir, PathBuf) {
        let (dir, root) = temp_lunii_mount(metadata_version);
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"FIXTURE_MDF").expect(".mdf");
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }

    /// `.mdf` is a SYMLINK to a regular file: the no-follow probe must
    /// refuse to count it as a FLAM marker.
    #[cfg(unix)]
    pub fn temp_flam_mount_symlink_mdf() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        let target = root.join("real_payload");
        fs::write(&target, b"FIXTURE_MDF").expect("target");
        std::os::unix::fs::symlink(&target, root.join(FLAM_PRIMARY_MARKER)).expect("symlink");
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }

    /// `str/` is a SYMLINK to a real directory: the no-follow directory
    /// check must not count it.
    #[cfg(unix)]
    pub fn temp_flam_mount_symlinked_str_dir() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(FLAM_PRIMARY_MARKER), b"FIXTURE_MDF").expect(".mdf");
        let real = root.join("real_dir");
        fs::create_dir(&real).expect("real dir");
        std::os::unix::fs::symlink(&real, root.join(FLAM_STORY_DIR)).expect("symlink str");
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("etc");
        (dir, root)
    }
}

fn lunii_facts(candidate: &DeviceCandidate) -> (&Vec<u8>, &Vec<u8>, bool) {
    match &candidate.facts {
        CandidateFacts::Lunii {
            metadata_payload,
            pi_payload,
            has_bt,
        } => (metadata_payload, pi_payload, *has_bt),
        other => panic!("expected Lunii facts, got {other:?}"),
    }
}

fn flam_facts(candidate: &DeviceCandidate) -> (&Vec<u8>, bool, bool) {
    match &candidate.facts {
        CandidateFacts::Flam {
            mdf_payload,
            has_str_dir,
            has_etc_dir,
        } => (mdf_payload, *has_str_dir, *has_etc_dir),
        other => panic!("expected Flam facts, got {other:?}"),
    }
}

#[test]
fn system_scanner_detects_lunii_v3_marker_under_temp_mount() {
    let (_g, root) = fixture::temp_lunii_mount(3);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root.clone()]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].mount_path, root);
    let (metadata_payload, _, has_bt) = lunii_facts(&report.candidates[0]);
    assert_eq!(metadata_payload[0], 3);
    assert!(has_bt);
    assert!(!report.truncated_due_to_timeout);
}

#[test]
fn system_scanner_detects_lunii_v6_marker_under_temp_mount() {
    let (_g, root) = fixture::temp_lunii_mount(6);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    let (metadata_payload, _, _) = lunii_facts(&report.candidates[0]);
    assert_eq!(metadata_payload[0], 6);
}

#[test]
fn system_scanner_detects_lunii_v7_marker_under_temp_mount() {
    let (_g, root) = fixture::temp_lunii_mount(7);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    let (metadata_payload, _, _) = lunii_facts(&report.candidates[0]);
    assert_eq!(metadata_payload[0], 7);
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
    let (_, pi_payload, _) = lunii_facts(&report.candidates[0]);
    assert!(pi_payload.is_empty());
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
    let (_, _, has_bt) = lunii_facts(&report.candidates[0]);
    assert!(!has_bt);
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
        .map(|c| lunii_facts(c).0[0])
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

#[test]
fn system_scanner_detects_conforming_flam_mount() {
    let (_g, root) = fixture::temp_flam_mount();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root.clone()]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].mount_path, root);
    let (mdf_payload, has_str_dir, has_etc_dir) = flam_facts(&report.candidates[0]);
    assert_eq!(mdf_payload.as_slice(), b"FIXTURE_MDF_PAYLOAD");
    assert!(has_str_dir);
    assert!(has_etc_dir);
}

#[test]
fn system_scanner_surfaces_flam_volume_with_empty_mdf_as_visible_candidate() {
    // An empty `.mdf` is a VISIBLE candidate (classified corrupt
    // downstream) — a broken FLAM must be seen, never a silent
    // "no device".
    let (_g, root) = fixture::temp_flam_mount_empty_mdf();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    let (mdf_payload, _, _) = flam_facts(&report.candidates[0]);
    assert!(mdf_payload.is_empty());
}

#[test]
fn system_scanner_surfaces_flam_volume_with_missing_required_dirs() {
    for missing in [fixture::FLAM_STORY_DIR, fixture::FLAM_CONFIG_DIR] {
        let (_g, root) = fixture::temp_flam_mount_missing_dir(missing);
        let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
        let report = scanner.scan(Duration::from_secs(1)).expect("scan");
        assert_eq!(report.candidates.len(), 1, "missing {missing}");
        let (_, has_str_dir, has_etc_dir) = flam_facts(&report.candidates[0]);
        assert_eq!(has_str_dir, missing != fixture::FLAM_STORY_DIR);
        assert_eq!(has_etc_dir, missing != fixture::FLAM_CONFIG_DIR);
    }
}

#[test]
fn system_scanner_ignores_flam_volume_with_oversize_mdf() {
    // An oversized `.mdf` is not a plausible FLAM — the volume is
    // ignored entirely (same discipline as the oversize Lunii `.md`).
    let (_g, root) = fixture::temp_flam_mount_oversize_mdf();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert!(report.candidates.is_empty());
}

#[test]
fn system_scanner_classifies_bimarker_volume_as_lunii_candidate() {
    // Family precedence is FIXED: `.md` present ⇒ Lunii candidate,
    // verbatim, even when the full FLAM marker set coexists.
    let (_g, root) = fixture::temp_bimarker_mount(3);
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    let (metadata_payload, pi_payload, has_bt) = lunii_facts(&report.candidates[0]);
    assert_eq!(metadata_payload[0], 3);
    assert_eq!(pi_payload.as_slice(), b"FIXTURE_PI_PAYLOAD");
    assert!(has_bt);
}

#[cfg(unix)]
#[test]
fn system_scanner_refuses_symlinked_mdf_marker() {
    // The FLAM probe is no-follow end to end: a symlinked `.mdf` does
    // not count as a marker, the volume is ignored.
    let (_g, root) = fixture::temp_flam_mount_symlink_mdf();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert!(report.candidates.is_empty());
}

/// Make `.mdf` unreadable (mode 000). Returns `false` when the running
/// process can still open it (root — permissions are inoperative), in
/// which case the caller skips: the I/O-failure path is unreachable.
#[cfg(unix)]
fn make_mdf_unreadable(root: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let mdf = root.join(fixture::FLAM_PRIMARY_MARKER);
    std::fs::set_permissions(&mdf, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    std::fs::File::open(&mdf).is_err()
}

#[cfg(unix)]
#[test]
fn system_scanner_ignores_flam_volume_with_unreadable_mdf_without_scan_error() {
    // Per-volume I/O failure on the FLAM path: the volume is IGNORED
    // and the scan succeeds with zero candidates — never a scan-level
    // error (`Détection indisponible` must not appear because one FLAM
    // marker is unreadable).
    let (_g, root) = fixture::temp_flam_mount();
    if !make_mdf_unreadable(&root) {
        return; // root: permissions inoperative, path untestable here.
    }
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner
        .scan(Duration::from_secs(1))
        .expect("scan must not fail on an unreadable FLAM marker");
    assert!(report.candidates.is_empty());
    assert!(!report.truncated_due_to_timeout);
}

#[cfg(unix)]
#[test]
fn system_scanner_unreadable_flam_volume_does_not_mask_a_healthy_lunii() {
    // The failing FLAM volume must not eat the scan: a healthy Lunii on
    // another mount is still detected.
    let (_gf, flam_root) = fixture::temp_flam_mount();
    if !make_mdf_unreadable(&flam_root) {
        return; // root: permissions inoperative, path untestable here.
    }
    let (_gl, lunii_root) = fixture::temp_lunii_mount(3);
    let scanner =
        SystemDeviceScanner::with_explicit_mount_roots(vec![flam_root, lunii_root.clone()]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].mount_path, lunii_root);
    let (metadata_payload, _, _) = lunii_facts(&report.candidates[0]);
    assert_eq!(metadata_payload[0], 3);
}

#[test]
fn system_scanner_ignores_volume_with_directory_md_entry_even_with_full_flam_set() {
    // Family precedence, historical shape: a `.md` DIRECTORY is not a
    // Lunii marker (`is_file()` refuses it) but its presence keeps the
    // volume out of the FLAM probe too — such a volume was ignored
    // before FLAM recognition and must stay ignored.
    let (_g, root) = fixture::temp_flam_mount();
    std::fs::create_dir(root.join(fixture::LUNII_PRIMARY_MARKER)).expect("mkdir .md");
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert!(report.candidates.is_empty());
}

#[cfg(unix)]
#[test]
fn system_scanner_does_not_count_symlinked_str_dir_as_real_directory() {
    // `str/` must be a REAL directory: a symlink to a directory does
    // not count (no-follow), so the candidate surfaces with
    // has_str_dir=false and classifies `metadataUnsupported` downstream.
    let (_g, root) = fixture::temp_flam_mount_symlinked_str_dir();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![root]);
    let report = scanner.scan(Duration::from_secs(1)).expect("scan");
    assert_eq!(report.candidates.len(), 1);
    let (_, has_str_dir, has_etc_dir) = flam_facts(&report.candidates[0]);
    assert!(!has_str_dir);
    assert!(has_etc_dir);
}
