import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  readStoryValidationTimeoutError,
  READ_STORY_VALIDATION_TIMEOUT_MS,
  ReadStoryValidationContractDriftError,
  readStoryValidation,
} from "./story-validation";

const VALID_ID = "0123456789abcdef0123456789abcdef";
const VALID_STORY_ID = "0197a5d0-0000-7000-8000-000000000000";
const INPUT = { storyId: VALID_STORY_ID, deviceIdentifier: VALID_ID };

const ready = {
  kind: "ready",
  deviceIdentifier: VALID_ID,
  story: { id: VALID_STORY_ID, title: "Mon histoire" },
  verdict: "blocked",
  blockers: [
    {
      axis: "structure",
      cause: "checksumMismatch",
      message: "Les données locales ont changé.",
      userAction: "Restaure une sauvegarde saine.",
    },
  ],
};

describe("readStoryValidation", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("invokes read_story_validation with the input wrapped under { input }", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "noDevice" });
    await readStoryValidation(INPUT).promise;
    expect(invoke).toHaveBeenCalledWith("read_story_validation", {
      input: INPUT,
    });
  });

  it("resolves a ready payload preserving the verdict and blockers", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(ready);
    const dto = await readStoryValidation(INPUT).promise;
    expect(dto.kind).toBe("ready");
    if (dto.kind === "ready") {
      expect(dto.verdict).toBe("blocked");
      expect(dto.blockers).toHaveLength(1);
      expect(dto.blockers[0].userAction).toMatch(/restaure/i);
    }
  });

  it("resolves a noDevice payload (device gone between detection and read)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "noDevice" });
    const dto = await readStoryValidation(INPUT).promise;
    expect(dto.kind).toBe("noDevice");
  });

  it("throws a drift error on an invalid wire shape", async () => {
    const raw = { kind: "ready", deviceIdentifier: "nothex", story: {} };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const handle = readStoryValidation(INPUT);
    await expect(handle.promise).rejects.toBeInstanceOf(
      ReadStoryValidationContractDriftError,
    );
    await expect(handle.promise).rejects.toMatchObject({ raw });
  });

  it("normalizes a non-AppError rejection through toAppError (UNKNOWN)", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("kaboom"));
    await expect(readStoryValidation(INPUT).promise).rejects.toMatchObject({
      code: "UNKNOWN",
    });
  });

  it("passes a normalized AppError rejection through verbatim", async () => {
    const appErr = {
      code: "DEVICE_SCAN_FAILED",
      message: "msg",
      userAction: "act",
      details: { source: "device_changed" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(appErr);
    await expect(readStoryValidation(INPUT).promise).rejects.toBe(appErr);
  });

  it("rejects with the timeout sentinel when the backend is silent past the budget", async () => {
    vi.mocked(invoke).mockReturnValueOnce(new Promise(() => undefined));
    const handle = readStoryValidation(INPUT, 50);
    const observed = handle.promise.catch((e) => e);
    await vi.advanceTimersByTimeAsync(60);
    expect(await observed).toEqual(readStoryValidationTimeoutError());
  });

  it("does not reject after cancel() even if the budget elapses", async () => {
    vi.mocked(invoke).mockReturnValueOnce(new Promise(() => undefined));
    const handle = readStoryValidation(INPUT, 50);
    let settled = false;
    handle.promise.catch(() => {
      settled = true;
    });
    handle.cancel();
    await vi.advanceTimersByTimeAsync(120);
    expect(settled).toBe(false);
  });

  it("exposes a documented timeout default above the Rust budget", () => {
    expect(READ_STORY_VALIDATION_TIMEOUT_MS).toBe(5500);
  });
});
