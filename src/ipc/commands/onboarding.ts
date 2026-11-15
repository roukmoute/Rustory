import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import {
  isOnboardingPreferences,
  isUpdateOnboardingPreferencesInput,
  type OnboardingPreferences,
  type UpdateOnboardingPreferencesInput,
} from "../../shared/ipc-contracts/onboarding";

export class OnboardingPreferencesContractDriftError extends Error {
  readonly raw: unknown;
  constructor(command: string, raw: unknown) {
    super(`${command} returned a payload that does not match the contract`);
    this.name = "OnboardingPreferencesContractDriftError";
    this.raw = raw;
  }
}

/** Read only: absent persisted choices are false, including on older installs. */
export async function getOnboardingPreferences(): Promise<OnboardingPreferences> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("get_onboarding_preferences");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isOnboardingPreferences(raw)) {
    throw new OnboardingPreferencesContractDriftError(
      "get_onboarding_preferences",
      raw,
    );
  }
  return raw;
}

/** Persist the two explicit flags atomically; never treat an IPC failure as saved. */
export async function updateOnboardingPreferences(
  input: UpdateOnboardingPreferencesInput,
): Promise<OnboardingPreferences> {
  if (!isUpdateOnboardingPreferencesInput(input)) {
    throw new TypeError(
      "update_onboarding_preferences requires exactly two boolean flags",
    );
  }
  let raw: unknown;
  try {
    raw = await invoke<unknown>("update_onboarding_preferences", { input });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isOnboardingPreferences(raw)) {
    throw new OnboardingPreferencesContractDriftError(
      "update_onboarding_preferences",
      raw,
    );
  }
  return raw;
}
