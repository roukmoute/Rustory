import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-send", () => ({
  sendPackToDevice: vi.fn(),
}));

import { sendPackToDevice } from "../../../ipc/commands/device-send";
import { useDevicePackSend } from "./use-device-pack-send";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";

const SENT_OUTCOME = {
  packUuid: "abababab-abab-abab-abab-ababfac5562d",
  imageCount: 117,
  audioCount: 223,
};

const RUST_ERROR = {
  code: "DEVICE_WRITE_FAILED" as const,
  message: "Envoi impossible: l'appareil a refusé l'écriture.",
  userAction: "Vérifie que l'appareil est bien connecté puis réessaie.",
  details: { source: "device_write" },
};

describe("useDevicePackSend", () => {
  beforeEach(() => {
    vi.mocked(sendPackToDevice).mockReset();
  });

  it("starts idle", () => {
    const { result } = renderHook(() => useDevicePackSend());
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.targetStoryId).toBeNull();
  });

  it("transitions sending → sent, scopes to the story and reports to the route", async () => {
    vi.mocked(sendPackToDevice).mockResolvedValueOnce(SENT_OUTCOME);
    const onSent = vi.fn();
    const { result } = renderHook(() => useDevicePackSend({ onSent }));
    await act(async () => {
      await result.current.triggerSend(DEVICE_ID, STORY_ID);
    });
    expect(sendPackToDevice).toHaveBeenCalledWith({
      deviceIdentifier: DEVICE_ID,
      storyId: STORY_ID,
    });
    expect(result.current.status).toEqual({
      kind: "sent",
      packUuid: SENT_OUTCOME.packUuid,
      imageCount: 117,
      audioCount: 223,
    });
    expect(result.current.targetStoryId).toBe(STORY_ID);
    expect(onSent).toHaveBeenCalledTimes(1);
    expect(onSent).toHaveBeenCalledWith(SENT_OUTCOME);
  });

  it("surfaces a failure without calling onSent", async () => {
    vi.mocked(sendPackToDevice).mockRejectedValueOnce(RUST_ERROR);
    const onSent = vi.fn();
    const { result } = renderHook(() => useDevicePackSend({ onSent }));
    await act(async () => {
      await result.current.triggerSend(DEVICE_ID, STORY_ID);
    });
    expect(result.current.status).toMatchObject({ kind: "failed" });
    expect(onSent).not.toHaveBeenCalled();
  });

  it("swallows a re-entrant trigger while a send is in flight", async () => {
    let release: (() => void) | null = null;
    vi.mocked(sendPackToDevice).mockImplementation(
      () =>
        new Promise((resolve) => {
          release = () => resolve(SENT_OUTCOME);
        }),
    );
    const { result } = renderHook(() => useDevicePackSend());
    let first!: Promise<void>;
    act(() => {
      first = result.current.triggerSend(DEVICE_ID, STORY_ID);
    });
    await act(async () => {
      await result.current.triggerSend(DEVICE_ID, STORY_ID);
    });
    await act(async () => {
      release?.();
      await first;
    });
    expect(sendPackToDevice).toHaveBeenCalledTimes(1);
  });

  it("dismisses a terminal status back to idle", async () => {
    vi.mocked(sendPackToDevice).mockResolvedValue(SENT_OUTCOME);
    const { result } = renderHook(() => useDevicePackSend());
    await act(async () => {
      await result.current.triggerSend(DEVICE_ID, STORY_ID);
    });
    expect(result.current.status.kind).toBe("sent");
    act(() => {
      result.current.dismissStatus();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.targetStoryId).toBeNull();
  });
});
