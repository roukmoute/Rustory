use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::application::device::library::DeviceLibraryOutcome;
use crate::domain::device::title::{PackTitle, PackTitleSource};
use crate::domain::device::DeviceStoryEntry;

use super::device::{reason_dto, UnsupportedReasonDto};

/// Wire token for a recognized title's provenance. camelCase serialization
/// keeps it symmetric with the TS contract; single words mean the tags are
/// `"user"` / `"official"` / `"unofficial"`. Mirrors
/// [`PackTitleSource`](crate::domain::device::title::PackTitleSource).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PackTitleSourceDto {
    User,
    Official,
    Unofficial,
}

impl From<PackTitleSource> for PackTitleSourceDto {
    fn from(source: PackTitleSource) -> Self {
        match source {
            PackTitleSource::User => PackTitleSourceDto::User,
            PackTitleSource::Official => PackTitleSourceDto::Official,
            PackTitleSource::Unofficial => PackTitleSourceDto::Unofficial,
        }
    }
}

/// Wire shape returned by the `read_device_library` Tauri command.
///
/// Tagged enum on `kind`: `"none"`, `"unsupported"`, `"readable"`. All
/// field names are camelCase. The frontend mirror lives at
/// `src/shared/ipc-contracts/device-library.ts` — drift is enforced by
/// the contract tests in `src-tauri/tests/contracts/device_library.rs`
/// AND the runtime guard `isDeviceLibraryDto`.
///
/// Scope reminder: the device stores only opaque pack identifiers; the
/// recognized `title` / `titleSource` / `thumbnail` are composed by RUST
/// from the local index and may be `null` (a genuinely unrecognized pack).
/// The OS mount path is never part of this DTO.
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

/// One device-resident story as surfaced for listing. The device stores no
/// title, so identity + structural flags come from the device while the
/// recognized `title` / `titleSource` / `thumbnail` are composed by RUST
/// from the local index (user names, cached official catalog, local-library
/// link). A `null` title is a genuinely unrecognized pack ("non reconnue");
/// the UI never recomposes the truth.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStoryDto {
    /// Canonical lowercase pack UUID (public content identifier).
    pub uuid: String,
    /// Uppercase last 8 hex characters — the `.content` folder name and the
    /// fallback label shown when the pack is not recognized.
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
    /// The recognized title, or `null` when no index covers this pack.
    pub title: Option<String>,
    /// Provenance of `title`. `null` exactly when `title` is `null`. Lets
    /// the UI show "officiel / non-officiel / saisi" and NEVER present a
    /// user/community title as official.
    pub title_source: Option<PackTitleSourceDto>,
    /// Presence flag for a cached cover: an OPAQUE local cache reference (a
    /// file name), or `null`. NEVER a remote URL — the frontend loads the
    /// image via the `read_pack_cover` command (a local read), never by
    /// rendering this value. `null` for user / local-library titles.
    pub thumbnail: Option<String>,
}

impl DeviceLibraryDto {
    /// Map the application outcome to the wire shape, composing local truth
    /// onto each entry: `alreadyImported` from the set of imported pack
    /// UUIDs, and the recognized `title` / `titleSource` / `thumbnail` from
    /// the resolved-titles map. Both are read under a scoped DB lock around
    /// the device I/O (never held across it) and keyed by pack UUID.
    pub fn from_outcome(
        outcome: DeviceLibraryOutcome,
        imported_uuids: &HashSet<String>,
        titles: &HashMap<String, PackTitle>,
    ) -> Self {
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
                    .map(|entry| story_dto(entry, imported_uuids, titles))
                    .collect(),
            },
        }
    }
}

