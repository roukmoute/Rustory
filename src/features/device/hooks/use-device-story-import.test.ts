import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-import", async () => {
  // Keep the REAL drift-error class exported: the hook branches on
  // `instanceof` to invalidate the overview cache after a post-commit
  // drift, so the mock must not shadow the class identity.
  const actual = await vi.importActual<
    typeof import("../../../ipc/commands/device-import")
  >("../../../ipc/commands/device-import");
  return { ...actual, importDeviceStory: vi.fn() };
});
vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: vi.fn(),
}));

import {
  ImportDeviceStoryContractDriftError,
  importDeviceStory,
} from "../../../ipc/commands/device-import";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { useDeviceStoryImport } from "./use-device-story-import";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";
const OTHER_PACK_UUID = "cdcdcdcd-cdcd-cdcd-cdcd-cdcd0000beef";

const SUCCESS_OUTCOME = {
  story: {
    id: "0197a5d0-0000-7000-8000-000000000000",
    title: "Histoire de ma Lunii (FAC5562D)",
  },
  packShortId: "FAC5562D",
  importedAt: "2026-06-10T12:00:00.000Z",
};

const RUST_ERROR = {
  code: "IMPORT_FAILED" as const,
  message: "Copie impossible: lecture de l'appareil interrompue.",
  userAction: "Vérifie la connexion de la Lunii puis réessaie la copie.",
  details: { source: "fs_read" },
};

