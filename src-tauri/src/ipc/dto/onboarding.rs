use serde::{Deserialize, Serialize};

use crate::application::settings::onboarding::OnboardingPreferences;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingPreferencesDto {
    pub welcome_completed: bool,
    pub device_recovery_prompt_dismissed: bool,
}

impl From<OnboardingPreferences> for OnboardingPreferencesDto {
    fn from(preferences: OnboardingPreferences) -> Self {
        Self {
            welcome_completed: preferences.welcome_completed,
            device_recovery_prompt_dismissed: preferences.device_recovery_prompt_dismissed,
        }
    }
}

/// Both flags are required: omission is never interpreted as an implicit reset.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateOnboardingPreferencesInputDto {
    pub welcome_completed: bool,
    pub device_recovery_prompt_dismissed: bool,
}

impl From<UpdateOnboardingPreferencesInputDto> for OnboardingPreferences {
    fn from(input: UpdateOnboardingPreferencesInputDto) -> Self {
        Self {
            welcome_completed: input.welcome_completed,
            device_recovery_prompt_dismissed: input.device_recovery_prompt_dismissed,
        }
    }
}
