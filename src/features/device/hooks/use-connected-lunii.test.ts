import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

import { CONNECTED_LUNII_POLL_INTERVAL_MS } from "./use-connected-lunii";

vi.mock("../../../ipc/commands/device", async () => {
  const actual = await vi.importActual<
    typeof import("../../../ipc/commands/device")
  >("../../../ipc/commands/device");
  return {
    ...actual,
    readConnectedLunii: vi.fn(),
  };
});

import {
  readConnectedLunii,
  ReadConnectedLuniiContractDriftError,
} from "../../../ipc/commands/device";

import {
  invalidateConnectedLuniiCache,
  useConnectedLunii,
} from "./use-connected-lunii";

const supported = {
  kind: "supported" as const,
  family: "lunii" as const,
  firmwareCohort: "origineV1" as const,
  metadataFormatVersion: 3,
  deviceIdentifier: "abc",
  supportedOperations: {
    readLibrary: true,
    inspectStory: true,
    importStory: true,
    writeStory: false,
  },
};

// REAL Rust wire for a recognized FLAM Gen1: the metadataFormatVersion
// key is ABSENT (never null) and every operation is false — mirrors the
// byte-for-byte contract test (src-tauri/tests/contracts/device.rs).
const supportedFlam = JSON.parse(
  '{"kind":"supported","family":"flam","firmwareCohort":"flamGen1",' +
    '"deviceIdentifier":"fedcba9876543210fedcba9876543210",' +
    '"supportedOperations":{"readLibrary":false,"inspectStory":false,' +
    '"importStory":false,"writeStory":false}}',
);

function mockHandle(promise: Promise<unknown>): { promise: Promise<unknown>; cancel: () => void } {
  return { promise, cancel: vi.fn() };
}

