//! Append-only JSONL log of device-detection events.
//!
//! Sibling to `recovery_log.rs`: each call to [`record_event`] writes a
//! single newline-terminated JSON line to
//! `{app_data_dir}/diagnostics/device.jsonl`. The line carries a stable
//! `category` token from a closed set — that token is the NFR24
//! identifier support uses to triage. No raw OS messages, no localized
//! copy, no `.pi` payload, no filesystem path.
//!
//! Rotation reuses the shared JSONL helper: when the file would exceed
//! [`MAX_DEVICE_LOG_BYTES`], it is renamed to a timestamped `.archived`
//! sibling before the new line is written.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::jsonl::{self, JsonlError};
use crate::domain::shared::AppError;

/// Soft cap on the live `device.jsonl` file.
pub const MAX_DEVICE_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Diagnostic events emitted by the device-detection flow. The
/// `category` tag is the NFR24 stable identifier; the variant set is
/// closed by design so a log consumer can grep / route on it without
/// surprise.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum Event {
    /// A scan was successfully classified as "no device". Carries the
    /// elapsed time so support can spot scans that approached the
    /// budget without finding anything.
    DeviceAbsent { elapsed_ms: u64 },
    /// A supported device was detected. Carries the opaque
    /// `device_identifier` (already hashed — never the raw payload),
    /// the firmware cohort tag, the metadata format version and the
    /// wall-clock elapsed time of the full scan pipeline. The version
    /// key is OMITTED for profiles that carry none (FLAM) — never
    /// `null`, never an invented `0`.
    DeviceDetectedSupported {
        device_identifier: String,
        firmware_cohort: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata_format_version: Option<u8>,
        elapsed_ms: u64,
    },
    /// A candidate was found but the profile is not in the allow-list.
    /// `reason` is a closed-set tag mirroring `UnsupportedReason`;
    /// `firmware_hint` is diagnostic-only (never user copy) and may
    /// contain a metadata version hint such as "metadata_v99".
    DeviceDetectedUnsupported {
        reason: &'static str,
        firmware_hint: Option<String>,
        elapsed_ms: u64,
    },
    /// The scan transport itself failed (OS enumeration, permission,
    /// timeout, mount disappeared between enumerate and read, mutex
    /// poisoned). `source` is closed-set. `kind` mirrors the typed
    /// `details.kind` carried by the upstream `AppError` (e.g.
    /// `permission_denied`, `timeout`) so support can triage without
    /// parsing the user-facing message.
    DeviceScanFailed {
        source: &'static str,
        kind: Option<String>,
        elapsed_ms: u64,
    },
    /// Auto-mount of a plugged Lunii volume succeeded via udisks2
    /// D-Bus. `device_class` is a low-cardinality tag (e.g. `sd_*`)
    /// derived from the host device path. The raw path (`/dev/sda1`)
    /// is intentionally NOT logged: it embeds the user's session
    /// state and would leak unnecessary detail into the diagnostic
    /// stream — the runbook bans absolute filesystem paths from
    /// `device.jsonl` for exactly this reason.
    DeviceAutomounted { device_class: &'static str },
    /// Auto-mount was attempted but failed. `reason` is a closed-set
    /// token from `MountOutcome::Failed`; see `automount.rs`.
    DeviceAutomountFailed {
        device_class: &'static str,
        reason: &'static str,
    },
    /// The device-side library inventory was read. Carries the opaque
    /// (hashed) `device_identifier`, the FAMILY/COHORT tags of the
    /// re-scanned profile (`"lunii"`/`"origine_v1"`, `"flam"`/
    /// `"flam_gen1"` — so support can triage per family; no metadata
    /// version is ever carried here, so nothing is invented for FLAM),
    /// the pack COUNTS (never the raw pack UUIDs — keeping the line
    /// PII-free and bounded regardless of library size) and the
    /// wall-clock elapsed time.
    DeviceLibraryRead {
        device_identifier: String,
        family: &'static str,
        firmware_cohort: &'static str,
        story_count: u32,
        hidden_count: u32,
        elapsed_ms: u64,
    },
    /// Reading the device-side library failed. `source` is closed-set
    /// (e.g. `fs_read`, `pack_index`, `read_timeout`, `device_changed`,
    /// `scan_timeout`); `kind` mirrors the upstream `details.kind` when
    /// present. No path, no raw UUID.
    DeviceLibraryReadFailed {
        source: &'static str,
        kind: Option<String>,
        elapsed_ms: u64,
    },
    /// A device pack was copied into the local library. Carries the
    /// opaque `short_id` (NEVER the full pack UUID), the FAMILY/COHORT
    /// tags of the re-scanned profile (same closed sets as the read
    /// entry), the created local `story_id`, the copy size/count and the
    /// wall-clock elapsed time. No absolute path — same PII rules as
    /// every device event.
    DeviceStoryImported {
        short_id: String,
        family: &'static str,
        firmware_cohort: &'static str,
        story_id: String,
        elapsed_ms: u64,
        bytes_copied: u64,
        file_count: u32,
    },
    /// A device-pack import failed. `source` is the closed import
    /// taxonomy (`already_imported`, `pack_missing`, `pack_invalid`,
    /// `pack_oversize`, `device_changed`, `fs_read`, `staging_write`,
    /// `promote`, `db_commit`, `read_timeout`, `capability_gate`,
    /// `spawn_blocking_join`, `other`); `kind` mirrors the upstream
    /// `details.kind` when present.
    DeviceStoryImportFailed {
        source: &'static str,
        kind: Option<String>,
        elapsed_ms: u64,
    },
    /// A device story was deleted (delisted + content removed). Carries the
    /// FAMILY/COHORT tags of the re-scanned profile, whether the pack was
    /// actually present (`false` = idempotent no-op) and the wall-clock
    /// elapsed time. NO pack UUID / short id — a deletion needs no per-pack
    /// identifier in the trace (same PII discipline as every device event).
    DeviceStoryDeleted {
        family: &'static str,
        firmware_cohort: &'static str,
        was_present: bool,
        elapsed_ms: u64,
    },
    /// A device-story delete failed. `source` is the closed delete taxonomy
    /// (`device_changed`, `capability_gate`, `delete_rejected`,
    /// `spawn_blocking_join`, `other`).
    DeviceStoryDeleteFailed {
        source: &'static str,
        elapsed_ms: u64,
    },
    /// A pack archive (`.zip`) was sent to the device (transcoded, ciphered
    /// for its `.md` key, written atomically). Carries the family/cohort tags
    /// of the re-scanned profile and the pack's distinct asset COUNTS — never
    /// the pack UUID or a path (the line stays PII-free and bounded).
    DevicePackSent {
        family: &'static str,
        firmware_cohort: &'static str,
        image_count: u32,
        audio_count: u32,
        elapsed_ms: u64,
    },
    /// A pack-archive send failed. `source` is the closed send taxonomy
    /// (`device_changed`, `capability_gate`, `archive`, `device_write`,
    /// `dialog`, `spawn_blocking_join`, `other`).
    DevicePackSendFailed {
        source: &'static str,
        elapsed_ms: u64,
    },
}

