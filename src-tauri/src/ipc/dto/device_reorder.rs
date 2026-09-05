//! Wire contract of `reorder_device_stories` — the device wheel order.
//! Mirrored in `src/shared/ipc-contracts/device-reorder.ts`.

use serde::{Deserialize, Serialize};

use crate::application::device::reorder::ReorderedDeviceStories;

/// The device and the COMPLETE new order of its visible packs (every listed
/// uuid, once, canonical lowercase).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderDeviceStoriesInputDto {
    pub device_identifier: String,
    pub ordered_pack_uuids: Vec<String>,
}

/// Outcome of a settled reorder. Family-neutral. `changed` is `false` when
/// the device already listed that order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderDeviceStoriesOutcomeDto {
    pub count: u32,
    pub changed: bool,
}

impl ReorderDeviceStoriesOutcomeDto {
    pub fn from_outcome(outcome: ReorderedDeviceStories) -> Self {
        Self {
            count: outcome.count as u32,
            changed: outcome.changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_parses_camel_case_and_outcome_serializes_family_neutral() {
        let input: ReorderDeviceStoriesInputDto = serde_json::from_str(
            r#"{"deviceIdentifier":"0123456789abcdef0123456789abcdef","orderedPackUuids":["a","b"]}"#,
        )
        .unwrap();
        assert_eq!(input.ordered_pack_uuids, vec!["a", "b"]);
        let out = ReorderDeviceStoriesOutcomeDto {
            count: 2,
            changed: true,
        };
        assert_eq!(
            serde_json::to_value(&out).unwrap(),
            serde_json::json!({ "count": 2, "changed": true })
        );
    }
}
