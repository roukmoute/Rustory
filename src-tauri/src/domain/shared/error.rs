use serde::Serialize;

/// Stable, UI-facing error categories.
///
/// The serialized wire format is `SCREAMING_SNAKE_CASE` so the frontend can
/// switch on a stable discriminant instead of parsing free-form strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppErrorCode {
    LocalStorageUnavailable,
    LibraryInconsistent,
    InvalidStoryTitle,
    ExportDestinationUnavailable,
    RecoveryDraftUnavailable,
    DeviceScanFailed,
    DeviceUnsupported,
    ImportFailed,
    PreparationFailed,
    TransferFailed,
    OfficialCatalogUnavailable,
}

/// Normalized application error crossing the IPC boundary.
///
/// Every error surfaced to the UI must carry a stable [`AppErrorCode`], a
/// human-readable message, an optional user-facing next action and optional
/// diagnostic details. The frontend never parses free-form strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: AppErrorCode,
    pub message: String,
    pub user_action: Option<String>,
    pub details: Option<serde_json::Value>,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Stable, human-readable rendering used by OS-level boot diagnostics
        // (setup handler, crash reports). Debug formatting would leak the
        // internal struct layout; serialization keeps the wire shape so
        // support can copy-paste it back into a JSON tool if needed.
        match serde_json::to_string(self) {
            Ok(json) => f.write_str(&json),
            Err(_) => write!(f, "[{:?}] {}", self.code, self.message),
        }
    }
}

impl std::error::Error for AppError {}

