import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-reorder", () => ({
  reorderDeviceStories: vi.fn(),
}));

import { reorderDeviceStories } from "../../../ipc/commands/device-reorder";

import { useDeviceStoryReorder } from "./use-device-story-reorder";

const DEVICE = "0123456789abcdef0123456789abcdef";
const A = "11111111-1111-1111-1111-1111aaaaaaaa";
const B = "22222222-2222-2222-2222-2222bbbbbbbb";
const C = "33333333-3333-3333-3333-3333cccccccc";

describe("useDeviceStoryReorder", () => {
  beforeEach(() => {
    vi.mocked(reorderDeviceStories).mockReset();
  });

  it("writes the moved order to the device, then asks for a re-read", async () => {
    vi.mocked(reorderDeviceStories).mockResolvedValueOnce({ count: 3, changed: true });
    const onReordered = vi.fn();
    const { result } = renderHook(() => useDeviceStoryReorder({ onReordered }));
    await act(async () => {
      await result.current.move(DEVICE, [A, B, C], C, -1);
    });
    expect(reorderDeviceStories).toHaveBeenCalledWith({
      deviceIdentifier: DEVICE,
      orderedPackUuids: [A, C, B],
    });
    expect(onReordered).toHaveBeenCalledTimes(1);
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("does nothing for a move with no room and swallows re-entrant moves", async () => {
    let resolve: (v: unknown) => void = () => undefined;
    vi.mocked(reorderDeviceStories).mockImplementationOnce(
      () => new Promise((r) => { resolve = r; }) as never,
    );
    const { result } = renderHook(() => useDeviceStoryReorder());
    await act(async () => {
      await result.current.move(DEVICE, [A, B], A, -1);
    });
    expect(reorderDeviceStories).not.toHaveBeenCalled();
    let first: Promise<void> = Promise.resolve();
    act(() => {
      first = result.current.move(DEVICE, [A, B], B, -1);
    });
    await waitFor(() => expect(result.current.status).toEqual({ kind: "moving", packUuid: B }));
    await act(async () => {
      await result.current.move(DEVICE, [A, B], A, 1);
    });
    expect(reorderDeviceStories).toHaveBeenCalledTimes(1);
    resolve({ count: 2, changed: true });
    await act(async () => {
      await first;
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("keeps a refusal as a dismissable failure", async () => {
    vi.mocked(reorderDeviceStories).mockRejectedValueOnce({
      code: "DEVICE_WRITE_FAILED",
      message: "Réorganisation impossible: l'appareil a refusé l'écriture.",
      userAction: "Vérifie que l'appareil est bien connecté puis réessaie.",
      details: { source: "reorder_rejected", cause: "write_rejected" },
    });
    const { result } = renderHook(() => useDeviceStoryReorder());
    await act(async () => {
      await result.current.move(DEVICE, [A, B], B, -1);
    });
    expect(result.current.status.kind).toBe("failed");
    act(() => result.current.dismissStatus());
    expect(result.current.status).toEqual({ kind: "idle" });
  });
});
