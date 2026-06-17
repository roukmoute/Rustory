use serde::Serialize;

/// Current state of the official-catalog cache. Mirror of
/// `src/shared/ipc-contracts/device-catalog.ts`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatusDto {
    /// Number of official titles currently cached locally.
    pub count: u32,
}

impl CatalogStatusDto {
    pub fn new(count: u32) -> Self {
        Self { count }
    }
}

/// Outcome of `import_official_catalog`. A cancelled file dialog is NOT an
/// error — it resolves with `{ kind: "cancelled" }` so the UI returns to
/// idle silently, mirroring the export flow.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportOfficialCatalogOutcomeDto {
    Cancelled,
    #[serde(rename_all = "camelCase")]
    Imported {
        count: u32,
    },
}

/// One cached cover, returned by `read_pack_cover` as a self-contained
/// `data:` URL the webview can render directly — read from the LOCAL cache,
/// no network. Mirror of `src/shared/ipc-contracts/device-catalog.ts`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackCoverDto {
    pub data_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_in_camel_case() {
        let v = serde_json::to_value(CatalogStatusDto::new(42)).expect("ser");
        assert_eq!(v, serde_json::json!({ "count": 42 }));
    }

    #[test]
    fn import_outcome_cancelled_serializes_with_kind() {
        let v = serde_json::to_value(ImportOfficialCatalogOutcomeDto::Cancelled).expect("ser");
        assert_eq!(v, serde_json::json!({ "kind": "cancelled" }));
    }

    #[test]
    fn import_outcome_imported_serializes_count() {
        let v = serde_json::to_value(ImportOfficialCatalogOutcomeDto::Imported { count: 7 })
            .expect("ser");
        assert_eq!(v, serde_json::json!({ "kind": "imported", "count": 7 }));
    }
}