impl AppError {
    pub fn local_storage_unavailable(
        message: impl Into<String>,
        user_action: impl Into<String>,
    ) -> Self {
        Self {
            code: AppErrorCode::LocalStorageUnavailable,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    pub fn library_inconsistent(
        message: impl Into<String>,
        user_action: impl Into<String>,
    ) -> Self {
        Self {
            code: AppErrorCode::LibraryInconsistent,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    pub fn invalid_story_title(message: impl Into<String>, user_action: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::InvalidStoryTitle,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    pub fn export_destination_unavailable(
        message: impl Into<String>,
        user_action: impl Into<String>,
    ) -> Self {
        Self {
            code: AppErrorCode::ExportDestinationUnavailable,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    pub fn recovery_draft_unavailable(
        message: impl Into<String>,
        user_action: impl Into<String>,
    ) -> Self {
        Self {
            code: AppErrorCode::RecoveryDraftUnavailable,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    /// Constructed when the device scan transport itself fails (OS enum,
    /// permission, timeout, mount disappeared between enumerate and read,
    /// mutex poisoned). Mapped to `details.source` + `details.kind`
    /// closed sets at the call site.
    pub fn device_scan_failed(message: impl Into<String>, user_action: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::DeviceScanFailed,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    /// Constructed when the device profile or operation is not in the
    /// official allow-list. Used by the capability gate AND by the IPC
    /// layer when surfacing classification failures that must not be
    /// silently retried.
    pub fn device_unsupported(message: impl Into<String>, user_action: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::DeviceUnsupported,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    /// Constructed when a device-story import fails at any stage of the
    /// acquisition pipeline (re-verification, copy, promotion, commit).
    /// The stage is carried by `details.source` from the closed set
    /// documented in `ui-states.md#Device Story Import Contract`.
    pub fn import_failed(message: impl Into<String>, user_action: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::ImportFailed,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    /// Constructed when a story PREPARATION cannot even produce a terminal job
    /// outcome — a TRANSPORT failure such as the local store having no resolvable
    /// home (`app_data_dir` unavailable). A FUNCTIONAL preparation failure
    /// (artifact missing/corrupt, preflight not passing, interruption) is NOT an
    /// `AppError`: it is the terminal `retryable` state of the job, surfaced
    /// through the preparation DTO and the `job:failed` event.
    pub fn preparation_failed(message: impl Into<String>, user_action: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::PreparationFailed,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    /// Constructed when a story TRANSFER cannot even produce a terminal job
    /// outcome — a TRANSPORT failure such as the local store having no resolvable
    /// home (`app_data_dir` unavailable) or the blocking worker being lost. A
    /// FUNCTIONAL transfer failure (write not authorized, device changed, write
    /// rejected, interruption) is NOT an `AppError`: it is the terminal
    /// `retryable` state of the job, surfaced through the transfer DTO and the
    /// `job:failed` event.
    pub fn transfer_failed(message: impl Into<String>, user_action: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::TransferFailed,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    /// Constructed when the EXPLICIT official-catalog action fails: the
    /// network fetch (offline, server error, auth), an imported catalog
    /// file (unreadable, oversize, malformed), or the parse. The specific
    /// stage is carried by `details.source`. Never produced implicitly —
    /// the catalog is only ever touched on a deliberate user action.
    pub fn official_catalog_unavailable(
        message: impl Into<String>,
        user_action: impl Into<String>,
    ) -> Self {
        Self {
            code: AppErrorCode::OfficialCatalogUnavailable,
            message: message.into(),
            user_action: Some(user_action.into()),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_in_screaming_snake_case() {
        let err = AppError::local_storage_unavailable("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "LOCAL_STORAGE_UNAVAILABLE");
    }

    #[test]
    fn serializes_fields_in_camel_case() {
        let err = AppError::local_storage_unavailable("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert!(
            v.get("userAction").is_some(),
            "userAction must be camelCase"
        );
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn details_are_attached_when_provided() {
        let err = AppError::local_storage_unavailable("msg", "action")
            .with_details(serde_json::json!({ "source": "unit-test" }));
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["details"]["source"], "unit-test");
    }

    #[test]
    fn library_inconsistent_serializes_with_stable_code() {
        let err = AppError::library_inconsistent("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "LIBRARY_INCONSISTENT");
    }

    #[test]
    fn invalid_story_title_serializes_with_stable_code() {
        let err = AppError::invalid_story_title("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "INVALID_STORY_TITLE");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
    }

    #[test]
    fn export_destination_unavailable_serializes_with_stable_code() {
        let err = AppError::export_destination_unavailable("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "EXPORT_DESTINATION_UNAVAILABLE");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
    }

    #[test]
    fn recovery_draft_unavailable_serializes_with_stable_code() {
        let err = AppError::recovery_draft_unavailable("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "RECOVERY_DRAFT_UNAVAILABLE");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
    }

    #[test]
    fn recovery_draft_unavailable_serializes_with_camel_case_user_action() {
        let err = AppError::recovery_draft_unavailable("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert!(
            v.get("userAction").is_some(),
            "userAction must be camelCase"
        );
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn recovery_draft_unavailable_carries_user_action_and_details() {
        let err = AppError::recovery_draft_unavailable(
            "Récupération indisponible.",
            "Vérifie le disque local et réessaie.",
        )
        .with_details(serde_json::json!({
            "source": "sqlite_upsert",
            "kind": "busy",
        }));
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "RECOVERY_DRAFT_UNAVAILABLE");
        assert_eq!(v["details"]["source"], "sqlite_upsert");
        assert_eq!(v["details"]["kind"], "busy");
    }

    #[test]
    fn export_destination_unavailable_carries_user_action_and_details() {
        let err = AppError::export_destination_unavailable(
            "Écriture refusée par le système.",
            "Choisis un dossier où tu as les droits en écriture.",
        )
        .with_details(serde_json::json!({
            "source": "temp_create",
            "kind": "permission_denied",
        }));
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "EXPORT_DESTINATION_UNAVAILABLE");
        assert_eq!(v["details"]["source"], "temp_create");
        assert_eq!(v["details"]["kind"], "permission_denied");
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn device_scan_failed_serializes_with_stable_code() {
        let err = AppError::device_scan_failed("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
    }

    #[test]
    fn device_unsupported_serializes_with_stable_code() {
        let err = AppError::device_unsupported("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "DEVICE_UNSUPPORTED");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
    }

    #[test]
    fn device_scan_failed_serializes_with_camel_case_user_action() {
        let err = AppError::device_scan_failed("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert!(
            v.get("userAction").is_some(),
            "userAction must be camelCase"
        );
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn device_unsupported_serializes_with_camel_case_user_action() {
        let err = AppError::device_unsupported("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert!(
            v.get("userAction").is_some(),
            "userAction must be camelCase"
        );
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn device_scan_failed_carries_user_action_and_details() {
        let err = AppError::device_scan_failed(
            "Détection indisponible.",
            "Vérifie que la Lunii est branchée et réessaie.",
        )
        .with_details(serde_json::json!({
            "source": "fs_read",
            "kind": "permission_denied",
        }));
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "fs_read");
        assert_eq!(v["details"]["kind"], "permission_denied");
    }

    #[test]
    fn import_failed_serializes_with_stable_code() {
        let err = AppError::import_failed("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
    }

    #[test]
    fn import_failed_serializes_with_camel_case_user_action() {
        let err = AppError::import_failed("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert!(
            v.get("userAction").is_some(),
            "userAction must be camelCase"
        );
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn import_failed_carries_user_action_and_details() {
        let err = AppError::import_failed(
            "Copie impossible: lecture de l'appareil interrompue.",
            "Vérifie la connexion de la Lunii puis réessaie la copie.",
        )
        .with_details(serde_json::json!({
            "source": "fs_read",
            "kind": "not_found",
        }));
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["details"]["source"], "fs_read");
        assert_eq!(v["details"]["kind"], "not_found");
    }

    #[test]
    fn preparation_failed_serializes_with_stable_code() {
        let err = AppError::preparation_failed("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "PREPARATION_FAILED");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn transfer_failed_serializes_with_stable_code() {
        let err = AppError::transfer_failed("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "TRANSFER_FAILED");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn official_catalog_unavailable_serializes_with_stable_code() {
        let err = AppError::official_catalog_unavailable("msg", "action");
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "OFFICIAL_CATALOG_UNAVAILABLE");
        assert_eq!(v["message"], "msg");
        assert_eq!(v["userAction"], "action");
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn official_catalog_unavailable_carries_source_details() {
        let err = AppError::official_catalog_unavailable(
            "Catalogue indisponible.",
            "Réessaie plus tard.",
        )
        .with_details(serde_json::json!({ "source": "network", "stage": "fetch" }));
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "OFFICIAL_CATALOG_UNAVAILABLE");
        assert_eq!(v["details"]["source"], "network");
        assert_eq!(v["details"]["stage"], "fetch");
    }

    #[test]
    fn device_unsupported_carries_user_action_and_details() {
        let err =
            AppError::device_unsupported("Profil non supporté.", "Consulte le profil de support.")
                .with_details(serde_json::json!({
                    "source": "capability_gate",
                    "operation": "write_story",
                }));
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["code"], "DEVICE_UNSUPPORTED");
        assert_eq!(v["details"]["source"], "capability_gate");
        assert_eq!(v["details"]["operation"], "write_story");
    }
}
