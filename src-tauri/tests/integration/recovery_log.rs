//! Integration tests for the recovery-log diagnostics subsystem.
//!
//! Tests target `record_event_at_path` directly: this entry point is the
//! one the production code reaches once the app-data dir has been
//! resolved, and using it here keeps the integration test crate free of
//! a Tauri runtime spin-up.

use std::fs;
use std::path::Path;

use rustory_lib::infrastructure::diagnostics::recovery_log::{record_event_at_path, Event};
use tempfile::TempDir;

fn read_lines(path: &Path) -> Vec<String> {
    let raw = fs::read_to_string(path).expect("read");
    raw.lines().map(str::to_string).collect()
}

#[test]
fn record_apply_then_discard_produces_three_lines_in_order() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("diagnostics").join("recovery.jsonl");

    let story_id = "story-1".to_string();

    record_event_at_path(
        &path,
        Event::RecoveryDraftProposed {
            story_id: story_id.clone(),
        },
    )
    .expect("propose");
    record_event_at_path(
        &path,
        Event::RecoveryDraftApplied {
            story_id: story_id.clone(),
        },
    )
    .expect("apply");
    record_event_at_path(&path, Event::RecoveryDraftDiscarded { story_id }).expect("discard");

    let lines = read_lines(&path);
    assert_eq!(lines.len(), 3);

    let categories: Vec<String> = lines
        .iter()
        .map(|line| {
            let parsed: serde_json::Value = serde_json::from_str(line).expect("parsable");
            parsed["event"]["category"].as_str().unwrap().to_string()
        })
        .collect();
    assert_eq!(
        categories,
        vec![
            "recovery_draft_proposed",
            "recovery_draft_applied",
            "recovery_draft_discarded",
        ]
    );

    // Each line carries a monotonically non-decreasing timestamp.
    let timestamps: Vec<String> = lines
        .iter()
        .map(|line| {
            let parsed: serde_json::Value = serde_json::from_str(line).expect("parsable");
            parsed["ts"].as_str().unwrap().to_string()
        })
        .collect();
    for window in timestamps.windows(2) {
        assert!(
            window[1] >= window[0],
            "ts must be non-decreasing across appends: {} -> {}",
            window[0],
            window[1]
        );
    }
}

#[test]
fn record_event_with_no_pending_drafts_writes_no_interrupted_event() {
    // The interrupted-session probe lives in `lib.rs::run().setup`. This
    // test asserts that when a path is freshly created and the recovery
    // log is empty, no boot-time event leaks in by accident — only the
    // events the caller explicitly emits do.
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("diagnostics").join("recovery.jsonl");

    record_event_at_path(
        &path,
        Event::RecoveryDraftDiscarded {
            story_id: "id".into(),
        },
    )
    .expect("discard");

    let lines = read_lines(&path);
    assert_eq!(lines.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable");
    assert_eq!(parsed["event"]["category"], "recovery_draft_discarded");
}

#[test]
fn record_event_emits_interrupted_session_detected_with_story_ids_array() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("diagnostics").join("recovery.jsonl");

    record_event_at_path(
        &path,
        Event::InterruptedSessionDetected {
            story_ids: vec!["a".into(), "b".into(), "c".into()],
        },
    )
    .expect("interrupted");

    let lines = read_lines(&path);
    assert_eq!(lines.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable");
    assert_eq!(parsed["event"]["category"], "interrupted_session_detected");
    assert_eq!(
        parsed["event"]["story_ids"],
        serde_json::json!(["a", "b", "c"])
    );
}