describe("useConnectedLunii", () => {
  beforeEach(() => {
    vi.mocked(readConnectedLunii).mockReset();
    invalidateConnectedLuniiCache();
  });

  afterEach(() => {
    invalidateConnectedLuniiCache();
  });

  it("starts in loading state then transitions to ready with the device", async () => {
    vi.mocked(readConnectedLunii).mockReturnValueOnce(
      mockHandle(Promise.resolve(supported)) as never,
    );
    const { result } = renderHook(() => useConnectedLunii());
    expect(result.current.state.kind).toBe("loading");
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    if (result.current.state.kind === "ready") {
      expect(result.current.state.device).toBe(supported);
    }
    expect(result.current.isRefreshing).toBe(false);
  });

  it("resolves to ready with kind=none when no device is connected", async () => {
    const none = { kind: "none" as const };
    vi.mocked(readConnectedLunii).mockReturnValueOnce(
      mockHandle(Promise.resolve(none)) as never,
    );
    const { result } = renderHook(() => useConnectedLunii());
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    if (result.current.state.kind === "ready") {
      expect(result.current.state.device.kind).toBe("none");
    }
  });

  it("resolves to ready with a recognized FLAM device (real wire, no version key)", async () => {
    vi.mocked(readConnectedLunii).mockReturnValueOnce(
      mockHandle(Promise.resolve(supportedFlam)) as never,
    );
    const { result } = renderHook(() => useConnectedLunii());
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    if (result.current.state.kind === "ready") {
      const device = result.current.state.device;
      expect(device.kind).toBe("supported");
      if (device.kind === "supported") {
        expect(device.family).toBe("flam");
        expect(device.firmwareCohort).toBe("flamGen1");
        // Absent key reads as undefined on the typed DTO — never null.
        expect(device.metadataFormatVersion).toBeUndefined();
        expect(device.supportedOperations.readLibrary).toBe(false);
        expect(device.supportedOperations.writeStory).toBe(false);
      }
    }
  });

  it("resolves to error when backend rejects with DEVICE_SCAN_FAILED", async () => {
    const err = {
      code: "DEVICE_SCAN_FAILED",
      message: "Détection indisponible.",
      userAction: "Réessaie.",
      details: { source: "fs_read", kind: "permission_denied" },
    };
    vi.mocked(readConnectedLunii).mockReturnValueOnce(
      mockHandle(Promise.reject(err)) as never,
    );
    const { result } = renderHook(() => useConnectedLunii());
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("converts a drift error into a typed DEVICE_SCAN_FAILED AppError", async () => {
    const drift = new ReadConnectedLuniiContractDriftError("nope", { raw: {} });
    vi.mocked(readConnectedLunii).mockReturnValueOnce(
      mockHandle(Promise.reject(drift)) as never,
    );
    const { result } = renderHook(() => useConnectedLunii());
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("refresh() supersedes the in-flight call with the latest result", async () => {
    let resolveSecond: ((v: unknown) => void) | undefined;
    vi.mocked(readConnectedLunii)
      .mockReturnValueOnce(mockHandle(Promise.resolve(supported)) as never)
      .mockReturnValueOnce(
        mockHandle(
          new Promise((resolve) => {
            resolveSecond = resolve;
          }),
        ) as never,
      );
    const { result } = renderHook(() => useConnectedLunii());
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    act(() => {
      result.current.refresh();
    });
    expect(result.current.isRefreshing).toBe(true);

    const next = { kind: "none" as const };
    act(() => {
      resolveSecond?.(next);
    });
    await waitFor(() =>
      expect(
        result.current.state.kind === "ready" &&
          result.current.state.device.kind === "none",
      ).toBe(true),
    );
  });

  it("cancels the in-flight call on unmount", async () => {
    const cancel = vi.fn();
    vi.mocked(readConnectedLunii).mockReturnValueOnce({
      promise: new Promise(() => undefined),
      cancel,
    } as never);
    const { unmount } = renderHook(() => useConnectedLunii());
    unmount();
    expect(cancel).toHaveBeenCalled();
  });

  it("renders the cached snapshot during a background refresh", async () => {
    vi.mocked(readConnectedLunii)
      .mockReturnValueOnce(mockHandle(Promise.resolve(supported)) as never)
      .mockReturnValueOnce(
        mockHandle(new Promise(() => undefined)) as never,
      );

    const first = renderHook(() => useConnectedLunii());
    await waitFor(() => expect(first.result.current.state.kind).toBe("ready"));
    first.unmount();

    // Second render should immediately surface the cached snapshot
    // while a fresh scan runs.
    const second = renderHook(() => useConnectedLunii());
    expect(second.result.current.state.kind).toBe("ready");
    expect(second.result.current.isRefreshing).toBe(true);
  });

  it("a stale response from a superseded call cannot overwrite a fresher state", async () => {
    let resolveFirst: ((v: unknown) => void) | undefined;
    vi.mocked(readConnectedLunii)
      .mockReturnValueOnce(
        mockHandle(
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
        ) as never,
      )
      .mockReturnValueOnce(
        mockHandle(Promise.resolve({ kind: "none" })) as never,
      );

    const { result } = renderHook(() => useConnectedLunii());
    act(() => {
      result.current.refresh(); // supersedes the first call
    });
    await waitFor(() =>
      expect(
        result.current.state.kind === "ready" &&
          result.current.state.device.kind === "none",
      ).toBe(true),
    );

    // Late resolve from the original superseded handle MUST be ignored.
    act(() => {
      resolveFirst?.(supported);
    });
    expect(
      result.current.state.kind === "ready" &&
        result.current.state.device.kind === "none",
    ).toBe(true);
  });

  it("invalidateConnectedLuniiCache clears the SWR snapshot", async () => {
    vi.mocked(readConnectedLunii).mockReturnValue(
      mockHandle(Promise.resolve(supported)) as never,
    );
    const first = renderHook(() => useConnectedLunii());
    await waitFor(() => expect(first.result.current.state.kind).toBe("ready"));
    first.unmount();

    invalidateConnectedLuniiCache();

    const second = renderHook(() => useConnectedLunii());
    expect(second.result.current.state.kind).toBe("loading");
  });
});

describe("useConnectedLunii — silent background polling", () => {
  beforeEach(() => {
    vi.mocked(readConnectedLunii).mockReset();
    invalidateConnectedLuniiCache();
  });

  afterEach(() => {
    invalidateConnectedLuniiCache();
  });

  it("re-fetches every CONNECTED_LUNII_POLL_INTERVAL_MS without flipping isRefreshing", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      vi.mocked(readConnectedLunii).mockReturnValue(
        mockHandle(Promise.resolve(supported)) as never,
      );
      const { result } = renderHook(() => useConnectedLunii());
      await waitFor(() => expect(result.current.state.kind).toBe("ready"));
      expect(result.current.isRefreshing).toBe(false);
      const callsAfterInitial = vi.mocked(readConnectedLunii).mock.calls.length;

      // Advance past one polling interval — a new silent poll MUST
      // fire and increment the IPC call count.
      await act(async () => {
        vi.advanceTimersByTime(CONNECTED_LUNII_POLL_INTERVAL_MS + 1);
        await Promise.resolve();
      });

      expect(
        vi.mocked(readConnectedLunii).mock.calls.length,
      ).toBeGreaterThan(callsAfterInitial);
      // Silent poll MUST NOT flip the user-visible flag — the panel
      // would otherwise flash `Détection en cours…` every 3 s.
      expect(result.current.isRefreshing).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("polling is wired with setInterval at the documented cadence", () => {
    const spy = vi.spyOn(globalThis, "setInterval");
    vi.mocked(readConnectedLunii).mockReturnValueOnce(
      mockHandle(Promise.resolve(supported)) as never,
    );
    const { unmount } = renderHook(() => useConnectedLunii());
    const intervalCall = spy.mock.calls.find(
      (c) => c[1] === CONNECTED_LUNII_POLL_INTERVAL_MS,
    );
    expect(intervalCall).toBeDefined();
    unmount();
    spy.mockRestore();
  });

  it("silent poll updates state when the underlying device changes", async () => {
    const none = { kind: "none" as const };
    vi.mocked(readConnectedLunii)
      .mockReturnValueOnce(mockHandle(Promise.resolve(supported)) as never)
      .mockReturnValue(mockHandle(Promise.resolve(none)) as never);
    const { result } = renderHook(() => useConnectedLunii());
    await waitFor(
      () =>
        result.current.state.kind === "ready" &&
        result.current.state.device.kind === "supported",
    );

    // Trigger a manual refresh (same code path as the silent poll for
    // result handling, only difference is isRefreshing). Validates the
    // state update semantics.
    act(() => {
      result.current.refresh();
    });
    await waitFor(() =>
      expect(
        result.current.state.kind === "ready" &&
          result.current.state.device.kind === "none",
      ).toBe(true),
    );
  });

  it("clears the polling interval on unmount", () => {
    const clearSpy = vi.spyOn(globalThis, "clearInterval");
    vi.mocked(readConnectedLunii).mockReturnValueOnce(
      mockHandle(Promise.resolve(supported)) as never,
    );
    const { unmount } = renderHook(() => useConnectedLunii());
    unmount();
    expect(clearSpy).toHaveBeenCalled();
    clearSpy.mockRestore();
  });

  it("CONNECTED_LUNII_POLL_INTERVAL_MS is well under the NFR4 5s cap", () => {
    expect(CONNECTED_LUNII_POLL_INTERVAL_MS).toBeLessThan(5000);
    expect(CONNECTED_LUNII_POLL_INTERVAL_MS).toBeGreaterThanOrEqual(1000);
  });
});
