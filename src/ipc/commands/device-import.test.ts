import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  ImportDeviceStoryContractDriftError,
  importDeviceStory,
} from "./device-import";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

const validOutcome = {
  story: {
    id: "0197a5d0-0000-7000-8000-000000000000",
    title: "Histoire de ma Lunii (FAC5562D)",
  },
  packShortId: "FAC5562D",
  importedAt: "2026-06-10T12:00:00.000Z",
};

describe("importDeviceStory", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls the import_device_story command with the expected payload shape", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(validOutcome);
    const result = await importDeviceStory({
      deviceIdentifier: DEVICE_ID,
      packUuid: PACK_UUID,
    });
    expect(invoke).toHaveBeenCalledWith("import_device_story", {
      input: { deviceIdentifier: DEVICE_ID, packUuid: PACK_UUID },
    });
    expect(result.story.title).toBe("Histoire de ma Lunii (FAC5562D)");
    expect(result.packShortId).toBe("FAC5562D");
  });

  it("rejects with the drift error (raw attached) when the payload drifts", async () => {
    const drifted = { ...validOutcome, mountPath: "/leak" };
    vi.mocked(invoke).mockResolvedValueOnce(drifted);
    const error = await importDeviceStory({
      deviceIdentifier: DEVICE_ID,
      packUuid: PACK_UUID,
    }).catch((err: unknown) => err);
    expect(error).toBeInstanceOf(ImportDeviceStoryContractDriftError);
    expect((error as ImportDeviceStoryContractDriftError).raw).toBe(drifted);
  });

  it("propagates a Rust AppError rejection untouched", async () => {
    const appError = {
      code: "IMPORT_FAILED",
      message: "Copie impossible: l'appareil connecté a changé.",
      userAction: "Rebranche la Lunii souhaitée puis réessaie la copie.",
      details: { source: "device_changed" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(appError);
    await expect(
      importDeviceStory({ deviceIdentifier: DEVICE_ID, packUuid: PACK_UUID }),
    ).rejects.toBe(appError);
  });

  it("normalizes a non-AppError transport rejection through toAppError", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("ipc transport blew up"));
    const error = await importDeviceStory({
      deviceIdentifier: DEVICE_ID,
      packUuid: PACK_UUID,
    }).catch((err: unknown) => err);
    expect(error).toMatchObject({ code: "UNKNOWN" });
    expect((error as { userAction: string | null }).userAction).not.toBeNull();
  });

  it("refuses a malformed input client-side without any IPC round-trip", async () => {
    await expect(
      importDeviceStory({
        deviceIdentifier: "NOT-HEX",
        packUuid: PACK_UUID,
      }),
    ).rejects.toBeInstanceOf(TypeError);
    await expect(
      importDeviceStory({
        deviceIdentifier: DEVICE_ID,
        packUuid: "not-a-uuid",
      }),
    ).rejects.toBeInstanceOf(TypeError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("sets no frontend timer — the promise settles only with the IPC call", async () => {
    // Rust owns the 300 s budget. A pending invoke must keep the facade
    // pending (no synthetic timeout rejection like the library read).
    vi.useFakeTimers();
    try {
      let settled = false;
      vi.mocked(invoke).mockReturnValueOnce(
        new Promise(() => {
          /* never settles */
        }),
      );
      void importDeviceStory({
        deviceIdentifier: DEVICE_ID,
        packUuid: PACK_UUID,
      }).finally(() => {
        settled = true;
      });
      await vi.advanceTimersByTimeAsync(600_000);
      expect(settled).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});
