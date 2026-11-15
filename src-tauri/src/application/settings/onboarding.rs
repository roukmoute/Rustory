//! Installation-level acknowledgements for the welcome and device-copy guides.
//! Missing keys mean the guide has not been acknowledged, including on an
//! existing installation. Reading never creates a preference on the user's behalf.

use crate::application::story::now_iso_ms;
use crate::domain::shared::AppError;
use crate::infrastructure::db::DbHandle;

const WELCOME_COMPLETED_KEY: &str = "onboarding_welcome_completed";
const DEVICE_RECOVERY_PROMPT_DISMISSED_KEY: &str = "onboarding_device_recovery_prompt_dismissed";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OnboardingPreferences {
    pub welcome_completed: bool,
    pub device_recovery_prompt_dismissed: bool,
}

pub fn get_onboarding_preferences(db: &DbHandle) -> Result<OnboardingPreferences, AppError> {
    // One statement reads a coherent snapshot of both flags.
    let (welcome, recovery): (Option<String>, Option<String>) = db
        .conn()
        .query_row(
            "SELECT (SELECT value FROM app_settings WHERE key = ?1), \
                    (SELECT value FROM app_settings WHERE key = ?2)",
            rusqlite::params![WELCOME_COMPLETED_KEY, DEVICE_RECOVERY_PROMPT_DISMISSED_KEY],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| storage_error("read"))?;
    Ok(OnboardingPreferences {
        welcome_completed: parse_flag(welcome.as_deref())?,
        device_recovery_prompt_dismissed: parse_flag(recovery.as_deref())?,
    })
}

/// Replace both flags explicitly. One SQLite statement commits both or neither;
/// unrelated settings (including the selected voice) are never touched.
pub fn update_onboarding_preferences(
    db: &DbHandle,
    preferences: OnboardingPreferences,
) -> Result<OnboardingPreferences, AppError> {
    let now = now_iso_ms()?;
    db.conn()
        .execute(
            "INSERT INTO app_settings (key, value, updated_at) \
             VALUES (?1, ?2, ?5), (?3, ?4, ?5) \
             ON CONFLICT(key) DO UPDATE SET \
               value = excluded.value, updated_at = excluded.updated_at",
            rusqlite::params![
                WELCOME_COMPLETED_KEY,
                flag_value(preferences.welcome_completed),
                DEVICE_RECOVERY_PROMPT_DISMISSED_KEY,
                flag_value(preferences.device_recovery_prompt_dismissed),
                now,
            ],
        )
        .map_err(|_| storage_error("write"))?;
    Ok(preferences)
}

fn flag_value(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn parse_flag(value: Option<&str>) -> Result<bool, AppError> {
    match value {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err(AppError::library_inconsistent(
            "Rustory n'a pas pu lire tes préférences d'accueil.",
            "Relance l'application ; si le problème persiste, consulte les traces locales.",
        )
        .with_details(serde_json::json!({
            "source": "onboarding_preferences", "cause": "invalid_value"
        }))),
    }
}

fn storage_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu accéder à tes préférences d'accueil.",
        "Réessaie ; si le problème persiste, relance l'application.",
    )
    .with_details(serde_json::json!({ "source": "onboarding_preferences", "stage": stage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::settings::{read_setting, write_setting, ANNOUNCEMENT_VOICE_KEY};
    use crate::infrastructure::db::{open_at, open_in_memory, run_migrations};

    fn fresh_db() -> DbHandle {
        let mut db = open_in_memory().expect("open");
        run_migrations(&mut db).expect("migrate");
        db
    }

    #[test]
    fn missing_flags_are_false_without_writing_on_an_existing_installation() {
        let db = fresh_db();
        write_setting(&db, ANNOUNCEMENT_VOICE_KEY, Some("system:voice")).unwrap();
        assert_eq!(
            get_onboarding_preferences(&db).unwrap(),
            OnboardingPreferences::default()
        );
        let count: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM app_settings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "reading does not acknowledge a guide");
    }

    #[test]
    fn flags_are_independent_and_updates_preserve_other_settings() {
        let db = fresh_db();
        write_setting(&db, ANNOUNCEMENT_VOICE_KEY, Some("system:voice")).unwrap();
        for preferences in [
            OnboardingPreferences {
                welcome_completed: true,
                device_recovery_prompt_dismissed: false,
            },
            OnboardingPreferences {
                welcome_completed: true,
                device_recovery_prompt_dismissed: true,
            },
            OnboardingPreferences {
                welcome_completed: false,
                device_recovery_prompt_dismissed: true,
            },
            OnboardingPreferences::default(),
        ] {
            assert_eq!(
                update_onboarding_preferences(&db, preferences).unwrap(),
                preferences
            );
            assert_eq!(get_onboarding_preferences(&db).unwrap(), preferences);
        }
        assert_eq!(
            read_setting(&db, ANNOUNCEMENT_VOICE_KEY)
                .unwrap()
                .as_deref(),
            Some("system:voice")
        );
    }

    #[test]
    fn preferences_survive_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rustory.sqlite");
        let preferences = OnboardingPreferences {
            welcome_completed: true,
            device_recovery_prompt_dismissed: true,
        };
        {
            let mut db = open_at(&path).unwrap();
            run_migrations(&mut db).unwrap();
            update_onboarding_preferences(&db, preferences).unwrap();
        }
        let db = open_at(&path).unwrap();
        assert_eq!(get_onboarding_preferences(&db).unwrap(), preferences);
    }

    #[test]
    fn a_missing_sibling_does_not_reset_a_stored_flag() {
        let db = fresh_db();
        write_setting(&db, WELCOME_COMPLETED_KEY, Some("true")).unwrap();
        assert_eq!(
            get_onboarding_preferences(&db).unwrap(),
            OnboardingPreferences {
                welcome_completed: true,
                device_recovery_prompt_dismissed: false,
            }
        );
    }

    #[test]
    fn malformed_stored_flags_are_not_silently_treated_as_completed() {
        let db = fresh_db();
        for key in [WELCOME_COMPLETED_KEY, DEVICE_RECOVERY_PROMPT_DISMISSED_KEY] {
            write_setting(&db, key, Some("yes")).unwrap();
            let value = serde_json::to_value(get_onboarding_preferences(&db).unwrap_err()).unwrap();
            assert_eq!(value["details"]["cause"], "invalid_value");
            write_setting(&db, key, None).unwrap();
        }
    }

    #[test]
    fn a_failed_second_write_rolls_back_the_whole_update() {
        let db = fresh_db();
        update_onboarding_preferences(&db, OnboardingPreferences::default()).unwrap();
        db.conn()
            .execute_batch(
                "CREATE TRIGGER refuse_recovery_preference BEFORE UPDATE ON app_settings \
             WHEN NEW.key = 'onboarding_device_recovery_prompt_dismissed' \
             BEGIN SELECT RAISE(ABORT, 'test refusal'); END;",
            )
            .unwrap();
        let result = update_onboarding_preferences(
            &db,
            OnboardingPreferences {
                welcome_completed: true,
                device_recovery_prompt_dismissed: true,
            },
        );
        let error = serde_json::to_value(result.unwrap_err()).unwrap();
        assert_eq!(error["details"]["stage"], "write");
        assert_eq!(
            get_onboarding_preferences(&db).unwrap(),
            OnboardingPreferences::default()
        );
    }
}