/// Append a single event to the device log. Production entry point —
/// wrap an [`AppHandle`] to get the app-data dir from the Tauri
/// runtime. Tests use [`record_event_at_path`] directly.
pub fn record_event(app: &AppHandle, event: Event) -> Result<(), AppError> {
    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        AppError::device_scan_failed(
            "Détection indisponible: trace locale inaccessible.",
            "Vérifie les permissions de ton dossier utilisateur puis relance.",
        )
        .with_details(serde_json::json!({
            "source": "diagnostics_app_data_dir",
        }))
    })?;
    let log_path = log_path_for(&app_data_dir);
    record_event_at_path(&log_path, event)
}

/// Resolve the canonical log path inside an app-data dir.
pub fn log_path_for(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("diagnostics").join("device.jsonl")
}

/// Path-direct entry point. Exposed `pub` rather than `pub(crate)` so the
/// integration test crate can exercise it without a Tauri runtime.
pub fn record_event_at_path(log_path: &Path, event: Event) -> Result<(), AppError> {
    jsonl::append_event(log_path, &event, MAX_DEVICE_LOG_BYTES, "device").map_err(map_jsonl_error)
}

fn map_jsonl_error(err: JsonlError) -> AppError {
    let (source, kind) = match &err {
        JsonlError::DirNotWritable { kind } => ("diagnostics_dir", kind.as_str()),
        JsonlError::PathInvalid => ("diagnostics_path_invalid", "n_a"),
        JsonlError::Open(kind) => ("diagnostics_open", kind.as_str()),
        JsonlError::Write(kind) => ("diagnostics_write", kind.as_str()),
        JsonlError::Serialize => ("diagnostics_serialize", "n_a"),
        JsonlError::SystemClock => ("diagnostics_clock", "n_a"),
        JsonlError::Rotate(kind) => ("diagnostics_rotate", kind.as_str()),
    };
    AppError::device_scan_failed(
        "Détection indisponible: trace locale inaccessible.",
        "Vérifie l'espace disque et les permissions de ton dossier utilisateur.",
    )
    .with_details(serde_json::json!({
        "source": source,
        "kind": kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn read_lines(path: &Path) -> Vec<String> {
        let raw = fs::read_to_string(path).expect("read");
        raw.lines().map(str::to_string).collect()
    }

    #[test]
    fn event_device_absent_serializes_with_camel_case_payload_and_snake_case_category() {
        let event = Event::DeviceAbsent { elapsed_ms: 42 };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_absent");
        assert_eq!(v["elapsed_ms"], 42);
    }

    #[test]
    fn event_device_detected_supported_carries_typed_fields() {
        let event = Event::DeviceDetectedSupported {
            device_identifier: "abc".into(),
            firmware_cohort: "origine_v1",
            metadata_format_version: Some(3),
            elapsed_ms: 42,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_detected_supported");
        assert_eq!(v["device_identifier"], "abc");
        assert_eq!(v["firmware_cohort"], "origine_v1");
        assert_eq!(v["metadata_format_version"], 3);
        assert_eq!(v["elapsed_ms"], 42);
    }

    #[test]
    fn event_device_detected_supported_omits_version_key_when_profile_has_none() {
        // A FLAM entry OMITS the version key entirely — never `null`,
        // never an invented `0`.
        let event = Event::DeviceDetectedSupported {
            device_identifier: "abc".into(),
            firmware_cohort: "flam_gen1",
            metadata_format_version: None,
            elapsed_ms: 42,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_detected_supported");
        assert_eq!(v["firmware_cohort"], "flam_gen1");
        assert!(v
            .as_object()
            .expect("object")
            .get("metadata_format_version")
            .is_none());
    }

    #[test]
    fn event_device_detected_unsupported_carries_typed_reason() {
        let event = Event::DeviceDetectedUnsupported {
            reason: "metadata_unsupported",
            firmware_hint: Some("metadata_v99".into()),
            elapsed_ms: 17,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_detected_unsupported");
        assert_eq!(v["reason"], "metadata_unsupported");
        assert_eq!(v["firmware_hint"], "metadata_v99");
        assert_eq!(v["elapsed_ms"], 17);
    }

    #[test]
    fn event_device_scan_failed_carries_typed_source_and_kind() {
        let event = Event::DeviceScanFailed {
            source: "fs_read",
            kind: Some("permission_denied".to_string()),
            elapsed_ms: 5,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_scan_failed");
        assert_eq!(v["source"], "fs_read");
        assert_eq!(v["kind"], "permission_denied");
        assert_eq!(v["elapsed_ms"], 5);
    }

    #[test]
    fn record_event_appends_one_line() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("diagnostics").join("device.jsonl");
        record_event_at_path(&path, Event::DeviceAbsent { elapsed_ms: 17 }).expect("record");
        let lines = read_lines(&path);
        assert_eq!(lines.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(&lines[0]).expect("parsable");
        assert_eq!(parsed["event"]["category"], "device_absent");
        assert!(parsed["ts"].is_string());
    }

    #[test]
    fn record_event_creates_diagnostics_dir_if_missing() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("diagnostics").join("device.jsonl");
        assert!(!path.parent().unwrap().exists());
        record_event_at_path(
            &path,
            Event::DeviceDetectedSupported {
                device_identifier: "id".into(),
                firmware_cohort: "v3",
                metadata_format_version: Some(7),
                elapsed_ms: 0,
            },
        )
        .expect("record");
        assert!(path.parent().unwrap().exists());
        assert!(path.exists());
    }

    #[test]
    fn log_path_for_resolves_under_diagnostics_subdir() {
        let p = Path::new("/tmp/app");
        let resolved = log_path_for(p);
        assert_eq!(resolved, Path::new("/tmp/app/diagnostics/device.jsonl"));
    }

    #[test]
    fn record_event_does_not_leak_pi_payload_in_serialized_event() {
        let event = Event::DeviceDetectedSupported {
            device_identifier: "OPAQUE_HASH".into(),
            firmware_cohort: "origine_v1",
            metadata_format_version: Some(3),
            elapsed_ms: 0,
        };
        let line = serde_json::to_string(&event).expect("ser");
        // The serialized form must not contain the literal substring
        // ".pi" (a marker that would only show up if a path leaked).
        assert!(!line.contains("/.pi"));
        // The serialized form must not contain "PI_PAYLOAD" or a raw
        // hardware serial — only the hashed identifier.
        assert!(!line.contains("PI_PAYLOAD"));
    }

    #[test]
    fn event_device_library_read_carries_counts_not_raw_uuids() {
        let event = Event::DeviceLibraryRead {
            device_identifier: "0123456789abcdef0123456789abcdef".into(),
            family: "lunii",
            firmware_cohort: "origine_v1",
            story_count: 7,
            hidden_count: 1,
            elapsed_ms: 120,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_library_read");
        assert_eq!(v["device_identifier"], "0123456789abcdef0123456789abcdef");
        assert_eq!(v["family"], "lunii");
        assert_eq!(v["firmware_cohort"], "origine_v1");
        assert_eq!(v["story_count"], 7);
        assert_eq!(v["hidden_count"], 1);
        assert_eq!(v["elapsed_ms"], 120);
        // The payload exposes only counts — no array of pack UUIDs.
        assert!(v.get("uuids").is_none());
        assert!(v.get("stories").is_none());
    }

    #[test]
    fn event_device_library_read_flam_carries_family_tags_without_version() {
        // The FLAM read entry names its family/cohort and never carries
        // a metadata version (the field does not exist on this event —
        // nothing is invented, nothing is null).
        let event = Event::DeviceLibraryRead {
            device_identifier: "fedcba9876543210fedcba9876543210".into(),
            family: "flam",
            firmware_cohort: "flam_gen1",
            story_count: 2,
            hidden_count: 1,
            elapsed_ms: 40,
        };
        let line = serde_json::to_string(&event).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&line).expect("parse");
        assert_eq!(v["family"], "flam");
        assert_eq!(v["firmware_cohort"], "flam_gen1");
        assert!(!line.contains("metadata_format_version"));
        assert!(!line.contains("null"));
    }

    #[test]
    fn event_device_library_read_failed_carries_typed_source_and_kind() {
        let event = Event::DeviceLibraryReadFailed {
            source: "fs_read",
            kind: Some("not_found".into()),
            elapsed_ms: 9,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_library_read_failed");
        assert_eq!(v["source"], "fs_read");
        assert_eq!(v["kind"], "not_found");
        assert_eq!(v["elapsed_ms"], 9);
    }

    #[test]
    fn event_device_story_imported_carries_short_id_never_full_uuid() {
        let event = Event::DeviceStoryImported {
            short_id: "FAC5562D".into(),
            family: "lunii",
            firmware_cohort: "origine_v1",
            story_id: "0197a5d0-0000-7000-8000-000000000000".into(),
            elapsed_ms: 1200,
            bytes_copied: 7168,
            file_count: 8,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_story_imported");
        assert_eq!(v["short_id"], "FAC5562D");
        assert_eq!(v["family"], "lunii");
        assert_eq!(v["firmware_cohort"], "origine_v1");
        assert_eq!(v["story_id"], "0197a5d0-0000-7000-8000-000000000000");
        assert_eq!(v["elapsed_ms"], 1200);
        assert_eq!(v["bytes_copied"], 7168);
        assert_eq!(v["file_count"], 8);
        // The payload never carries the pack UUID nor a path.
        assert!(v.get("pack_uuid").is_none());
        assert!(v.get("uuid").is_none());
        assert!(v.get("path").is_none());
    }

    #[test]
    fn event_device_story_imported_flam_carries_family_tags_without_version() {
        let event = Event::DeviceStoryImported {
            short_id: "55667788".into(),
            family: "flam",
            firmware_cohort: "flam_gen1",
            story_id: "0197a5d0-0000-7000-8000-000000000001".into(),
            elapsed_ms: 900,
            bytes_copied: 448,
            file_count: 3,
        };
        let line = serde_json::to_string(&event).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&line).expect("parse");
        assert_eq!(v["family"], "flam");
        assert_eq!(v["firmware_cohort"], "flam_gen1");
        assert!(!line.contains("metadata_format_version"));
        assert!(!line.contains("null"));
    }

    #[test]
    fn event_device_story_import_failed_carries_typed_source_and_kind() {
        let event = Event::DeviceStoryImportFailed {
            source: "staging_write",
            kind: Some("no_space".into()),
            elapsed_ms: 42,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_story_import_failed");
        assert_eq!(v["source"], "staging_write");
        assert_eq!(v["kind"], "no_space");
        assert_eq!(v["elapsed_ms"], 42);
    }

    #[test]
    fn event_device_pack_sent_carries_counts_never_the_uuid_nor_a_path() {
        let event = Event::DevicePackSent {
            family: "lunii",
            firmware_cohort: "v3",
            image_count: 117,
            audio_count: 223,
            elapsed_ms: 5400,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_pack_sent");
        assert_eq!(v["family"], "lunii");
        assert_eq!(v["firmware_cohort"], "v3");
        assert_eq!(v["image_count"], 117);
        assert_eq!(v["audio_count"], 223);
        assert_eq!(v["elapsed_ms"], 5400);
        assert!(v.get("pack_uuid").is_none());
        assert!(v.get("path").is_none());
    }

    #[test]
    fn event_device_pack_send_failed_carries_typed_source() {
        let event = Event::DevicePackSendFailed {
            source: "capability_gate",
            elapsed_ms: 12,
        };
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "device_pack_send_failed");
        assert_eq!(v["source"], "capability_gate");
        assert_eq!(v["elapsed_ms"], 12);
    }

    #[test]
    fn record_event_rotates_when_file_exceeds_cap() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("diagnostics").join("device.jsonl");
        fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        fs::write(&path, vec![b'x'; (MAX_DEVICE_LOG_BYTES + 1) as usize]).expect("seed huge file");

        record_event_at_path(&path, Event::DeviceAbsent { elapsed_ms: 0 }).expect("record");

        let lines = read_lines(&path);
        assert_eq!(lines.len(), 1);
        let archived: Vec<_> = fs::read_dir(path.parent().unwrap())
            .expect("readdir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl.archived"))
            .collect();
        assert_eq!(archived.len(), 1);
    }
}