fn story_dto(
    entry: DeviceStoryEntry,
    imported_uuids: &HashSet<String>,
    titles: &HashMap<String, PackTitle>,
) -> DeviceStoryDto {
    let already_imported = imported_uuids.contains(&entry.uuid);
    let resolved = titles.get(&entry.uuid);
    DeviceStoryDto {
        already_imported,
        title: resolved.map(|t| t.title.clone()),
        title_source: resolved.map(|t| t.source.into()),
        thumbnail: resolved.and_then(|t| t.thumbnail.clone()),
        uuid: entry.uuid,
        short_id: entry.short_id,
        hidden: entry.hidden,
        content_present: entry.content_present,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::title::PackTitle;
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

    fn no_titles() -> HashMap<String, PackTitle> {
        HashMap::new()
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
            &no_titles(),
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
        // Unrecognized packs carry explicit null title fields (stable shape).
        assert!(v["stories"][0]["title"].is_null());
        assert!(v["stories"][0]["titleSource"].is_null());
        assert!(v["stories"][0]["thumbnail"].is_null());
        // No snake_case leak.
        assert!(v["stories"][0].get("short_id").is_none());
        assert!(v["stories"][0].get("content_present").is_none());
        assert!(v["stories"][0].get("already_imported").is_none());
        assert!(v["stories"][0].get("title_source").is_none());
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
            &no_titles(),
        );
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["stories"][0]["alreadyImported"], true);
        assert_eq!(v["stories"][1]["alreadyImported"], false);
    }

    #[test]
    fn readable_variant_stamps_resolved_title_and_provenance() {
        let recognized = entry("0000ABCD", false, true);
        let mut titles = HashMap::new();
        titles.insert(
            recognized.uuid.clone(),
            PackTitle {
                title: "Le Loup".into(),
                source: PackTitleSource::Official,
                thumbnail: Some("cover.png".into()),
            },
        );
        let dto = DeviceLibraryDto::from_outcome(
            DeviceLibraryOutcome::Readable {
                device_identifier: "0123456789abcdef0123456789abcdef".into(),
                library: DeviceLibrary {
                    entries: vec![recognized, entry("0000BEEF", false, true)],
                    had_trailing_bytes: false,
                },
            },
            &no_imports(),
            &titles,
        );
        let v = serde_json::to_value(&dto).expect("ser");
        // Recognized pack: title + camelCase provenance token + cover.
        assert_eq!(v["stories"][0]["title"], "Le Loup");
        assert_eq!(v["stories"][0]["titleSource"], "official");
        assert_eq!(v["stories"][0]["thumbnail"], "cover.png");
        // The other pack stays unrecognized.
        assert!(v["stories"][1]["title"].is_null());
        assert!(v["stories"][1]["titleSource"].is_null());
    }

    #[test]
    fn user_and_unofficial_sources_serialize_to_their_camel_case_tokens() {
        let a = entry("0000AAAA", false, true);
        let b = entry("0000BBBB", false, true);
        let mut titles = HashMap::new();
        titles.insert(
            a.uuid.clone(),
            PackTitle {
                title: "Saisi".into(),
                source: PackTitleSource::User,
                thumbnail: None,
            },
        );
        titles.insert(
            b.uuid.clone(),
            PackTitle {
                title: "Importée".into(),
                source: PackTitleSource::Unofficial,
                thumbnail: None,
            },
        );
        let dto = DeviceLibraryDto::from_outcome(
            DeviceLibraryOutcome::Readable {
                device_identifier: "0123456789abcdef0123456789abcdef".into(),
                library: DeviceLibrary {
                    entries: vec![a, b],
                    had_trailing_bytes: false,
                },
            },
            &no_imports(),
            &titles,
        );
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["stories"][0]["titleSource"], "user");
        assert_eq!(v["stories"][1]["titleSource"], "unofficial");
    }

    #[test]
    fn unsupported_variant_serializes_typed_reason() {
        let dto = DeviceLibraryDto::from_outcome(
            DeviceLibraryOutcome::Unsupported {
                reason: UnsupportedReason::MultipleCandidates,
                firmware_hint: Some("count_2".into()),
            },
            &no_imports(),
            &no_titles(),
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
            &no_titles(),
        );
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(v["kind"], "readable");
        assert_eq!(v["stories"], json!([]));
    }
}
