/** Mirror of src-tauri/src/ipc/dto/onboarding.rs. These acknowledgements are
 * installation-wide UI preferences, not device identities or copy outcomes. */
export interface OnboardingPreferences {
  welcomeCompleted: boolean;
  deviceRecoveryPromptDismissed: boolean;
}

/** The update replaces both flags; no omitted field can silently reset one. */
export type UpdateOnboardingPreferencesInput = OnboardingPreferences;

export function isOnboardingPreferences(
  value: unknown,
): value is OnboardingPreferences {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const candidate = value as Record<string, unknown>;
  const keys = Object.keys(candidate);
  return (
    keys.length === 2 &&
    keys.every(
      (key) =>
        key === "welcomeCompleted" || key === "deviceRecoveryPromptDismissed",
    ) &&
    typeof candidate.welcomeCompleted === "boolean" &&
    typeof candidate.deviceRecoveryPromptDismissed === "boolean"
  );
}

export const isUpdateOnboardingPreferencesInput = isOnboardingPreferences;
