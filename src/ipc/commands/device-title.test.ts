import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  SetDeviceStoryTitleContractDriftError,
  setDeviceStoryTitle,
} from "./device-title";

const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

describe("setDeviceStoryTitle", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls set_device_story_title with the expected payload and returns the stored title", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      title: "Mon histoire",
      source: "user",
    });
    const result = await setDeviceStoryTitle({
      packUuid: PACK_UUID,
      title: "Mon histoire",
    });
    expect(invoke).toHaveBeenCalledWith("set_device_story_title", {
      input: { packUuid: PACK_UUID, title: "Mon histoire" },
    });
    expect(result).toEqual({ title: "Mon histoire", source: "user" });
  });

  it("rejects with the drift error (raw attached) when the payload drifts", async () => {
    const drifted = { title: "x", source: "community" };
    vi.mocked(invoke).mockResolvedValueOnce(drifted);
    const error = await setDeviceStoryTitle({
      packUuid: PACK_UUID,
      title: "x",
    }).catch((err: unknown) => err);
    expect(error).toBeInstanceOf(SetDeviceStoryTitleContractDriftError);
    expect((error as SetDeviceStoryTitleContractDriftError).raw).toBe(drifted);
  });

  it("propagates a Rust AppError rejection untouched (e.g. INVALID_STORY_TITLE)", async () => {
    const appError = {
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre trop long (120 caractères maximum).",
      userAction: "Raccourcis le titre à 120 caractères maximum.",
      details: { cause: "too_long" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(appError);
    await expect(
      setDeviceStoryTitle({ packUuid: PACK_UUID, title: "x".repeat(200) }),
    ).rejects.toBe(appError);
  });

  it("normalizes a non-AppError transport rejection through toAppError", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("ipc transport blew up"));
    const error = await setDeviceStoryTitle({
      packUuid: PACK_UUID,
      title: "x",
    }).catch((err: unknown) => err);
    expect(error).toMatchObject({ code: "UNKNOWN" });
  });

  it("refuses a malformed input client-side without any IPC round-trip", async () => {
    await expect(
      setDeviceStoryTitle({ packUuid: "not-a-uuid", title: "x" }),
    ).rejects.toBeInstanceOf(TypeError);
    await expect(
      setDeviceStoryTitle({ packUuid: PACK_UUID, title: "   " }),
    ).rejects.toBeInstanceOf(TypeError);
    expect(invoke).not.toHaveBeenCalled();
  });
});
