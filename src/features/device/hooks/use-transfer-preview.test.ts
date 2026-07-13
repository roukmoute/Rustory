import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

vi.mock("../../../ipc/commands/transfer-preview", async () => {
  const actual = await vi.importActual<
    typeof import("../../../ipc/commands/transfer-preview")
  >("../../../ipc/commands/transfer-preview");
  return {
    ...actual,
    readTransferPreview: vi.fn(),
  };
});

import {
  readTransferPreview,
  ReadTransferPreviewContractDriftError,
} from "../../../ipc/commands/transfer-preview";

import { useTransferPreview } from "./use-transfer-preview";

const ID_A = "0123456789abcdef0123456789abcdef";
const ID_B = "fedcba9876543210fedcba9876543210";
const STORY = "0197a5d0-0000-7000-8000-000000000000";

const ready = (onDevice: boolean, unchangedCount: number) => ({
  kind: "ready" as const,
  deviceIdentifier: ID_A,
  story: { id: STORY, title: "Mon histoire" },
  onDevice,
  unchangedCount,
  transferable: false,
});

function mockHandle(promise: Promise<unknown>): {
  promise: Promise<unknown>;
  cancel: () => void;
} {
  return { promise, cancel: vi.fn() };
}

describe("useTransferPreview", () => {
  beforeEach(() => {
    vi.mocked(readTransferPreview).mockReset();
  });

  it("stays idle and issues no IPC when there is no comparable pair", () => {
    const a = renderHook(() => useTransferPreview(null, ID_A));
    expect(a.result.current.state.kind).toBe("idle");
    const b = renderHook(() => useTransferPreview(STORY, null));
    expect(b.result.current.state.kind).toBe("idle");
    expect(readTransferPreview).not.toHaveBeenCalled();
  });

  it("loads then transitions to ready with the composed comparison", async () => {
    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(Promise.resolve(ready(false, 3))) as never,
    );
    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    expect(result.current.state.kind).toBe("loading");
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    if (result.current.state.kind === "ready") {
      expect(result.current.state.onDevice).toBe(false);
      expect(result.current.state.unchangedCount).toBe(3);
      expect(result.current.state.storyTitle).toBe("Mon histoire");
    }
  });

  it("phrases the device-changed next gesture family-correct (FLAM vs Lunii verbatim)", async () => {
    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(Promise.resolve({ kind: "noDevice" })) as never,
    );
    const flam = renderHook(() => useTransferPreview(STORY, ID_A, "flam"));
    await waitFor(() => expect(flam.result.current.state.kind).toBe("error"));
    if (flam.result.current.state.kind === "error") {
      expect(flam.result.current.state.error.userAction).toBe(
        "Vérifie que l'appareil est toujours branché puis réessaie la comparaison.",
      );
    }
    flam.unmount();

    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(Promise.resolve({ kind: "noDevice" })) as never,
    );
    const lunii = renderHook(() => useTransferPreview(STORY, ID_A, "lunii"));
    await waitFor(() => expect(lunii.result.current.state.kind).toBe("error"));
    if (lunii.result.current.state.kind === "error") {
      expect(lunii.result.current.state.error.userAction).toBe(
        "Vérifie que la Lunii est toujours branchée puis réessaie la comparaison.",
      );
    }
  });

  it("surfaces a recoverable device-changed error when the device folds to noDevice", async () => {
    // A readable device was requested, but the authoritative re-read no longer
    // resolves to it → recoverable "device changed", never a silent idle.
    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(Promise.resolve({ kind: "noDevice" })) as never,
    );
    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.message).toMatch(/l'appareil a changé/i);
    }
  });

  it("surfaces a recoverable device-changed error on an unsupported response", async () => {
    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(
        Promise.resolve({ kind: "unsupported", reason: "metadataUnsupported" }),
      ) as never,
    );
    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.message).toMatch(/l'appareil a changé/i);
    }
  });

  it("surfaces a recoverable error when the read rejects", async () => {
    const err = {
      code: "DEVICE_SCAN_FAILED",
      message: "Comparaison indisponible.",
      userAction: "Réessaie.",
      details: { source: "device_changed" },
    };
    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(Promise.reject(err)) as never,
    );
    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("converts a drift error into a typed DEVICE_SCAN_FAILED AppError", async () => {
    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(
        Promise.reject(new ReadTransferPreviewContractDriftError("nope", { raw: {} })),
      ) as never,
    );
    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("refresh() re-reads the current pair", async () => {
    vi.mocked(readTransferPreview)
      .mockReturnValueOnce(mockHandle(Promise.resolve(ready(false, 1))) as never)
      .mockReturnValueOnce(mockHandle(Promise.resolve(ready(true, 0))) as never);
    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    act(() => {
      result.current.refresh();
    });
    await waitFor(() => {
      expect(
        result.current.state.kind === "ready" && result.current.state.onDevice,
      ).toBe(true);
    });
  });

  it("re-reads when the device identifier changes (another Lunii plugged)", async () => {
    vi.mocked(readTransferPreview).mockReturnValue(
      mockHandle(Promise.resolve(ready(false, 1))) as never,
    );
    const { rerender } = renderHook(
      ({ id }) => useTransferPreview(STORY, id),
      { initialProps: { id: ID_A as string | null } },
    );
    await waitFor(() =>
      expect(vi.mocked(readTransferPreview).mock.calls.length).toBeGreaterThan(0),
    );
    const before = vi.mocked(readTransferPreview).mock.calls.length;
    rerender({ id: ID_B });
    await waitFor(() =>
      expect(vi.mocked(readTransferPreview).mock.calls.length).toBeGreaterThan(
        before,
      ),
    );
    expect(vi.mocked(readTransferPreview).mock.lastCall?.[0]).toEqual({
      storyId: STORY,
      deviceIdentifier: ID_B,
    });
  });

  it("clears to idle when either side goes null", async () => {
    vi.mocked(readTransferPreview).mockReturnValue(
      mockHandle(Promise.resolve(ready(false, 1))) as never,
    );
    const { result, rerender } = renderHook(
      ({ sid }: { sid: string | null }) => useTransferPreview(sid, ID_A),
      { initialProps: { sid: STORY as string | null } },
    );
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    rerender({ sid: null });
    expect(result.current.state.kind).toBe("idle");
  });

  it("cancels the in-flight read on unmount", () => {
    const cancel = vi.fn();
    vi.mocked(readTransferPreview).mockReturnValueOnce({
      promise: new Promise(() => undefined),
      cancel,
    } as never);
    const { unmount } = renderHook(() => useTransferPreview(STORY, ID_A));
    unmount();
    expect(cancel).toHaveBeenCalled();
  });

  it("ignores a stale response from a superseded read", async () => {
    let resolveFirst: ((v: unknown) => void) | undefined;
    vi.mocked(readTransferPreview)
      .mockReturnValueOnce(
        mockHandle(
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
        ) as never,
      )
      .mockReturnValueOnce(mockHandle(Promise.resolve(ready(true, 0))) as never);

    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    act(() => {
      result.current.refresh();
    });
    await waitFor(() => {
      expect(
        result.current.state.kind === "ready" && result.current.state.onDevice,
      ).toBe(true);
    });

    // Late resolve of the superseded first read MUST be ignored.
    act(() => {
      resolveFirst?.(ready(false, 9));
    });
    expect(
      result.current.state.kind === "ready" && result.current.state.onDevice,
    ).toBe(true);
  });

  it("always re-reads fresh on remount — no cached verdict (decision surface)", async () => {
    vi.mocked(readTransferPreview).mockReturnValue(
      mockHandle(Promise.resolve(ready(false, 1))) as never,
    );
    const first = renderHook(() => useTransferPreview(STORY, ID_A));
    await waitFor(() => expect(first.result.current.state.kind).toBe("ready"));
    first.unmount();

    // No SWR cache: a remount must start from loading and re-read, never
    // paint a (possibly stale) cached add/replace verdict on a pre-write surface.
    const second = renderHook(() => useTransferPreview(STORY, ID_A));
    expect(second.result.current.state.kind).toBe("loading");
    await waitFor(() => expect(second.result.current.state.kind).toBe("ready"));
  });

  it("surfaces device-changed when the ready payload's deviceIdentifier does not match", async () => {
    // Backend echoes a DIFFERENT device than requested (misroute/race).
    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(
        Promise.resolve({ ...ready(false, 1), deviceIdentifier: ID_B }),
      ) as never,
    );
    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.message).toMatch(/l'appareil a changé/i);
    }
  });

  it("surfaces device-changed when the ready payload's story.id does not match", async () => {
    vi.mocked(readTransferPreview).mockReturnValueOnce(
      mockHandle(
        Promise.resolve({
          ...ready(false, 1),
          story: { id: "0197a5d0-0000-7000-8000-ffffffffffff", title: "Autre" },
        }),
      ) as never,
    );
    const { result } = renderHook(() => useTransferPreview(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
  });
});
