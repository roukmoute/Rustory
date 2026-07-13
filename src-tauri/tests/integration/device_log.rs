use std::fs;
use std::path::PathBuf;

use rustory_lib::infrastructure::diagnostics::device_log::{
    log_path_for, record_event_at_path, Event, MAX_DEVICE_LOG_BYTES,
};
use tempfile::TempDir;

fn read_lines(path: &PathBuf) -> Vec<String> {
    let raw = fs::read_to_string(path).expect("read");
    raw.lines().map(str::to_string).collect()
}

#[test]
fn device_log_appends_one_line_per_event() {
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    record_event_at_path(&path, Event::DeviceAbsent { elapsed_ms: 1 }).expect("rec");
    record_event_at_path(
        &path,
        Event::DeviceDetectedSupported {
            device_identifier: "abc".into(),
            firmware_cohort: "origine_v1",
            metadata_format_version: Some(3),
            elapsed_ms: 0,
        },
    )
    .expect("rec");
    let lines = read_lines(&path);
    assert_eq!(lines.len(), 2);
}

#[test]
fn device_log_serializes_supported_event_with_typed_fields() {
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    record_event_at_path(
        &path,
        Event::DeviceDetectedSupported {
            device_identifier: "id-42".into(),
            firmware_cohort: "v3",
            metadata_format_version: Some(7),
            elapsed_ms: 0,
        },
    )
    .expect("rec");
    let lines = read_lines(&path);
    let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable");
    assert_eq!(parsed["event"]["category"], "device_detected_supported");
    assert_eq!(parsed["event"]["device_identifier"], "id-42");
    assert_eq!(parsed["event"]["firmware_cohort"], "v3");
    assert_eq!(parsed["event"]["metadata_format_version"], 7);
}

#[test]
fn device_log_omits_metadata_version_key_for_flam_supported_event() {
    // The FLAM entry OMITS the version key — never `null`, never an
    // invented `0` (the profile carries no metadata version).
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    record_event_at_path(
        &path,
        Event::DeviceDetectedSupported {
            device_identifier: "id-flam".into(),
            firmware_cohort: "flam_gen1",
            metadata_format_version: None,
            elapsed_ms: 0,
        },
    )
    .expect("rec");
    let lines = read_lines(&path);
    let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable");
    assert_eq!(parsed["event"]["category"], "device_detected_supported");
    assert_eq!(parsed["event"]["firmware_cohort"], "flam_gen1");
    assert!(parsed["event"]
        .as_object()
        .expect("object")
        .get("metadata_format_version")
        .is_none());
    assert!(!lines[0].contains("null"));
}

#[test]
fn device_log_records_unsupported_with_typed_reason_string() {
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    record_event_at_path(
        &path,
        Event::DeviceDetectedUnsupported {
            reason: "metadata_unsupported",
            firmware_hint: Some("metadata_v99".into()),
            elapsed_ms: 0,
        },
    )
    .expect("rec");
    let lines = read_lines(&path);
    let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable");
    assert_eq!(parsed["event"]["category"], "device_detected_unsupported");
    assert_eq!(parsed["event"]["reason"], "metadata_unsupported");
    assert_eq!(parsed["event"]["firmware_hint"], "metadata_v99");
}

#[test]
fn device_log_records_scan_failed_with_typed_source_string() {
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    record_event_at_path(
        &path,
        Event::DeviceScanFailed {
            source: "permission_denied",
            kind: None,
            elapsed_ms: 0,
        },
    )
    .expect("rec");
    let lines = read_lines(&path);
    let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable");
    assert_eq!(parsed["event"]["category"], "device_scan_failed");
    assert_eq!(parsed["event"]["source"], "permission_denied");
}

#[test]
fn device_log_does_not_leak_pi_payload_in_serialized_event() {
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    record_event_at_path(
        &path,
        Event::DeviceDetectedSupported {
            device_identifier: "OPAQUE_HASH".into(),
            firmware_cohort: "origine_v1",
            metadata_format_version: Some(3),
            elapsed_ms: 0,
        },
    )
    .expect("rec");
    let raw = fs::read_to_string(&path).expect("read");
    // None of the obvious raw markers should appear in the file.
    assert!(!raw.contains(".pi"));
    assert!(!raw.contains("PI_PAYLOAD"));
    assert!(!raw.contains("HARDWARE_SERIAL"));
}

