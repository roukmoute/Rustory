import { beforeEach, describe, expect, it, vi } from "vitest";

// A fake Channel matching the shape the facade uses (`new Channel()` +
// `.onmessage = fn`). The mocked `invoke` reaches into the passed channel and
// drives its `onmessage` to simulate the Rust-side progress stream. Defined
// INSIDE the factory since `vi.mock` is hoisted above module-scope bindings.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage: ((msg: number) => void) | null = null;
  },
}));

/** Structural view of the fake channel the mocked `invoke` receives. */
type ChannelLike = { onmessage?: ((msg: number) => void) | null };

import { invoke } from "@tauri-apps/api/core";

import {
  SendPackToDeviceContractDriftError,
  sendPackToDevice,
} from "./device-send";

const VALID_INPUT = {
  deviceIdentifier: "0123456789abcdef0123456789abcdef",
  storyId: "0197a5d0-0000-7000-8000-000000000000",
};
const VALID_OUTCOME = {
  packUuid: "abababab-abab-abab-abab-ababfac5562d",
  imageCount: 117,
  audioCount: 223,
};

describe("sendPackToDevice", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("resolves the validated outcome", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(VALID_OUTCOME);
    await expect(sendPackToDevice(VALID_INPUT)).resolves.toEqual(VALID_OUTCOME);
  });

  it("forwards the streamed percent, clamping out-of-range and dropping non-finite ticks", async () => {
    vi.mocked(invoke).mockImplementationOnce(async (_cmd, args) => {
      const channel = (args as { onProgress?: ChannelLike } | undefined)
        ?.onProgress;
      channel?.onmessage?.(10);
      channel?.onmessage?.(150); // out of range → clamped to 99
      channel?.onmessage?.(Number.NaN); // non-finite → dropped
      channel?.onmessage?.(0.6); // rounded to 1
      return VALID_OUTCOME;
    });
    const seen: number[] = [];
    const out = await sendPackToDevice(VALID_INPUT, (p) => seen.push(p));
    expect(out).toEqual(VALID_OUTCOME);
    expect(seen).toEqual([10, 99, 1]);
  });

  it("rejects a malformed input client-side before any round-trip", async () => {
    await expect(
      sendPackToDevice({ deviceIdentifier: "nope", storyId: "nope" }),
    ).rejects.toBeInstanceOf(TypeError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("throws a drift error on a payload that does not match the contract", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ packUuid: "not-a-uuid" });
    await expect(sendPackToDevice(VALID_INPUT)).rejects.toBeInstanceOf(
      SendPackToDeviceContractDriftError,
    );
  });
});
