//! Symmetric with src/ipc/contract-tests/onboarding.test.ts.
use rustory_lib::application::settings::onboarding::OnboardingPreferences;
use rustory_lib::ipc::dto::{OnboardingPreferencesDto, UpdateOnboardingPreferencesInputDto};

#[test]
fn onboarding_preferences_always_serialize_both_camel_case_booleans() {
    for welcome in [false, true] {
        for recovery in [false, true] {
            let dto = OnboardingPreferencesDto::from(OnboardingPreferences {
                welcome_completed: welcome,
                device_recovery_prompt_dismissed: recovery,
            });
            assert_eq!(
                serde_json::to_value(dto).unwrap(),
                serde_json::json!({
                    "welcomeCompleted": welcome,
                    "deviceRecoveryPromptDismissed": recovery,
                })
            );
        }
    }
}

#[test]
fn onboarding_update_requires_exactly_two_boolean_fields() {
    let input: UpdateOnboardingPreferencesInputDto = serde_json::from_value(serde_json::json!({
        "welcomeCompleted": true,
        "deviceRecoveryPromptDismissed": false,
    }))
    .unwrap();
    let preferences = OnboardingPreferences::from(input);
    assert!(preferences.welcome_completed);
    assert!(!preferences.device_recovery_prompt_dismissed);
    for invalid in [
        serde_json::json!({}),
        serde_json::json!({"welcomeCompleted": true}),
        serde_json::json!({"deviceRecoveryPromptDismissed": true}),
        serde_json::json!({"welcomeCompleted": "true", "deviceRecoveryPromptDismissed": false}),
        serde_json::json!({"welcomeCompleted": true, "deviceRecoveryPromptDismissed": null}),
        serde_json::json!({"welcomeCompleted": true, "deviceRecoveryPromptDismissed": false, "other": false}),
        serde_json::json!({"welcome_completed": true, "device_recovery_prompt_dismissed": false}),
    ] {
        assert!(serde_json::from_value::<UpdateOnboardingPreferencesInputDto>(invalid).is_err());
    }
}
