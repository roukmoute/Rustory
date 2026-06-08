import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

vi.mock("../../../ipc/commands/device-library", async () => {
  const actual = await vi.importActual<
    typeof import("../../../ipc/commands/device-library")
  >("../../../ipc/commands/device-library");
  return {
    ...actual,
    readDeviceLibrary: vi.fn(),
  };
});

import {
  readDeviceLibrary,
  ReadDeviceLibraryContractDriftError,
} from "../../../ipc/commands/device-library";

import {
  invalidateDeviceLibraryCache,
  useDeviceLibrary,
} from "./use-device-library";

const ID_A = "0123456789abcdef0123456789abcdef";
const ID_B = "fedcba9876543210fedcba9876543210";

const readable = (shortId: string) => ({
  kind: "readable" as const,
  deviceIdentifier: ID_A,
  stories: [{ uuid: `uuid-${shortId}`, shortId, hidden: false, contentPresent: true }],
});

function mockHandle(promise: Promise<unknown>): {
  promise: Promise<unknown>;
  cancel: () => void;
} {
  return { promise, cancel: vi.fn() };
}

describe("useDeviceLibrary", () => {
  beforeEach(() => {
    vi.mocked(readDeviceLibrary).mockReset();
    invalidateDeviceLibraryCache();
  });

  afterEach(() => {
    invalidateDeviceLibraryCache();
  });

  it("stays idle and issues no IPC when there is no readable device", () => {
    const { result } = renderHook(() => useDeviceLibrary(null));
    expect(result.current.state.kind).toBe("idle");
    expect(readDeviceLibrary).not.toHaveBeenCalled();
  });

  it("loads then transitions to ready with the device stories", async () => {
    vi.mocked(readDeviceLibrary).mockReturnValueOnce(
      mockHandle(Promise.resolve(readable("0000ABCD"))) as never,
    );
    const { result } = renderHook(() => useDeviceLibrary(ID_A));
    expect(result.current.state.kind).toBe("loading");
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    if (result.current.state.kind === "ready") {
      expect(result.current.state.stories).toHaveLength(1);
      expect(result.current.state.stories[0].shortId).toBe("0000ABCD");
    }
  });

  it("maps a none payload to idle (device gone between detection and read)", async () => {
    vi.mocked(readDeviceLibrary).mockReturnValueOnce(
      mockHandle(Promise.resolve({ kind: "none" })) as never,
    );
    const { result } = renderHook(() => useDeviceLibrary(ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("idle"));
  });

  it("maps an unsupported payload to idle", async () => {
    vi.mocked(readDeviceLibrary).mockReturnValueOnce(
      mockHandle(
        Promise.resolve({
          kind: "unsupported",
          reason: "metadataUnsupported",
          firmwareHint: null,
        }),
      ) as never,
    );
    const { result } = renderHook(() => useDeviceLibrary(ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("idle"));
  });

  it("surfaces a recoverable error when the read rejects (mid-read disconnect)", async () => {
    const err = {
      code: "DEVICE_SCAN_FAILED",
      message: "Lecture indisponible.",
      userAction: "Réessaie.",
      details: { source: "fs_read", kind: "not_found" },
    };
    vi.mocked(readDeviceLibrary).mockReturnValueOnce(
      mockHandle(Promise.reject(err)) as never,
    );
    const { result } = renderHook(() => useDeviceLibrary(ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("converts a drift error into a typed DEVICE_SCAN_FAILED AppError", async () => {
    vi.mocked(readDeviceLibrary).mockReturnValueOnce(
      mockHandle(
        Promise.reject(new ReadDeviceLibraryContractDriftError("nope", { raw: {} })),
      ) as never,
    );
    const { result } = renderHook(() => useDeviceLibrary(ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("refresh() re-reads the current device", async () => {
    vi.mocked(readDeviceLibrary)
      .mockReturnValueOnce(mockHandle(Promise.resolve(readable("0000ABCD"))) as never)
      .mockReturnValueOnce(mockHandle(Promise.resolve(readable("0000BEEF"))) as never);
    const { result } = renderHook(() => useDeviceLibrary(ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    act(() => {
      result.current.refresh();
    });
    await waitFor(() => {
      expect(
        result.current.state.kind === "ready" &&
          result.current.state.stories[0].shortId === "0000BEEF",
      ).toBe(true);
    });
  });

  it("re-reads when the deviceIdentifier changes (another Lunii plugged)", async () => {
    vi.mocked(readDeviceLibrary).mockReturnValue(
      mockHandle(Promise.resolve(readable("0000ABCD"))) as never,
    );
    const { rerender } = renderHook(({ id }) => useDeviceLibrary(id), {
      initialProps: { id: ID_A as string | null },
    });
    await waitFor(() =>
      expect(vi.mocked(readDeviceLibrary).mock.calls.length).toBeGreaterThan(0),
    );
    const callsAfterFirst = vi.mocked(readDeviceLibrary).mock.calls.length;
    rerender({ id: ID_B });
    await waitFor(() =>
      expect(vi.mocked(readDeviceLibrary).mock.calls.length).toBeGreaterThan(
        callsAfterFirst,
      ),
    );
    expect(vi.mocked(readDeviceLibrary).mock.lastCall?.[0]).toBe(ID_B);
  });

  it("clears to idle when the device identifier goes null", async () => {
    vi.mocked(readDeviceLibrary).mockReturnValue(
      mockHandle(Promise.resolve(readable("0000ABCD"))) as never,
    );
    const { result, rerender } = renderHook(({ id }) => useDeviceLibrary(id), {
      initialProps: { id: ID_A as string | null },
    });
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    rerender({ id: null });
    expect(result.current.state.kind).toBe("idle");
  });

  it("cancels the in-flight read on unmount", () => {
    const cancel = vi.fn();
    vi.mocked(readDeviceLibrary).mockReturnValueOnce({
      promise: new Promise(() => undefined),
      cancel,
    } as never);
    const { unmount } = renderHook(() => useDeviceLibrary(ID_A));
    unmount();
    expect(cancel).toHaveBeenCalled();
  });

  it("ignores a stale response from a superseded read", async () => {
    let resolveFirst: ((v: unknown) => void) | undefined;
    vi.mocked(readDeviceLibrary)
      .mockReturnValueOnce(
        mockHandle(
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
        ) as never,
      )
      .mockReturnValueOnce(mockHandle(Promise.resolve(readable("0000BEEF"))) as never);

    const { result } = renderHook(() => useDeviceLibrary(ID_A));
    act(() => {
      result.current.refresh();
    });
    await waitFor(() => {
      expect(
        result.current.state.kind === "ready" &&
          result.current.state.stories[0].shortId === "0000BEEF",
      ).toBe(true);
    });

    // Late resolve of the superseded first read MUST be ignored.
    act(() => {
      resolveFirst?.(readable("0000ABCD"));
    });
    expect(
      result.current.state.kind === "ready" &&
        result.current.state.stories[0].shortId === "0000BEEF",
    ).toBe(true);
  });

  it("renders the cached snapshot immediately on remount (SWR)", async () => {
    vi.mocked(readDeviceLibrary).mockReturnValue(
      mockHandle(Promise.resolve(readable("0000ABCD"))) as never,
    );
    const first = renderHook(() => useDeviceLibrary(ID_A));
    await waitFor(() => expect(first.result.current.state.kind).toBe("ready"));
    first.unmount();

    const second = renderHook(() => useDeviceLibrary(ID_A));
    expect(second.result.current.state.kind).toBe("ready");
    expect(second.result.current.isRefreshing).toBe(true);
  });

  it("invalidateDeviceLibraryCache clears the SWR snapshot", async () => {
    vi.mocked(readDeviceLibrary).mockReturnValue(
      mockHandle(Promise.resolve(readable("0000ABCD"))) as never,
    );
    const first = renderHook(() => useDeviceLibrary(ID_A));
    await waitFor(() => expect(first.result.current.state.kind).toBe("ready"));
    first.unmount();

    invalidateDeviceLibraryCache();

    const second = renderHook(() => useDeviceLibrary(ID_A));
    expect(second.result.current.state.kind).toBe("loading");
  });
});
