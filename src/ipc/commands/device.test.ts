import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  READ_CONNECTED_LUNII_TIMEOUT_ERROR,
  READ_CONNECTED_LUNII_TIMEOUT_MS,
  ReadConnectedLuniiContractDriftError,
  readConnectedLunii,
} from "./device";

const validSupported = {
  kind: "supported",
  family: "lunii",
  firmwareCohort: "origineV1",
  metadataFormatVersion: 3,
  deviceIdentifier: "0123456789abcdef0123456789abcdef",
  supportedOperations: {
    readLibrary: true,
    inspectStory: true,
    importStory: true,
    writeStory: false,
    deleteStory: false,
  },
};

describe("readConnectedLunii", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("calls the read_connected_lunii command with no arguments", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "none" });
    const handle = readConnectedLunii();
    await handle.promise;
    expect(invoke).toHaveBeenCalledWith("read_connected_lunii");
  });

  it("resolves with kind=none when the backend returns no device", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "none" });
    const handle = readConnectedLunii();
    const dto = await handle.promise;
    expect(dto.kind).toBe("none");
  });

  it("resolves with a complete supported payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(validSupported);
    const handle = readConnectedLunii();
    const dto = await handle.promise;
    expect(dto.kind).toBe("supported");
    if (dto.kind === "supported") {
      expect(dto.firmwareCohort).toBe("origineV1");
      expect(dto.supportedOperations.writeStory).toBe(false);
    }
  });

  it("resolves with an unsupported payload preserving the typed reason", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      kind: "unsupported",
      reason: "metadataUnsupported",
      firmwareHint: "metadata_v99",
    });
    const handle = readConnectedLunii();
    const dto = await handle.promise;
    expect(dto.kind).toBe("unsupported");
    if (dto.kind === "unsupported") {
      expect(dto.reason).toBe("metadataUnsupported");
      expect(dto.firmwareHint).toBe("metadata_v99");
    }
  });

  it("resolves with an ambiguous payload preserving the candidate count", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      kind: "ambiguous",
      candidateCount: 2,
    });
    const handle = readConnectedLunii();
    const dto = await handle.promise;
    expect(dto.kind).toBe("ambiguous");
    if (dto.kind === "ambiguous") {
      expect(dto.candidateCount).toBe(2);
    }
  });

  it("throws a ReadConnectedLuniiContractDriftError when backend returns an invalid kind", async () => {
    const raw = { kind: "weird" };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const handle = readConnectedLunii();
    await expect(handle.promise).rejects.toBeInstanceOf(
      ReadConnectedLuniiContractDriftError,
    );
    await expect(handle.promise).rejects.toMatchObject({ raw });
  });

  it("throws a drift error when supported payload lacks supportedOperations", async () => {
    const { supportedOperations: _drop, ...incomplete } = validSupported;
    void _drop;
    vi.mocked(invoke).mockResolvedValueOnce(incomplete);
    const handle = readConnectedLunii();
    await expect(handle.promise).rejects.toBeInstanceOf(
      ReadConnectedLuniiContractDriftError,
    );
  });

  it("rejects with the underlying AppError when backend throws DEVICE_SCAN_FAILED", async () => {
    const appErr = {
      code: "DEVICE_SCAN_FAILED",
      message: "msg",
      userAction: "act",
      details: { source: "fs_read", kind: "permission_denied" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(appErr);
    const handle = readConnectedLunii();
    await expect(handle.promise).rejects.toBe(appErr);
  });

  it("rejects with the timeout sentinel when backend silent past the budget", async () => {
    // Make invoke return a promise that never resolves.
    vi.mocked(invoke).mockReturnValueOnce(new Promise(() => undefined));
    const handle = readConnectedLunii(50);
    const observed = handle.promise.catch((e) => e);
    await vi.advanceTimersByTimeAsync(60);
    const err = await observed;
    expect(err).toBe(READ_CONNECTED_LUNII_TIMEOUT_ERROR);
  });

  it("does not reject after cancel() even if the budget elapses", async () => {
    vi.mocked(invoke).mockReturnValueOnce(new Promise(() => undefined));
    const handle = readConnectedLunii(50);
    let settled = false;
    handle.promise.catch(() => {
      settled = true;
    });
    handle.cancel();
    await vi.advanceTimersByTimeAsync(120);
    expect(settled).toBe(false);
  });

  it("exposes the documented timeout default", () => {
    expect(READ_CONNECTED_LUNII_TIMEOUT_MS).toBe(4500);
  });
});