describe("useDeviceStoryImport", () => {
  beforeEach(() => {
    vi.mocked(importDeviceStory).mockReset();
    vi.mocked(invalidateLibraryOverviewCache).mockReset();
  });

  it("starts in idle", () => {
    const { result } = renderHook(() => useDeviceStoryImport());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("transitions through importing → imported on success and reports to the route", async () => {
    vi.mocked(importDeviceStory).mockResolvedValueOnce(SUCCESS_OUTCOME);
    const onImported = vi.fn();
    const { result } = renderHook(() => useDeviceStoryImport({ onImported }));
    await act(async () => {
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    expect(importDeviceStory).toHaveBeenCalledWith({
      deviceIdentifier: DEVICE_ID,
      packUuid: PACK_UUID,
    });
    expect(result.current.status).toEqual({
      kind: "imported",
      story: SUCCESS_OUTCOME.story,
      packShortId: "FAC5562D",
    });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
    expect(onImported).toHaveBeenCalledWith(SUCCESS_OUTCOME);
  });

  it("transitions to failed with the AppError when Rust rejects", async () => {
    vi.mocked(importDeviceStory).mockRejectedValueOnce(RUST_ERROR);
    const { result } = renderHook(() => useDeviceStoryImport());
    await act(async () => {
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    expect(result.current.status).toEqual({ kind: "failed", error: RUST_ERROR });
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("re-entrancy: a second trigger while a first is in flight is a no-op", async () => {
    let resolveFirst!: (value: typeof SUCCESS_OUTCOME) => void;
    vi.mocked(importDeviceStory).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveFirst = resolve;
      }),
    );

    const { result } = renderHook(() => useDeviceStoryImport());
    let firstDone: Promise<void>;
    await act(async () => {
      firstDone = result.current.triggerImport(DEVICE_ID, PACK_UUID);
      // Fired synchronously after the first — must be swallowed.
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    expect(importDeviceStory).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveFirst(SUCCESS_OUTCOME);
      await firstDone;
    });
    expect(result.current.status.kind).toBe("imported");
  });

  it("attaches targetPackUuid to the STARTED pack and never to a swallowed re-entrant trigger", async () => {
    let resolveFirst!: (value: typeof SUCCESS_OUTCOME) => void;
    vi.mocked(importDeviceStory).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveFirst = resolve;
      }),
    );

    const { result } = renderHook(() => useDeviceStoryImport());
    expect(result.current.targetPackUuid).toBeNull();

    let firstDone: Promise<void>;
    await act(async () => {
      firstDone = result.current.triggerImport(DEVICE_ID, PACK_UUID);
      // A second copy on ANOTHER pack fired while the first is in flight:
      // swallowed by the re-entrancy guard, so the target must NOT move.
      await result.current.triggerImport(DEVICE_ID, OTHER_PACK_UUID);
    });
    expect(importDeviceStory).toHaveBeenCalledTimes(1);
    expect(result.current.targetPackUuid).toBe(PACK_UUID);

    await act(async () => {
      resolveFirst(SUCCESS_OUTCOME);
      await firstDone;
    });
    // Settled success still belongs to the pack that actually started.
    expect(result.current.status.kind).toBe("imported");
    expect(result.current.targetPackUuid).toBe(PACK_UUID);
  });

  it("dismissStatus clears targetPackUuid alongside the status", async () => {
    vi.mocked(importDeviceStory).mockResolvedValueOnce(SUCCESS_OUTCOME);
    const { result } = renderHook(() => useDeviceStoryImport());
    await act(async () => {
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    expect(result.current.targetPackUuid).toBe(PACK_UUID);
    act(() => {
      result.current.dismissStatus();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.targetPackUuid).toBeNull();
  });

  it("retryImport re-fires the last trigger from a failed state", async () => {
    vi.mocked(importDeviceStory)
      .mockRejectedValueOnce(RUST_ERROR)
      .mockResolvedValueOnce(SUCCESS_OUTCOME);
    const { result } = renderHook(() => useDeviceStoryImport());
    await act(async () => {
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    expect(result.current.status.kind).toBe("failed");

    await act(async () => {
      await result.current.retryImport();
    });
    expect(importDeviceStory).toHaveBeenNthCalledWith(2, {
      deviceIdentifier: DEVICE_ID,
      packUuid: PACK_UUID,
    });
    expect(result.current.status.kind).toBe("imported");
  });

  it("retryImport is a no-op outside the failed state", async () => {
    const { result } = renderHook(() => useDeviceStoryImport());
    await act(async () => {
      await result.current.retryImport();
    });
    expect(importDeviceStory).not.toHaveBeenCalled();
  });

  it("dismissStatus clears an imported status back to idle", async () => {
    vi.mocked(importDeviceStory).mockResolvedValueOnce(SUCCESS_OUTCOME);
    const { result } = renderHook(() => useDeviceStoryImport());
    await act(async () => {
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    act(() => {
      result.current.dismissStatus();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("dismissStatus clears a failed alert back to idle (the alert's Fermer must work)", async () => {
    vi.mocked(importDeviceStory).mockRejectedValueOnce(RUST_ERROR);
    const { result } = renderHook(() => useDeviceStoryImport());
    await act(async () => {
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    expect(result.current.status.kind).toBe("failed");
    act(() => {
      result.current.dismissStatus();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("a post-commit contract drift still invalidates the overview cache before failing", async () => {
    // The drift rejects AFTER Rust committed the import: the local store
    // HAS changed, so the stale overview snapshot must be dropped even
    // though the outcome is unrenderable.
    vi.mocked(importDeviceStory).mockRejectedValueOnce(
      new ImportDeviceStoryContractDriftError({ drifted: true }),
    );
    const { result } = renderHook(() => useDeviceStoryImport());
    await act(async () => {
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    expect(result.current.status.kind).toBe("failed");
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
  });

  it("a throwing onImported callback never reclassifies a committed import as failed", async () => {
    vi.mocked(importDeviceStory).mockResolvedValueOnce(SUCCESS_OUTCOME);
    const onImported = vi.fn(() => {
      throw new Error("orchestration exploded");
    });
    const { result } = renderHook(() => useDeviceStoryImport({ onImported }));
    await act(async () => {
      await result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    expect(onImported).toHaveBeenCalledTimes(1);
    expect(result.current.status).toEqual({
      kind: "imported",
      story: SUCCESS_OUTCOME.story,
      packShortId: "FAC5562D",
    });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
  });

  it("invalidates the overview cache even when unmounted mid-copy (coherence survives)", async () => {
    let resolveCall!: (value: typeof SUCCESS_OUTCOME) => void;
    vi.mocked(importDeviceStory).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveCall = resolve;
      }),
    );
    const onImported = vi.fn();
    const { result, unmount } = renderHook(() =>
      useDeviceStoryImport({ onImported }),
    );
    let done: Promise<void>;
    await act(async () => {
      done = result.current.triggerImport(DEVICE_ID, PACK_UUID);
    });
    unmount();
    await act(async () => {
      resolveCall(SUCCESS_OUTCOME);
      await done;
    });
    // The module-local cache invalidation ran regardless of the unmount…
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
    // …but no state update nor orchestration callback fired post-unmount.
    expect(onImported).not.toHaveBeenCalled();
  });
});
