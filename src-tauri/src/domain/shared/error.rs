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
}
