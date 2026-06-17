import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-title", () => ({
  setDeviceStoryTitle: vi.fn(),
}));

import { setDeviceStoryTitle } from "../../../ipc/commands/device-title";
import { useDeviceStoryTitle } from "./use-device-story-title";

const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

const RUST_TITLE_ERROR = {
  code: "INVALID_STORY_TITLE" as const,
  message: "Création impossible: titre trop long (120 caractères maximum).",
  userAction: "Raccourcis le titre à 120 caractères maximum.",
  details: null,
};

describe("useDeviceStoryTitle", () => {
  beforeEach(() => {
    vi.mocked(setDeviceStoryTitle).mockReset();
  });

  it("starts in idle with no target", () => {
    const { result } = renderHook(() => useDeviceStoryTitle());
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.targetPackUuid).toBeNull();
  });

  it("commits a title, returns true, and reports to the route", async () => {
    vi.mocked(setDeviceStoryTitle).mockResolvedValueOnce({
      title: "Mon histoire",
      source: "user",
    });
    const onTitled = vi.fn();
    const { result } = renderHook(() => useDeviceStoryTitle({ onTitled }));

    let committed: boolean | undefined;
    await act(async () => {
      committed = await result.current.setTitle(PACK_UUID, "Mon histoire");
    });

    expect(committed).toBe(true);
    expect(setDeviceStoryTitle).toHaveBeenCalledWith({
      packUuid: PACK_UUID,
      title: "Mon histoire",
    });
    expect(onTitled).toHaveBeenCalledWith(PACK_UUID, {
      title: "Mon histoire",
      source: "user",
    });
    // Settles back to idle on success.
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.targetPackUuid).toBeNull();
  });

  it("surfaces a failure (false) and exposes the error + target without calling onTitled", async () => {
    vi.mocked(setDeviceStoryTitle).mockRejectedValueOnce(RUST_TITLE_ERROR);
    const onTitled = vi.fn();
    const { result } = renderHook(() => useDeviceStoryTitle({ onTitled }));

    let committed: boolean | undefined;
    await act(async () => {
      committed = await result.current.setTitle(PACK_UUID, "x".repeat(200));
    });

    expect(committed).toBe(false);
    expect(onTitled).not.toHaveBeenCalled();
    expect(result.current.status).toEqual({
      kind: "failed",
      error: RUST_TITLE_ERROR,
    });
    // The target stays attached to the failing card so the route can scope
    // the error to it.
    expect(result.current.targetPackUuid).toBe(PACK_UUID);
  });

  it("reset() clears a failure back to idle", async () => {
    vi.mocked(setDeviceStoryTitle).mockRejectedValueOnce(RUST_TITLE_ERROR);
    const { result } = renderHook(() => useDeviceStoryTitle());
    await act(async () => {
      await result.current.setTitle(PACK_UUID, "x");
    });
    expect(result.current.status.kind).toBe("failed");
    act(() => {
      result.current.reset();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.targetPackUuid).toBeNull();
  });

  it("does not call onTitled after the hook unmounts mid-write", async () => {
    let resolve!: (v: { title: string; source: "user" }) => void;
    vi.mocked(setDeviceStoryTitle).mockReturnValueOnce(
      new Promise((r) => {
        resolve = r;
      }),
    );
    const onTitled = vi.fn();
    const { result, unmount } = renderHook(() =>
      useDeviceStoryTitle({ onTitled }),
    );

    let pending!: Promise<boolean>;
    act(() => {
      pending = result.current.setTitle(PACK_UUID, "Mon titre");
    });
    // Unmount BEFORE the write settles, then let it resolve.
    unmount();
    await act(async () => {
      resolve({ title: "Mon titre", source: "user" });
      await pending;
    });
    expect(onTitled).not.toHaveBeenCalled();
  });

  it("swallows a re-entrant call while a write is in flight", async () => {
    let resolveFirst!: (v: { title: string; source: "user" }) => void;
    vi.mocked(setDeviceStoryTitle).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveFirst = resolve;
      }),
    );
    const { result } = renderHook(() => useDeviceStoryTitle());

    let secondResult: boolean | undefined;
    await act(async () => {
      void result.current.setTitle(PACK_UUID, "First");
      // A second call before the first settles must be swallowed.
      secondResult = await result.current.setTitle(PACK_UUID, "Second");
    });
    expect(secondResult).toBe(false);
    expect(setDeviceStoryTitle).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveFirst({ title: "First", source: "user" });
    });
  });
});
