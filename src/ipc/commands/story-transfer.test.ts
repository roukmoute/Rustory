import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";

import {
  discardTransferOutcome,
  readTransferOutcome,
  readTransferState,
  startTransferStory,
  TransferContractDriftError,
} from "./story-transfer";

const DEVICE = "0123456789abcdef0123456789abcdef";
const STORY = "0197a5d0-0000-7000-8000-000000000000";
const JOB = "0197a5d0-0000-7000-8000-0000000000aa";

describe("startTransferStory", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("wraps the input under { input } and returns the acceptance", async () => {
    vi.mocked(invoke).mockResolvedValue({ jobId: JOB, storyId: STORY });
    const accepted = await startTransferStory({
      storyId: STORY,
      deviceIdentifier: DEVICE,
    });
    expect(invoke).toHaveBeenCalledWith("start_transfer_story", {
      input: { storyId: STORY, deviceIdentifier: DEVICE },
    });
    expect(accepted).toEqual({ jobId: JOB, storyId: STORY });
  });

  it("throws a drift error on a bad shape", async () => {
    vi.mocked(invoke).mockResolvedValue({ nope: true });
    await expect(
      startTransferStory({ storyId: STORY, deviceIdentifier: DEVICE }),
    ).rejects.toBeInstanceOf(TransferContractDriftError);
  });

  it("normalizes a non-AppError rejection to an UNKNOWN AppError", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    await expect(
      startTransferStory({ storyId: STORY, deviceIdentifier: DEVICE }),
    ).rejects.toMatchObject({ code: "UNKNOWN" });
  });

  it("passes an AppError rejection through verbatim", async () => {
    const appErr = {
      code: "TRANSFER_FAILED",
      message: "m",
      userAction: "a",
      details: null,
    };
    vi.mocked(invoke).mockRejectedValueOnce(appErr);
    await expect(
      startTransferStory({ storyId: STORY, deviceIdentifier: DEVICE }),
    ).rejects.toBe(appErr);
  });
});

describe("readTransferState", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("wraps the input under { input } and returns the state", async () => {
    vi.mocked(invoke).mockResolvedValue({ kind: "idle" });
    const state = await readTransferState({
      storyId: STORY,
      deviceIdentifier: DEVICE,
    });
    expect(invoke).toHaveBeenCalledWith("read_transfer_state", {
      input: { storyId: STORY, deviceIdentifier: DEVICE },
    });
    expect(state).toEqual({ kind: "idle" });
  });

  it("throws a drift error on a bad shape", async () => {
    vi.mocked(invoke).mockResolvedValue({ kind: "weird" });
    await expect(
      readTransferState({ storyId: STORY, deviceIdentifier: DEVICE }),
    ).rejects.toBeInstanceOf(TransferContractDriftError);
  });
});

describe("readTransferOutcome", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  const outcome = {
    storyId: STORY,
    terminalKind: "retryable",
    cause: "deviceChanged",
    message: "Envoi interrompu : l'appareil connecté a changé.",
    userAction: "Rebranche la Lunii souhaitée puis relance l'envoi.",
    recordedAt: "2026-06-23T00:00:00.000Z",
  };

  it("wraps the input under { input } and returns the remembered outcome", async () => {
    vi.mocked(invoke).mockResolvedValue(outcome);
    const read = await readTransferOutcome({ storyId: STORY });
    expect(invoke).toHaveBeenCalledWith("read_transfer_outcome", {
      input: { storyId: STORY },
    });
    expect(read).toEqual(outcome);
  });

  it("returns null when there is no durable memory", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    await expect(readTransferOutcome({ storyId: STORY })).resolves.toBeNull();
  });

  it("throws a drift error on a bad shape", async () => {
    vi.mocked(invoke).mockResolvedValue({ terminalKind: "weird" });
    await expect(
      readTransferOutcome({ storyId: STORY }),
    ).rejects.toBeInstanceOf(TransferContractDriftError);
  });
});

describe("discardTransferOutcome", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("wraps the input under { input } and resolves void on success", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await expect(
      discardTransferOutcome({ storyId: STORY }),
    ).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith("discard_transfer_outcome", {
      input: { storyId: STORY },
    });
  });

  it("normalizes a rejection to an AppError", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    await expect(discardTransferOutcome({ storyId: STORY })).rejects.toMatchObject(
      { code: "UNKNOWN" },
    );
  });
});
