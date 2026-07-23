import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-send", () => ({
  sendPackToDevice: vi.fn(),
}));

import { sendPackToDevice } from "../../../ipc/commands/device-send";
import { useDevicePackSend } from "./use-device-pack-send";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

const SENT_OUTCOME = {
  kind: "sent" as const,
  packUuid: PACK_UUID,
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
  });

  it("transitions sending → sent and reports to the route", async () => {
    vi.mocked(sendPackToDevice).mockResolvedValueOnce(SENT_OUTCOME);
    const onSent = vi.fn();
    const { result } = renderHook(() => useDevicePackSend({ onSent }));
    await act(async () => {
      await result.current.triggerSend(DEVICE_ID);
    });
    expect(sendPackToDevice).toHaveBeenCalledWith({
      deviceIdentifier: DEVICE_ID,
    });
    expect(result.current.status).toEqual({
      kind: "sent",
      packUuid: PACK_UUID,
      imageCount: 117,
      audioCount: 223,
    });
    expect(onSent).toHaveBeenCalledTimes(1);
    expect(onSent).toHaveBeenCalledWith(SENT_OUTCOME);
  });

  it("settles back to idle on a dismissed picker without calling onSent", async () => {
    vi.mocked(sendPackToDevice).mockResolvedValueOnce({ kind: "cancelled" });
    const onSent = vi.fn();
    const { result } = renderHook(() => useDevicePackSend({ onSent }));
    await act(async () => {
      await result.current.triggerSend(DEVICE_ID);
    });
    // A dismissed native picker is a non-event: no status, no callback.
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(onSent).not.toHaveBeenCalled();
  });

  it("surfaces a failure without calling onSent", async () => {
    vi.mocked(sendPackToDevice).mockRejectedValueOnce(RUST_ERROR);
    const onSent = vi.fn();
    const { result } = renderHook(() => useDevicePackSend({ onSent }));
    await act(async () => {
      await result.current.triggerSend(DEVICE_ID);
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
      first = result.current.triggerSend(DEVICE_ID);
    });
    await act(async () => {
      await result.current.triggerSend(DEVICE_ID);
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
      await result.current.triggerSend(DEVICE_ID);
    });
    expect(result.current.status.kind).toBe("sent");
    act(() => {
      result.current.dismissStatus();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });
});
