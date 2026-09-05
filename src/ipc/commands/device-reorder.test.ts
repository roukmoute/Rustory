import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";

import {
  ReorderDeviceStoriesContractDriftError,
  reorderDeviceStories,
} from "./device-reorder";

const INPUT = {
  deviceIdentifier: "0123456789abcdef0123456789abcdef",
  orderedPackUuids: [
    "11111111-1111-1111-1111-1111aaaaaaaa",
    "22222222-2222-2222-2222-2222bbbbbbbb",
  ],
};

describe("reorderDeviceStories", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("resolves the validated outcome", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ count: 2, changed: true });
    await expect(reorderDeviceStories(INPUT)).resolves.toEqual({ count: 2, changed: true });
    expect(invoke).toHaveBeenCalledWith("reorder_device_stories", { input: INPUT });
  });

  it("refuses a malformed input client-side without a round-trip", async () => {
    await expect(
      reorderDeviceStories({ ...INPUT, orderedPackUuids: ["x"] }),
    ).rejects.toBeInstanceOf(TypeError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("normalizes AppError rejections and flags contract drift", async () => {
    vi.mocked(invoke).mockRejectedValueOnce({
      code: "DEVICE_WRITE_FAILED",
      message: "Réorganisation impossible: la liste des histoires de l'appareil a changé entre-temps.",
      userAction: "Relance la lecture de l'appareil, puis déplace à nouveau l'histoire.",
      details: { source: "reorder_diverged", cause: "diverged" },
    });
    await expect(reorderDeviceStories(INPUT)).rejects.toMatchObject({ code: "DEVICE_WRITE_FAILED" });
    vi.mocked(invoke).mockResolvedValueOnce({ nope: true });
    await expect(reorderDeviceStories(INPUT)).rejects.toBeInstanceOf(
      ReorderDeviceStoriesContractDriftError,
    );
  });
});
