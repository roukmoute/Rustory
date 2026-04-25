use crate::domain::shared::AppError;

/// Bound the accepted story id length so a hostile or malformed payload
/// can never issue a multi-kilobyte SQL prepared-statement parameter.
/// UUIDv7 canonical form is 36 bytes; a generous ceiling leaves room for
/// future id schemes without becoming an attack surface.
pub const MAX_STORY_ID_LEN: usize = 256;

/// Validate an incoming story id at the IPC boundary, surfacing the same
/// `LIBRARY_INCONSISTENT` category regardless of which command the id
/// feeds — the UI recovers the same way in every case ("reload the
/// library"). Kept as a single source of truth so story-id validation
/// never drifts between commands.
pub fn validate_story_id(raw: &str) -> Result<(), AppError> {
    if raw.is_empty() {
        return Err(AppError::library_inconsistent(
            "Histoire introuvable, recharge la bibliothèque.",
            "Retourne à la bibliothèque et recharge la liste.",
        )
        .with_details(serde_json::json!({
            "source": "story_id_invalid",
            "cause": "empty",
        })));
    }
    if raw.len() > MAX_STORY_ID_LEN {
        return Err(AppError::library_inconsistent(
            "Histoire introuvable, recharge la bibliothèque.",
            "Retourne à la bibliothèque et recharge la liste.",
        )
        .with_details(serde_json::json!({
            "source": "story_id_invalid",
            "cause": "too_long",
            "maxLen": MAX_STORY_ID_LEN,
        })));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;

    #[test]
    fn accepts_a_canonical_uuid_v7() {
        assert!(validate_story_id("0197a5d0-0000-7000-8000-000000000000").is_ok());
    }

    #[test]
    fn rejects_empty_string() {
        let err = validate_story_id("").expect_err("must reject empty");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "story_id_invalid");
        assert_eq!(details["cause"], "empty");
    }

    #[test]
    fn rejects_oversize_string() {
        let huge = "a".repeat(MAX_STORY_ID_LEN + 1);
        let err = validate_story_id(&huge).expect_err("must reject oversize");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["cause"], "too_long");
        assert_eq!(details["maxLen"], MAX_STORY_ID_LEN);
    }
}
