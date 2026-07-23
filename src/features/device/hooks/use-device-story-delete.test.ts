import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-delete", () => ({
  deleteDeviceStory: vi.fn(),
}));

import { deleteDeviceStory } from "../../../ipc/commands/device-delete";
import { useDeviceStoryDelete } from "./use-device-story-delete";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

const RUST_ERROR = {
  code: "DEVICE_DELETE_FAILED" as const,
  message: "Suppression impossible: l'appareil a refusé l'écriture.",
  userAction: "Vérifie que l'appareil est bien connecté puis réessaie.",
  details: { source: "delete_rejected" },
};

describe("useDeviceStoryDelete", () => {
  beforeEach(() => {
    vi.mocked(deleteDeviceStory).mockReset();
  });

  it("starts idle", () => {
    const { result } = renderHook(() => useDeviceStoryDelete());
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.targetPackUuid).toBeNull();
  });

  it("transitions deleting → deleted and reports to the route", async () => {
    vi.mocked(deleteDeviceStory).mockResolvedValueOnce({
      packUuid: PACK_UUID,
      wasPresent: true,
    });
    const onDeleted = vi.fn();
    const { result } = renderHook(() => useDeviceStoryDelete({ onDeleted }));
    await act(async () => {
      await result.current.triggerDelete(DEVICE_ID, PACK_UUID);
    });
    expect(deleteDeviceStory).toHaveBeenCalledWith({
      deviceIdentifier: DEVICE_ID,
      packUuid: PACK_UUID,
    });
    expect(result.current.status).toEqual({ kind: "deleted", wasPresent: true });
    expect(result.current.targetPackUuid).toBe(PACK_UUID);
    expect(onDeleted).toHaveBeenCalledTimes(1);
  });

  it("surfaces a failure without calling onDeleted", async () => {
    vi.mocked(deleteDeviceStory).mockRejectedValueOnce(RUST_ERROR);
    const onDeleted = vi.fn();
    const { result } = renderHook(() => useDeviceStoryDelete({ onDeleted }));
    await act(async () => {
      await result.current.triggerDelete(DEVICE_ID, PACK_UUID);
    });
    expect(result.current.status).toMatchObject({ kind: "failed" });
    expect(onDeleted).not.toHaveBeenCalled();
  });

  it("swallows a re-entrant trigger while a delete is in flight", async () => {
    let release: (() => void) | null = null;
    vi.mocked(deleteDeviceStory).mockImplementation(
      () =>
        new Promise((resolve) => {
          release = () => resolve({ packUuid: PACK_UUID, wasPresent: true });
        }),
    );
    const { result } = renderHook(() => useDeviceStoryDelete());
    let first!: Promise<void>;
    act(() => {
      first = result.current.triggerDelete(DEVICE_ID, PACK_UUID);
    });
    await act(async () => {
      await result.current.triggerDelete(DEVICE_ID, "cccccccc-cccc-cccc-cccc-cccccccccccc");
    });
    await act(async () => {
      release?.();
      await first;
    });
    expect(deleteDeviceStory).toHaveBeenCalledTimes(1);
  });

  it("dismisses a terminal status back to idle", async () => {
    vi.mocked(deleteDeviceStory).mockResolvedValue({
      packUuid: PACK_UUID,
      wasPresent: false,
    });
    const { result } = renderHook(() => useDeviceStoryDelete());
    await act(async () => {
      await result.current.triggerDelete(DEVICE_ID, PACK_UUID);
    });
    expect(result.current.status.kind).toBe("deleted");
    act(() => {
      result.current.dismissStatus();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.targetPackUuid).toBeNull();
  });
});
