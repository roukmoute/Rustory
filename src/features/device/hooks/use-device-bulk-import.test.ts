import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-import", async () => {
  // Keep the REAL drift-error class: the hook branches on `instanceof` to
  // invalidate the overview cache after a post-commit drift.
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
import { useDeviceBulkImport } from "./use-device-bulk-import";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const UUID_A = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
const UUID_B = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
const UUID_C = "cccccccc-cccc-cccc-cccc-cccccccccccc";

const OUTCOME = {
  story: {
    id: "0197a5d0-0000-7000-8000-000000000000",
    title: "Histoire",
  },
  packShortId: "FAC5562D",
  importedAt: "2026-06-10T12:00:00.000Z",
};

const RUST_ERROR = {
  code: "IMPORT_FAILED" as const,
  message: "Copie impossible: lecture de l'appareil interrompue.",
  userAction: "Vérifie la connexion de la Lunii puis réessaie.",
  details: { source: "fs_read" },
};

describe("useDeviceBulkImport", () => {
  beforeEach(() => {
    vi.mocked(importDeviceStory).mockReset();
    vi.mocked(invalidateLibraryOverviewCache).mockReset();
  });

  it("starts idle", () => {
    const { result } = renderHook(() => useDeviceBulkImport());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("imports every pack sequentially, in order, and reports a full tally", async () => {
    const order: string[] = [];
    vi.mocked(importDeviceStory).mockImplementation(async ({ packUuid }) => {
      order.push(packUuid);
      return OUTCOME;
    });
    const onCompleted = vi.fn();
    const { result } = renderHook(() => useDeviceBulkImport({ onCompleted }));

    await act(async () => {
      await result.current.start(DEVICE_ID, [UUID_A, UUID_B, UUID_C]);
    });

    expect(order).toEqual([UUID_A, UUID_B, UUID_C]);
    expect(importDeviceStory).toHaveBeenCalledTimes(3);
    expect(result.current.status).toEqual({
      kind: "done",
      total: 3,
      succeeded: 3,
      failed: 0,
      firstError: null,
    });
    // One authoritative re-read for the whole batch, and one cache drop
    // per landed pack.
    expect(onCompleted).toHaveBeenCalledTimes(1);
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(3);
  });

  it("continues past a failure and keeps the first error in the summary", async () => {
    vi.mocked(importDeviceStory)
      .mockResolvedValueOnce(OUTCOME)
      .mockRejectedValueOnce(RUST_ERROR)
      .mockResolvedValueOnce(OUTCOME);
    const { result } = renderHook(() => useDeviceBulkImport());

    await act(async () => {
      await result.current.start(DEVICE_ID, [UUID_A, UUID_B, UUID_C]);
    });

    // All three were attempted despite the middle failure.
    expect(importDeviceStory).toHaveBeenCalledTimes(3);
    expect(result.current.status).toMatchObject({
      kind: "done",
      total: 3,
      succeeded: 2,
      failed: 1,
    });
    const status = result.current.status;
    expect(status.kind === "done" && status.firstError?.message).toBe(
      RUST_ERROR.message,
    );
  });

  it("drops the overview cache after a post-commit contract drift too", async () => {
    vi.mocked(importDeviceStory).mockRejectedValueOnce(
      new ImportDeviceStoryContractDriftError({ bad: true }),
    );
    const { result } = renderHook(() => useDeviceBulkImport());

    await act(async () => {
      await result.current.start(DEVICE_ID, [UUID_A]);
    });

    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
    expect(result.current.status).toMatchObject({
      kind: "done",
      succeeded: 0,
      failed: 1,
    });
  });

  it("is a no-op for an empty selection", async () => {
    const onCompleted = vi.fn();
    const { result } = renderHook(() => useDeviceBulkImport({ onCompleted }));

    await act(async () => {
      await result.current.start(DEVICE_ID, []);
    });

    expect(importDeviceStory).not.toHaveBeenCalled();
    expect(onCompleted).not.toHaveBeenCalled();
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("swallows a re-entrant start while a batch is in flight", async () => {
    let releaseFirst: (() => void) | null = null;
    vi.mocked(importDeviceStory).mockImplementation(
      () =>
        new Promise((resolve) => {
          releaseFirst = () => resolve(OUTCOME);
        }),
    );
    const { result } = renderHook(() => useDeviceBulkImport());

    let firstDone!: Promise<void>;
    act(() => {
      firstDone = result.current.start(DEVICE_ID, [UUID_A]);
    });
    // A second start while the first is still awaiting is ignored.
    await act(async () => {
      await result.current.start(DEVICE_ID, [UUID_B, UUID_C]);
    });
    await act(async () => {
      releaseFirst?.();
      await firstDone;
    });

    // Only the FIRST batch ran (one pack), never the swallowed second.
    expect(importDeviceStory).toHaveBeenCalledTimes(1);
    expect(importDeviceStory).toHaveBeenCalledWith({
      deviceIdentifier: DEVICE_ID,
      packUuid: UUID_A,
    });
  });

  it("dismisses a terminal summary back to idle", async () => {
    vi.mocked(importDeviceStory).mockResolvedValue(OUTCOME);
    const { result } = renderHook(() => useDeviceBulkImport());

    await act(async () => {
      await result.current.start(DEVICE_ID, [UUID_A]);
    });
    expect(result.current.status.kind).toBe("done");

    act(() => {
      result.current.dismiss();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });
});