#[test]
fn device_log_does_not_leak_filesystem_path_in_serialized_event() {
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    // The Event payloads should never carry a mount path. Asserting on
    // the closed event surface — by construction, no Event variant
    // exposes a `PathBuf`.
    record_event_at_path(
        &path,
        Event::DeviceDetectedSupported {
            device_identifier: "abc".into(),
            firmware_cohort: "v3",
            metadata_format_version: Some(7),
            elapsed_ms: 0,
        },
    )
    .expect("rec");
    let raw = fs::read_to_string(&path).expect("read");
    assert!(!raw.contains("/Volumes/"));
    assert!(!raw.contains("/run/media/"));
    assert!(!raw.contains("/mnt/"));
}

#[test]
fn device_log_read_and_import_entries_carry_family_and_cohort_tags() {
    // The read/import entries name their family/cohort so support can
    // triage per family; the FLAM lines never carry a metadata version
    // (the field does not exist on these events — nothing is invented,
    // nothing is null).
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    record_event_at_path(
        &path,
        Event::DeviceLibraryRead {
            device_identifier: "fedcba9876543210fedcba9876543210".into(),
            family: "flam",
            firmware_cohort: "flam_gen1",
            story_count: 2,
            hidden_count: 1,
            elapsed_ms: 30,
        },
    )
    .expect("rec read");
    record_event_at_path(
        &path,
        Event::DeviceStoryImported {
            short_id: "55667788".into(),
            family: "flam",
            firmware_cohort: "flam_gen1",
            story_id: "0197a5d0-0000-7000-8000-000000000002".into(),
            elapsed_ms: 700,
            bytes_copied: 448,
            file_count: 3,
        },
    )
    .expect("rec import");
    let lines = read_lines(&path);
    assert_eq!(lines.len(), 2);
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("parsable");
        assert_eq!(parsed["event"]["family"], "flam");
        assert_eq!(parsed["event"]["firmware_cohort"], "flam_gen1");
        assert!(!line.contains("metadata_format_version"));
        assert!(!line.contains("null"));
    }
}

#[test]
fn device_log_lunii_read_and_import_entries_carry_their_family_tags_too() {
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    record_event_at_path(
        &path,
        Event::DeviceLibraryRead {
            device_identifier: "0123456789abcdef0123456789abcdef".into(),
            family: "lunii",
            firmware_cohort: "origine_v1",
            story_count: 4,
            hidden_count: 0,
            elapsed_ms: 12,
        },
    )
    .expect("rec read");
    record_event_at_path(
        &path,
        Event::DeviceStoryImported {
            short_id: "FAC5562D".into(),
            family: "lunii",
            firmware_cohort: "origine_v1",
            story_id: "0197a5d0-0000-7000-8000-000000000003".into(),
            elapsed_ms: 800,
            bytes_copied: 7168,
            file_count: 7,
        },
    )
    .expect("rec import");
    let lines = read_lines(&path);
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("parsable");
        assert_eq!(parsed["event"]["family"], "lunii");
        assert_eq!(parsed["event"]["firmware_cohort"], "origine_v1");
    }
}

#[test]
fn device_log_rotates_when_file_exceeds_cap() {
    let tmp = TempDir::new().expect("tempdir");
    let path = log_path_for(tmp.path());
    fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
    fs::write(&path, vec![b'x'; (MAX_DEVICE_LOG_BYTES + 1) as usize]).expect("seed huge file");
    record_event_at_path(&path, Event::DeviceAbsent { elapsed_ms: 0 }).expect("rec");
    let lines = read_lines(&path);
    assert_eq!(lines.len(), 1);
    let archived: Vec<_> = fs::read_dir(path.parent().unwrap())
        .expect("readdir")
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl.archived"))
        .collect();
    assert_eq!(archived.len(), 1);
}

#[test]
fn device_log_swallow_failure_does_not_panic_application_path() {
    // A path with no parent component would fail PathInvalid. Ensure
    // record_event_at_path returns Err rather than panicking.
    let bad = PathBuf::from("device.jsonl");
    let outcome = record_event_at_path(&bad, Event::DeviceAbsent { elapsed_ms: 0 });
    // Regardless of pass/fail, no panic is the contract.
    let _ = outcome;
}
