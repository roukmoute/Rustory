use tauri::State;

use crate::application::settings::onboarding;
use crate::domain::shared::AppError;
use crate::ipc::dto::{OnboardingPreferencesDto, UpdateOnboardingPreferencesInputDto};
use crate::AppState;

#[tauri::command]
pub fn get_onboarding_preferences(
    state: State<'_, AppState>,
) -> Result<OnboardingPreferencesDto, AppError> {
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    onboarding::get_onboarding_preferences(&db).map(Into::into)
}

#[tauri::command]
pub fn update_onboarding_preferences(
    state: State<'_, AppState>,
    input: UpdateOnboardingPreferencesInputDto,
) -> Result<OnboardingPreferencesDto, AppError> {
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    onboarding::update_onboarding_preferences(&db, input.into()).map(Into::into)
}
