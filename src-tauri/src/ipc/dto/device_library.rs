use std::collections::HashSet;

use serde::Serialize;

use crate::application::device::library::DeviceLibraryOutcome;
use crate::domain::device::DeviceStoryEntry;

use super::device::{reason_dto, UnsupportedReasonDto};

/// Wire shape returned by the `read_device_library` Tauri command.
///
/// Tagged enum on `kind`: `"none"`, `"unsupported"`, `"readable"`. All
/// field names are camelCase. The frontend mirror lives at
/// `src/shared/ipc-contracts/device-library.ts` — drift is enforced by
/// the contract tests in `src-tauri/tests/contracts/device_library.rs`
/// AND the runtime guard `isDeviceLibraryDto`.
///
/// Scope reminder: the device exposes only opaque pack identifiers, so a
/// `DeviceStoryDto` carries NO title — the UI renders each as an
/// unrecognized device-resident story. The OS mount path is never part
/// of this DTO.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DeviceLibraryDto {
    None,
    #[serde(rename_all = "camelCase")]
    Unsupported {
        reason: UnsupportedReasonDto,
        firmware_hint: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Readable {
        device_identifier: String,
        stories: Vec<DeviceStoryDto>,
    },
}

/// One device-resident story as surfaced for listing. Opaque identity +
/// structural flags only — never an asserted title or cover.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStoryDto {
    /// Canonical lowercase pack UUID (public content identifier).
    pub uuid: String,
    /// Uppercase last 8 hex characters — the opaque label shown to the
    /// user and the `.content` folder name.
    pub short_id: String,
    /// Listed in `.pi.hidden` rather than `.pi`.
    pub hidden: bool,
    /// A `.content/<shortId>` payload folder exists; `false` flags an
    /// orphan/ambiguous entry.
    pub content_present: bool,
    /// A `story_imports` provenance row links this pack UUID to a local
    /// story. Stamped by RUST (local truth + device truth composed at
    /// the boundary) — the frontend never recomposes it. Keyed on the
    /// pack UUID: the same pack seen from another device is equally
    /// "déjà dans ta bibliothèque".
    pub already_imported: bool,
}

impl DeviceLibraryDto {
    /// Map the application outcome to the wire shape, stamping each
    /// entry's `alreadyImported` from the set of imported pack UUIDs the
    /// command read under a scoped DB lock BEFORE the device I/O.
    pub fn from_outcome(outcome: DeviceLibraryOutcome, imported_uuids: &HashSet<String>) -> Self {
        match outcome {
            DeviceLibraryOutcome::None => Self::None,
            DeviceLibraryOutcome::Unsupported {
                reason,
                firmware_hint,
            } => Self::Unsupported {
                reason: reason_dto(reason),
                firmware_hint,
            },
            DeviceLibraryOutcome::Readable {
                device_identifier,
                library,
            } => Self::Readable {
                device_identifier,
                stories: library
                    .entries
                    .into_iter()
                    .map(|entry| story_dto(entry, imported_uuids))
                    .collect(),
            },
        }
    }
}

fn story_dto(entry: DeviceStoryEntry, imported_uuids: &HashSet<String>) -> DeviceStoryDto {
    let already_imported = imported_uuids.contains(&entry.uuid);
    DeviceStoryDto {
        uuid: entry.uuid,
        short_id: entry.short_id,
        hidden: entry.hidden,
        content_present: entry.content_present,
        already_imported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::{DeviceLibrary, DeviceStoryEntry, UnsupportedReason};
    use serde_json::json;

    fn entry(short: &str, hidden: bool, present: bool) -> DeviceStoryEntry {
        DeviceStoryEntry {
            uuid: format!("00000000-0000-0000-0000-0000{short}"),
            short_id: short.to_string(),
            hidden,
            content_present: present,
        }
    }

    fn no_imports() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn none_variant_serializes_with_single_kind_key() {
        let v = serde_json::to_value(DeviceLibraryDto::None).expect("ser");
        assert_eq!(v, json!({ "kind": "none" }));
        assert_eq!(v.as_object().expect("obj").len(), 1);
    }

    #[test]
    fn readable_variant_round_trips_with_camel_case_fields() {
        let dto = DeviceLibraryDto::from_outcome(
            DeviceLibraryOutcome::Readable {
                device_identifier: "0123456789abcdef0123456789abcdef".into(),
                library: DeviceLibrary {
                    entries: vec![
                        entry("0000ABCD", false, true),
                        entry("0000BEEF", true, false),
                    ],
                    had_trailing_bytes: false,
                },
            },
            &no_imports(),
        );
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "readable");
        assert_eq!(v["deviceIdentifier"], "0123456789abcdef0123456789abcdef");
        assert!(v["deviceIdentifier"].is_string());
        assert_eq!(v["stories"][0]["shortId"], "0000ABCD");
        assert_eq!(v["stories"][0]["hidden"], false);
        assert_eq!(v["stories"][0]["contentPresent"], true);
        assert_eq!(v["stories"][0]["alreadyImported"], false);
        assert_eq!(v["stories"][1]["hidden"], true);
        assert_eq!(v["stories"][1]["contentPresent"], false);
        // No snake_case leak.
        assert!(v["stories"][0].get("short_id").is_none());
        assert!(v["stories"][0].get("content_present").is_none());
        assert!(v["stories"][0].get("already_imported").is_none());
    }

    #[test]
    fn readable_variant_stamps_already_imported_from_the_provenance_set() {
        let imported: HashSet<String> = [entry("0000ABCD", false, true).uuid].into();
        let dto = DeviceLibraryDto::from_outcome(
            DeviceLibraryOutcome::Readable {
                device_identifier: "0123456789abcdef0123456789abcdef".into(),
                library: DeviceLibrary {
                    entries: vec![
                        entry("0000ABCD", false, true),
                        entry("0000BEEF", false, true),
                    ],
                    had_trailing_bytes: false,
                },
            },
            &imported,
        );
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["stories"][0]["alreadyImported"], true);
        assert_eq!(v["stories"][1]["alreadyImported"], false);
    }

    #[test]
    fn unsupported_variant_serializes_typed_reason() {
        let dto = DeviceLibraryDto::from_outcome(
            DeviceLibraryOutcome::Unsupported {
                reason: UnsupportedReason::MultipleCandidates,
                firmware_hint: Some("count_2".into()),
            },
            &no_imports(),
        );
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "unsupported");
        assert_eq!(v["reason"], "multipleCandidates");
        assert_eq!(v["firmwareHint"], "count_2");
    }

    #[test]
    fn readable_empty_library_serializes_empty_stories_array() {
        let dto = DeviceLibraryDto::from_outcome(
            DeviceLibraryOutcome::Readable {
                device_identifier: "ffffffffffffffffffffffffffffffff".into(),
                library: DeviceLibrary::default(),
            },
            &no_imports(),
        );
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "readable");
        assert_eq!(v["stories"], json!([]));
    }
}
