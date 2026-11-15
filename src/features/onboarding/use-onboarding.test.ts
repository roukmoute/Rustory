import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../ipc/commands/onboarding", () => ({
  getOnboardingPreferences: vi.fn(),
  updateOnboardingPreferences: vi.fn(),
}));

import {
  getOnboardingPreferences,
  updateOnboardingPreferences,
} from "../../ipc/commands/onboarding";
import type { OnboardingPreferences } from "../../shared/ipc-contracts/onboarding";
import { useOnboarding } from "./use-onboarding";

const untouched: OnboardingPreferences = {
  welcomeCompleted: false,
  deviceRecoveryPromptDismissed: false,
};
const completed: OnboardingPreferences = {
  welcomeCompleted: true,
  deviceRecoveryPromptDismissed: true,
};
const storageError = {
  code: "LOCAL_STORAGE_UNAVAILABLE" as const,
  message: "Préférences indisponibles.",
  userAction: "Réessaie.",
  details: null,
};

function deferred() {
  let resolve!: (value: OnboardingPreferences) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<OnboardingPreferences>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useOnboarding", () => {
  beforeEach(() => {
    vi.mocked(getOnboardingPreferences).mockReset();
    vi.mocked(updateOnboardingPreferences).mockReset();
    vi.mocked(updateOnboardingPreferences).mockImplementation(
      async (input) => input,
    );
  });

  it("does not overwrite stored preferences before or after a failed initial read", async () => {
    const read = deferred();
    vi.mocked(getOnboardingPreferences).mockReturnValueOnce(read.promise);
    const { result } = renderHook(() => useOnboarding());
    await act(async () => {
      expect(await result.current.complete(false)).toBe(false);
      read.reject(storageError);
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBe(storageError);
    expect(result.current.preferences).toBeNull();
    await act(async () => {
      expect(await result.current.complete(false)).toBe(false);
    });
    expect(updateOnboardingPreferences).not.toHaveBeenCalled();

    vi.mocked(getOnboardingPreferences).mockResolvedValueOnce(completed);
    await act(async () => result.current.retry());
    await act(async () => {
      expect(await result.current.complete(false)).toBe(true);
    });
    expect(updateOnboardingPreferences).toHaveBeenCalledExactlyOnceWith(
      completed,
    );
    expect(result.current.error).toBeNull();
  });

  it("ignores a stale StrictMode read after the current guide has been saved", async () => {
    const staleRead = deferred();
    const activeRead = deferred();
    vi.mocked(getOnboardingPreferences)
      .mockReturnValueOnce(staleRead.promise)
      .mockReturnValueOnce(activeRead.promise);
    const { result } = renderHook(() => useOnboarding(), {
      reactStrictMode: true,
    });
    expect(getOnboardingPreferences).toHaveBeenCalledTimes(2);
    await act(async () => activeRead.resolve(untouched));
    await act(async () => {
      expect(await result.current.complete(true)).toBe(true);
    });
    await act(async () => staleRead.resolve(untouched));
    expect(result.current.preferences).toEqual(completed);
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(result.current.saving).toBe(false);
  });

  it("ignores an older read failure after a retry succeeds", async () => {
    const staleRead = deferred();
    vi.mocked(getOnboardingPreferences)
      .mockReturnValueOnce(staleRead.promise)
      .mockResolvedValueOnce(completed);
    const { result } = renderHook(() => useOnboarding());
    await act(async () => result.current.retry());
    await act(async () => staleRead.reject(storageError));
    expect(result.current.preferences).toEqual(completed);
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  it("invalidates previously read flags when a refresh fails", async () => {
    vi.mocked(getOnboardingPreferences)
      .mockResolvedValueOnce(untouched)
      .mockRejectedValueOnce(storageError);
    const { result } = renderHook(() => useOnboarding());
    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => result.current.retry());
    await act(async () => {
      expect(await result.current.complete(true)).toBe(false);
    });
    expect(result.current.preferences).toBeNull();
    expect(updateOnboardingPreferences).not.toHaveBeenCalled();
  });

  it("keeps a failed save visible and allows a successful read and save retry", async () => {
    vi.mocked(getOnboardingPreferences)
      .mockResolvedValueOnce(untouched)
      .mockResolvedValueOnce(completed);
    vi.mocked(updateOnboardingPreferences).mockRejectedValueOnce(storageError);
    const { result } = renderHook(() => useOnboarding());
    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => {
      expect(await result.current.complete(true)).toBe(false);
    });
    expect(result.current.preferences).toEqual(untouched);
    expect(result.current.error).toBe(storageError);
    expect(result.current.saving).toBe(false);

    await act(async () => result.current.retry());
    expect(result.current.error).toBeNull();
    await act(async () => {
      expect(await result.current.complete(false)).toBe(true);
    });
    expect(updateOnboardingPreferences).toHaveBeenLastCalledWith(completed);
    expect(result.current.preferences).toEqual(completed);
  });

  it("ignores concurrent completion and retry while saving without leaving saving stuck", async () => {
    const write = deferred();
    vi.mocked(getOnboardingPreferences).mockResolvedValueOnce(untouched);
    vi.mocked(updateOnboardingPreferences).mockReturnValueOnce(write.promise);
    const { result } = renderHook(() => useOnboarding());
    await waitFor(() => expect(result.current.loading).toBe(false));
    let pending!: Promise<boolean>;
    await act(async () => {
      pending = result.current.complete(true);
      expect(await result.current.complete(false)).toBe(false);
      result.current.retry();
    });
    expect(result.current.saving).toBe(true);
    expect(getOnboardingPreferences).toHaveBeenCalledTimes(1);
    expect(updateOnboardingPreferences).toHaveBeenCalledTimes(1);
    await act(async () => {
      write.resolve(completed);
      expect(await pending).toBe(true);
    });
    expect(result.current.saving).toBe(false);
    expect(result.current.preferences).toEqual(completed);
  });

  it("preserves the latest sibling flag even through a previously captured callback", async () => {
    vi.mocked(getOnboardingPreferences).mockResolvedValueOnce(untouched);
    const { result } = renderHook(() => useOnboarding());
    await waitFor(() => expect(result.current.loading).toBe(false));
    const complete = result.current.complete;
    await act(async () => {
      expect(await complete(true)).toBe(true);
      expect(await complete(false)).toBe(true);
    });
    expect(updateOnboardingPreferences).toHaveBeenNthCalledWith(2, completed);
  });

  it("does not report success after unmounting during a save", async () => {
    const write = deferred();
    vi.mocked(getOnboardingPreferences).mockResolvedValueOnce(untouched);
    vi.mocked(updateOnboardingPreferences).mockReturnValueOnce(write.promise);
    const { result, unmount } = renderHook(() => useOnboarding());
    await waitFor(() => expect(result.current.loading).toBe(false));
    let pending!: Promise<boolean>;
    act(() => {
      pending = result.current.complete(true);
    });
    unmount();
    await act(async () => {
      write.resolve(completed);
      expect(await pending).toBe(false);
    });
    expect(await result.current.complete(false)).toBe(false);
    expect(updateOnboardingPreferences).toHaveBeenCalledTimes(1);
  });
});
