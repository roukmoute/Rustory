import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-catalog", () => ({
  readPackCover: vi.fn(),
}));

import { readPackCover } from "../../../ipc/commands/device-catalog";
import { invalidatePackCoverCache, usePackCover } from "./use-pack-cover";

const UUID_A = "abababab-abab-abab-abab-ababfac5562d";
const OLD_COVER = "data:image/png;base64,AAAA";
const NEW_COVER = "data:image/png;base64,BBBB";

function pendingCover() {
  let resolve!: (value: { dataUrl: string }) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<{ dataUrl: string }>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("usePackCover", () => {
  beforeEach(() => {
    vi.mocked(readPackCover).mockReset();
    invalidatePackCoverCache();
  });

  afterEach(() => {
    cleanup();
    invalidatePackCoverCache();
  });

  it("never requests a cover when the pack has none (hasCover=false)", () => {
    const { result } = renderHook(() => usePackCover(UUID_A, false));
    act(() => invalidatePackCoverCache());
    expect(result.current).toBeNull();
    expect(readPackCover).not.toHaveBeenCalled();
  });

  it("resolves the cover data URL when one exists", async () => {
    vi.mocked(readPackCover).mockResolvedValueOnce({
      dataUrl: "data:image/png;base64,AAAA",
    });
    const { result } = renderHook(() => usePackCover(UUID_A, true));
    await waitFor(() =>
      expect(result.current).toBe("data:image/png;base64,AAAA"),
    );
  });

  it("caches the result so a second consumer does not re-hit the backend", async () => {
    vi.mocked(readPackCover).mockResolvedValueOnce({
      dataUrl: "data:image/png;base64,AAAA",
    });
    const first = renderHook(() => usePackCover(UUID_A, true));
    await waitFor(() => expect(first.result.current).not.toBeNull());

    const second = renderHook(() => usePackCover(UUID_A, true));
    await waitFor(() =>
      expect(second.result.current).toBe("data:image/png;base64,AAAA"),
    );
    expect(readPackCover).toHaveBeenCalledTimes(1);
  });

  it("degrades to null (no cover) when the read fails", async () => {
    vi.mocked(readPackCover).mockRejectedValueOnce(new Error("boom"));
    const { result } = renderHook(() => usePackCover(UUID_A, true));
    // Stays null; the failure is swallowed (covers are decorative).
    await waitFor(() => expect(readPackCover).toHaveBeenCalled());
    expect(result.current).toBeNull();
  });

  it("re-resolves covers for already mounted consumers after catalog invalidation", async () => {
    vi.mocked(readPackCover).mockResolvedValue({ dataUrl: OLD_COVER });
    const first = renderHook(() => usePackCover(UUID_A, true));
    await waitFor(() => expect(first.result.current).toBe(OLD_COVER));
    const second = renderHook(() => usePackCover(UUID_A, true));
    expect(second.result.current).toBe(OLD_COVER);
    vi.mocked(readPackCover).mockResolvedValue({ dataUrl: NEW_COVER });

    act(() => invalidatePackCoverCache());

    await waitFor(() => {
      expect(first.result.current).toBe(NEW_COVER);
      expect(second.result.current).toBe(NEW_COVER);
    });
    expect(readPackCover).toHaveBeenCalledTimes(3);
  });

  it.each(["resolve", "reject"] as const)(
    "ignores a pre-invalidation read that later %ss without changing the current cover or cache",
    async (outcome) => {
      const oldRead = pendingCover();
      vi.mocked(readPackCover)
        .mockReturnValueOnce(oldRead.promise)
        .mockResolvedValueOnce({ dataUrl: NEW_COVER });
      const first = renderHook(() => usePackCover(UUID_A, true));

      act(() => invalidatePackCoverCache());
      await waitFor(() => expect(first.result.current).toBe(NEW_COVER));
      await act(async () => {
        if (outcome === "resolve") oldRead.resolve({ dataUrl: OLD_COVER });
        else oldRead.reject(new Error("old read failed"));
      });
      expect(first.result.current).toBe(NEW_COVER);

      const second = renderHook(() => usePackCover(UUID_A, true));
      expect(second.result.current).toBe(NEW_COVER);
      expect(readPackCover).toHaveBeenCalledTimes(2);
    },
  );

  it("does not let an unmounted consumer restore a cover cached before invalidation", async () => {
    const oldRead = pendingCover();
    vi.mocked(readPackCover)
      .mockReturnValueOnce(oldRead.promise)
      .mockResolvedValueOnce({ dataUrl: NEW_COVER });
    const first = renderHook(() => usePackCover(UUID_A, true));
    first.unmount();

    invalidatePackCoverCache();
    expect(readPackCover).toHaveBeenCalledTimes(1);
    await act(async () => oldRead.resolve({ dataUrl: OLD_COVER }));

    const second = renderHook(() => usePackCover(UUID_A, true));
    expect(second.result.current).toBeNull();
    await waitFor(() => expect(second.result.current).toBe(NEW_COVER));
    expect(readPackCover).toHaveBeenCalledTimes(2);
  });
});
