use serde::Serialize;

/// Stable, UI-facing error categories.
///
/// The serialized wire format is `SCREAMING_SNAKE_CASE` so the frontend can
/// switch on a stable discriminant instead of parsing free-form strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppErrorCode {
    LocalStorageUnavailable,
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
        assert!(v.get("userAction").is_some(), "userAction must be camelCase");
        assert!(v.get("user_action").is_none(), "snake_case must not leak");
    }

    #[test]
    fn details_are_attached_when_provided() {
        let err = AppError::local_storage_unavailable("msg", "action")
            .with_details(serde_json::json!({ "source": "unit-test" }));
        let v = serde_json::to_value(&err).expect("serialize");
        assert_eq!(v["details"]["source"], "unit-test");
    }
}
