//! User-level application settings: a flat key/value ledger (`app_settings`),
//! one row per setting. The first setting is the announcement voice — the
//! voice id (`system:…` / `embedded:…`) the story announcements are
//! generated with. Absent = "not chosen yet": the caller falls back to the
//! first available French voice, never to a guess persisted on the user's
//! behalf.

use rusqlite::OptionalExtension;

use crate::application::story::now_iso_ms;
use crate::domain::shared::AppError;
use crate::infrastructure::db::DbHandle;

/// The announcement voice id setting.
pub const ANNOUNCEMENT_VOICE_KEY: &str = "announcement_voice_id";

/// Upper bound on a stored value (a voice id is a short string).
const MAX_VALUE_CHARS: usize = 512;

pub fn read_setting(db: &DbHandle, key: &str) -> Result<Option<String>, AppError> {
    db.conn()
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            rusqlite::params![key],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| storage_error("read"))
}

/// Write (or clear, with `None`) a setting.
pub fn write_setting(db: &DbHandle, key: &str, value: Option<&str>) -> Result<(), AppError> {
    match value {
        Some(value) => {
            if value.is_empty() || value.chars().count() > MAX_VALUE_CHARS {
                return Err(invalid_value());
            }
            let now = now_iso_ms()?;
            db.conn()
                .execute(
                    "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    rusqlite::params![key, value, now],
                )
                .map_err(|_| storage_error("write"))?;
        }
        None => {
            db.conn()
                .execute(
                    "DELETE FROM app_settings WHERE key = ?1",
                    rusqlite::params![key],
                )
                .map_err(|_| storage_error("clear"))?;
        }
    }
    Ok(())
}

fn invalid_value() -> AppError {
    AppError::library_inconsistent(
        "Réglage invalide.",
        "Choisis une valeur dans la liste proposée.",
    )
    .with_details(serde_json::json!({ "source": "settings", "cause": "invalid_value" }))
}

fn storage_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu lire tes réglages.",
        "Relance l'application ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "settings", "stage": stage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::{open_in_memory, run_migrations};

    #[test]
    fn a_setting_round_trips_updates_and_clears() {
        let mut db = open_in_memory().unwrap();
        run_migrations(&mut db).unwrap();
        assert_eq!(read_setting(&db, ANNOUNCEMENT_VOICE_KEY).unwrap(), None);
        write_setting(&db, ANNOUNCEMENT_VOICE_KEY, Some("system:say:Thomas")).unwrap();
        assert_eq!(
            read_setting(&db, ANNOUNCEMENT_VOICE_KEY)
                .unwrap()
                .as_deref(),
            Some("system:say:Thomas")
        );
        write_setting(&db, ANNOUNCEMENT_VOICE_KEY, Some("embedded:x")).unwrap();
        assert_eq!(
            read_setting(&db, ANNOUNCEMENT_VOICE_KEY)
                .unwrap()
                .as_deref(),
            Some("embedded:x")
        );
        write_setting(&db, ANNOUNCEMENT_VOICE_KEY, None).unwrap();
        assert_eq!(read_setting(&db, ANNOUNCEMENT_VOICE_KEY).unwrap(), None);
        assert!(write_setting(&db, ANNOUNCEMENT_VOICE_KEY, Some("")).is_err());
        assert!(write_setting(&db, ANNOUNCEMENT_VOICE_KEY, Some(&"x".repeat(600))).is_err());
    }
}
