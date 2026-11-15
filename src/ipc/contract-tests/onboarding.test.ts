import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { invoke } from "@tauri-apps/api/core";
import {
  isOnboardingPreferences,
  isUpdateOnboardingPreferencesInput,
  type OnboardingPreferences,
} from "../../shared/ipc-contracts/onboarding";
import {
  getOnboardingPreferences,
  updateOnboardingPreferences,
  OnboardingPreferencesContractDriftError,
} from "../commands/onboarding";

describe("onboarding IPC contract", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("accepts the four Rust boolean combinations, including explicit false", () => {
    for (const welcomeCompleted of [false, true]) {
      for (const deviceRecoveryPromptDismissed of [false, true]) {
        const payload = { welcomeCompleted, deviceRecoveryPromptDismissed };
        expect(isOnboardingPreferences(payload)).toBe(true);
        expect(isUpdateOnboardingPreferencesInput(payload)).toBe(true);
      }
    }
  });

  it.each([
    null, [], {},
    { welcomeCompleted: true },
    { deviceRecoveryPromptDismissed: true },
    { welcomeCompleted: "true", deviceRecoveryPromptDismissed: false },
    { welcomeCompleted: true, deviceRecoveryPromptDismissed: null },
    { welcomeCompleted: true, deviceRecoveryPromptDismissed: false, other: false },
    { welcome_completed: true, device_recovery_prompt_dismissed: false },
  ].map((payload) => ({ payload })))("rejects an incomplete or drifted payload: $payload", ({ payload }) => {
    expect(isOnboardingPreferences(payload)).toBe(false);
    expect(isUpdateOnboardingPreferencesInput(payload)).toBe(false);
  });

  it("reads the preferences without arguments or an implicit write", async () => {
    const payload = { welcomeCompleted: false, deviceRecoveryPromptDismissed: false };
    vi.mocked(invoke).mockResolvedValueOnce(payload);
    expect(await getOnboardingPreferences()).toEqual(payload);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("get_onboarding_preferences");
  });

  it("sends both explicit flags and returns the stored result", async () => {
    const input = { welcomeCompleted: true, deviceRecoveryPromptDismissed: false };
    vi.mocked(invoke).mockResolvedValueOnce(input);
    expect(await updateOnboardingPreferences(input)).toEqual(input);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("update_onboarding_preferences", { input });
  });

  it("rejects a malformed update before invoking Rust", async () => {
    await expect(updateOnboardingPreferences({ welcomeCompleted: true } as OnboardingPreferences))
      .rejects.toBeInstanceOf(TypeError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it.each(["get", "update"])("refuses drifted %s responses without invented flags", async (command) => {
    const drift = { welcomeCompleted: true };
    vi.mocked(invoke).mockResolvedValueOnce(drift);
    const call = command === "get" ? getOnboardingPreferences() : updateOnboardingPreferences({
      welcomeCompleted: true, deviceRecoveryPromptDismissed: true,
    });
    const error: unknown = await call.catch((failure: unknown) => failure);
    expect(error).toBeInstanceOf(OnboardingPreferencesContractDriftError);
    expect(error).toMatchObject({ raw: drift });
  });

  it("preserves Rust storage errors and normalizes transport failures", async () => {
    const failure = { code: "LOCAL_STORAGE_UNAVAILABLE", message: "Indisponible.", userAction: "Réessaie.", details: { source: "onboarding_preferences", stage: "write" } };
    vi.mocked(invoke).mockRejectedValueOnce(failure);
    await expect(updateOnboardingPreferences({ welcomeCompleted: true, deviceRecoveryPromptDismissed: false }))
      .rejects.toBe(failure);
    vi.mocked(invoke).mockRejectedValueOnce(new Error("transport"));
    await expect(getOnboardingPreferences()).rejects.toMatchObject({ code: "UNKNOWN" });
  });
});
