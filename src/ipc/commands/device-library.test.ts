import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  READ_DEVICE_LIBRARY_TIMEOUT_ERROR,
  READ_DEVICE_LIBRARY_TIMEOUT_MS,
  ReadDeviceLibraryContractDriftError,
  readDeviceLibrary,
} from "./device-library";

const VALID_ID = "0123456789abcdef0123456789abcdef";

const readable = {
  kind: "readable",
  deviceIdentifier: VALID_ID,
  stories: [
    {
      uuid: "00000000-0000-0000-0000-00000000abcd",
      shortId: "0000ABCD",
      hidden: false,
      contentPresent: true,
      alreadyImported: false,
      title: null,
      titleSource: null,
      thumbnail: null,
    },
  ],
};

describe("readDeviceLibrary", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("invokes read_device_library with the camelCase deviceIdentifier argument", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "none" });
    const handle = readDeviceLibrary(VALID_ID);
    await handle.promise;
    expect(invoke).toHaveBeenCalledWith("read_device_library", {
      deviceIdentifier: VALID_ID,
    });
  });

  it("resolves a readable payload preserving the stories", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(readable);
    const dto = await readDeviceLibrary(VALID_ID).promise;
    expect(dto.kind).toBe("readable");
    if (dto.kind === "readable") {
      expect(dto.stories).toHaveLength(1);
      expect(dto.stories[0].shortId).toBe("0000ABCD");
    }
  });

  it("resolves a none payload (device gone between detection and read)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "none" });
    const dto = await readDeviceLibrary(VALID_ID).promise;
    expect(dto.kind).toBe("none");
  });

  it("throws a drift error on an invalid wire shape", async () => {
    const raw = { kind: "readable", deviceIdentifier: "nothex", stories: [] };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const handle = readDeviceLibrary(VALID_ID);
    await expect(handle.promise).rejects.toBeInstanceOf(
      ReadDeviceLibraryContractDriftError,
    );
    await expect(handle.promise).rejects.toMatchObject({ raw });
  });

  it("rejects with the underlying AppError when the backend throws DEVICE_SCAN_FAILED", async () => {
    const appErr = {
      code: "DEVICE_SCAN_FAILED",
      message: "msg",
      userAction: "act",
      details: { source: "fs_read", kind: "not_found" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(appErr);
    await expect(readDeviceLibrary(VALID_ID).promise).rejects.toBe(appErr);
  });

  it("rejects with the timeout sentinel when the backend is silent past the budget", async () => {
    vi.mocked(invoke).mockReturnValueOnce(new Promise(() => undefined));
    const handle = readDeviceLibrary(VALID_ID, 50);
    const observed = handle.promise.catch((e) => e);
    await vi.advanceTimersByTimeAsync(60);
    expect(await observed).toBe(READ_DEVICE_LIBRARY_TIMEOUT_ERROR);
  });

  it("does not reject after cancel() even if the budget elapses", async () => {
    vi.mocked(invoke).mockReturnValueOnce(new Promise(() => undefined));
    const handle = readDeviceLibrary(VALID_ID, 50);
    let settled = false;
    handle.promise.catch(() => {
      settled = true;
    });
    handle.cancel();
    await vi.advanceTimersByTimeAsync(120);
    expect(settled).toBe(false);
  });

  it("exposes a documented timeout default above the Rust budget", () => {
    expect(READ_DEVICE_LIBRARY_TIMEOUT_MS).toBe(5500);
  });
});
