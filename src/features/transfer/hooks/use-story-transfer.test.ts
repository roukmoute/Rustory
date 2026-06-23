import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/story-transfer", async () => {
  const actual = await vi.importActual<
    typeof import("../../../ipc/commands/story-transfer")
  >("../../../ipc/commands/story-transfer");
  return {
    ...actual,
    startTransferStory: vi.fn(),
    readTransferState: vi.fn(),
  };
});

vi.mock("../../../ipc/events/job-events", () => ({
  subscribeJobEvents: vi.fn(),
}));

import {
  readTransferState,
  startTransferStory,
  TransferContractDriftError,
} from "../../../ipc/commands/story-transfer";
import { subscribeJobEvents } from "../../../ipc/events/job-events";
import type { JobSubscription } from "../../../ipc/events/job-events";
import { useJobShell } from "../../../shell/state/job-shell-store";

import { useStoryTransfer } from "./use-story-transfer";

const STORY = "0197a5d0-0000-7000-8000-000000000000";
const DEVICE = "0123456789abcdef0123456789abcdef";

let subscriptions: JobSubscription[] = [];
let unsubscribeSpies: Array<() => void> = [];

function lastSubscription(): JobSubscription {
  const sub = subscriptions[subscriptions.length - 1];
  if (!sub) throw new Error("no subscription captured");
  return sub;
}

function lastUnsubscribe(): () => void {
  const fn = unsubscribeSpies[unsubscribeSpies.length - 1];
  if (!fn) throw new Error("no unsubscribe captured");
  return fn;
}

const progress = (
  phase: "preflight" | "transfer" | "verify",
  sequence: number,
) => ({
  jobId: "j1",
  jobType: "transfer_story",
  targetStoryId: STORY,
  phase,
  progress: null,
  sequence,
  message: null,
});

const completed = (sequence: number) => ({
  jobId: "j1",
  jobType: "transfer_story",
  targetStoryId: STORY,
  sequence,
});

const failed = (sequence: number) => ({
  jobId: "j1",
  jobType: "transfer_story",
  targetStoryId: STORY,
  sequence,
  errorCode: "TRANSFER_FAILED",
  errorMessage: "Transfert interrompu : l'appareil connecté a changé.",
  userAction: "Rebranche la Lunii puis relance l'envoi.",
});

// A `job:failed` whose device-mutation signal marks the write as having STARTED
// (AC2 — the device may hold a partial copy).
const failedIncomplete = (sequence: number) => ({
  ...failed(sequence),
  errorMessage: "L'appareil peut contenir une copie partielle.",
  userAction: "Relance l'envoi pour rétablir un état sûr.",
  completeness: "incomplete" as const,
  cause: "writeRejected" as const,
});

// A `job:failed` carrying the verify `partial` verdict (état partiel) — distinct
// from a write-phase `incomplete`.
const failedPartial = (sequence: number) => ({
  ...failed(sequence),
  errorMessage:
    "Envoi dans un état partiel : certains éléments n'ont pas pu être confirmés sur la Lunii.",
  userAction: "Relance l'envoi pour rétablir un état sûr.",
  verifyVerdict: "partial" as const,
});

// The AC2 summary lines, composed in Rust (here scripted as ready-made strings).
const SUMMARY = {
  changed: "« Mon histoire » est maintenant sur la Lunii.",
  unchanged: "2 autres histoires de l'appareil restent inchangées.",
};

// A transfer `job:completed` carrying the verified summary ON the terminal (F1).
const completedVerified = (sequence: number) => ({
  ...completed(sequence),
  summary: SUMMARY,
});

// The authoritative re-read confirming the verify success, carrying the AC2
// summary lines (used by the catch-up / standalone re-read path).
const verifiedState = {
  kind: "verified" as const,
  deviceIdentifier: DEVICE,
  story: { id: STORY, title: "Mon histoire" },
  summary: SUMMARY,
};

describe("useStoryTransfer", () => {
  beforeEach(() => {
    subscriptions = [];
    unsubscribeSpies = [];
    vi.mocked(startTransferStory).mockReset();
    vi.mocked(readTransferState).mockReset();
    vi.mocked(subscribeJobEvents).mockReset();
    vi.mocked(subscribeJobEvents).mockImplementation((sub) => {
      subscriptions.push(sub);
      const unsubscribe = vi.fn();
      unsubscribeSpies.push(unsubscribe);
      return unsubscribe;
    });
    useJobShell.setState({ activeJobs: new Map() });
    // Default catch-up re-read: idle, so it does not interfere with the live
    // phase progression unless a test scripts a terminal first.
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });
  });

  it("starts idle; send() with an empty id is a no-op", () => {
    const { result } = renderHook(() => useStoryTransfer());
    expect(result.current.state.kind).toBe("idle");
    act(() => result.current.send("", DEVICE));
    act(() => result.current.send(STORY, ""));
    expect(startTransferStory).not.toHaveBeenCalled();
  });

  it("optimistically enters transferring for the targeted story", () => {
    vi.mocked(startTransferStory).mockReturnValue(new Promise(() => undefined));
    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    expect(result.current.state).toEqual({
      kind: "transferring",
      storyId: STORY,
      progress: null,
      phase: null,
    });
  });

  it("drives the live phases (incl. verify) then settles verified FROM the completed event (F1)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    // The re-read folds to idle (the pre-write identifier no longer resolves the
    // device after `.pi` changed) — yet the success must NOT be lost: it is settled
    // straight from the terminal event's summary.
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());

    act(() => lastSubscription().onProgress(progress("preflight", 1)));
    expect(result.current.state.kind).toBe("transferring");
    act(() => lastSubscription().onProgress(progress("transfer", 2)));
    expect(result.current.state.kind).toBe("transferring");
    // The FINAL verify phase stays a transient `transferring` carrying phase=verify
    // (the panel renders the distinct "écriture effectuée — vérification à venir").
    act(() => lastSubscription().onProgress(progress("verify", 3)));
    expect(result.current.state).toMatchObject({
      kind: "transferring",
      phase: "verify",
    });

    act(() => lastSubscription().onCompleted(completedVerified(4)));
    await waitFor(() => expect(result.current.state.kind).toBe("verified"));
    if (result.current.state.kind === "verified") {
      expect(result.current.state.storyId).toBe(STORY);
      // The summary lines are rendered verbatim (composed in Rust).
      expect(result.current.state.summary.changed).toContain("Mon histoire");
      expect(result.current.state.summary.unchanged).toMatch(
        /2 autres histoires/i,
      );
    }
  });

  it("maps a job:failed with verifyVerdict 'partial' to the partial terminal, distinct from incomplete/retryable (AC3)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    act(() => lastSubscription().onFailed(failedPartial(4)));

    await waitFor(() => expect(result.current.state.kind).toBe("partial"));
    expect(result.current.state.kind).not.toBe("incomplete");
    expect(result.current.state.kind).not.toBe("retryable");
    if (result.current.state.kind === "partial") {
      expect(result.current.state.storyId).toBe(STORY);
      expect(result.current.state.message).toMatch(/état partiel/i);
      expect(result.current.state.userAction).toMatch(/rétablir un état sûr/i);
    }
  });

  it("never claims verified when the completed re-read cannot confirm the device (folds to idle) — honest recoverable instead (F1/AC3)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    // Both the catch-up and the onCompleted re-read fold to idle (device gone):
    // the device cannot confirm the write, so success must NOT be claimed.
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    act(() => lastSubscription().onCompleted(completed(3)));

    await waitFor(() => expect(result.current.state.kind).toBe("retryable"));
    expect(result.current.state.kind).not.toBe("verified");
    if (result.current.state.kind === "retryable") {
      expect(result.current.state.storyId).toBe(STORY);
      // Honest "non confirmé" — never any success vocabulary.
      expect(result.current.state.message).toMatch(/non confirmé/i);
      expect(result.current.state.message).not.toMatch(/transférée et vérifiée/i);
      expect(result.current.state.userAction).toMatch(/relance/i);
    }
  });

  it("the catch-up re-read reconciles to the verified terminal when the event is missed", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    // The write+verify finished before the subscription registered: NO event fires,
    // but the catch-up re-read still reaches the verified terminal.
    vi.mocked(readTransferState).mockResolvedValue(verifiedState);

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("verified"));
  });

  it("a late progress event never regresses a settled terminal", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    vi.mocked(readTransferState).mockResolvedValue(verifiedState);

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("verified"));
    expect(lastUnsubscribe()).toHaveBeenCalled();
    act(() => lastSubscription().onProgress(progress("verify", 9)));
    expect(result.current.state.kind).toBe("verified");
  });

  it("pins the authoritative re-read to the targeted device (C1)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(readTransferState).toHaveBeenCalled());
    // The re-read must prove the pack on the TARGETED device, not on any
    // writable Lunii connected at the terminal.
    expect(readTransferState).toHaveBeenCalledWith({
      storyId: STORY,
      deviceIdentifier: DEVICE,
    });
  });

  it("ignores a second re-read once the job is settled (C2)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    // Catch-up re-read confirms `verified` (settles the job); the later
    // onCompleted re-read folds to idle and MUST be ignored — never flipping the
    // already-settled terminal nor re-running a redundant scan.
    vi.mocked(readTransferState)
      .mockResolvedValueOnce(verifiedState)
      .mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("verified"));

    act(() => lastSubscription().onCompleted(completed(3)));
    await waitFor(() => expect(readTransferState).toHaveBeenCalledTimes(2));
    // The settled terminal is NOT flipped by the second (idle) re-read.
    expect(result.current.state.kind).toBe("verified");
  });

  it("settles a job:failed from the event without a re-read, never flipping to verified (F1/AC2)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    // The catch-up re-read returns idle; the failure terminal must come straight
    // from the EVENT — a `job:failed` is never re-read into a false success
    // (a pack present after a post-promote fsync failure is the `incomplete` case).
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    await waitFor(() => expect(readTransferState).toHaveBeenCalledTimes(1));
    act(() => lastSubscription().onFailed(failed(2)));

    await waitFor(() => expect(result.current.state.kind).toBe("retryable"));
    expect(result.current.state.kind).not.toBe("verified");
    // No extra re-read on the failure path — only the earlier catch-up ran.
    expect(readTransferState).toHaveBeenCalledTimes(1);
    if (result.current.state.kind === "retryable") {
      expect(result.current.state.storyId).toBe(STORY);
      expect(result.current.state.message).toMatch(/l'appareil connecté a changé/i);
    }
  });

  it("preserves the event's failure message when the re-read folds to idle (device gone)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    act(() => lastSubscription().onFailed(failed(2)));

    await waitFor(() => expect(result.current.state.kind).toBe("retryable"));
    if (result.current.state.kind === "retryable") {
      expect(result.current.state.message).toMatch(/l'appareil connecté a changé/i);
    }
  });

  it("maps a job:failed with completeness 'incomplete' to the incomplete terminal, distinct from retryable (AC2)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    act(() => lastSubscription().onFailed(failedIncomplete(2)));

    await waitFor(() => expect(result.current.state.kind).toBe("incomplete"));
    expect(result.current.state.kind).not.toBe("retryable");
    if (result.current.state.kind === "incomplete") {
      expect(result.current.state.storyId).toBe(STORY);
      // The structured cause is carried into the current state (F4/AC3).
      expect(result.current.state.cause).toBe("writeRejected");
      expect(result.current.state.message).toMatch(/copie partielle/i);
      expect(result.current.state.userAction).toMatch(/rétablir un état sûr/i);
    }
  });

  it("dismiss() returns to idle while keeping retry() available, draft untouched (AC3)", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    vi.mocked(readTransferState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    act(() => lastSubscription().onFailed(failedIncomplete(2)));
    await waitFor(() => expect(result.current.state.kind).toBe("incomplete"));

    // Abandonner → back to idle.
    act(() => result.current.dismiss());
    expect(result.current.state.kind).toBe("idle");

    // The last request is preserved, so retry() / send() stay possible afterwards.
    act(() => result.current.retry());
    await waitFor(() => expect(startTransferStory).toHaveBeenCalledTimes(2));
    expect(vi.mocked(startTransferStory).mock.lastCall?.[0]).toEqual({
      storyId: STORY,
      deviceIdentifier: DEVICE,
    });
  });

  it("surfaces an error when start_transfer_story rejects with a drift", async () => {
    vi.mocked(startTransferStory).mockRejectedValue(
      new TransferContractDriftError("nope", { raw: {} }),
    );
    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("TRANSFER_FAILED");
      expect(result.current.state.storyId).toBe(STORY);
    }
  });

  it("surfaces a normalized error when start_transfer_story rejects with an AppError", async () => {
    vi.mocked(startTransferStory).mockRejectedValue({
      code: "DEVICE_SCAN_FAILED",
      message: "Identifiant d'appareil invalide.",
      userAction: "Relance la détection.",
      details: null,
    });
    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("unsubscribes from the job events on unmount", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    const { result, unmount } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    unmount();
    expect(lastUnsubscribe()).toHaveBeenCalled();
  });

  it("does NOT reset when used across renders (it is not selection-keyed)", () => {
    vi.mocked(startTransferStory).mockReturnValue(new Promise(() => undefined));
    const { result, rerender } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    expect(result.current.state.kind).toBe("transferring");
    rerender();
    expect(result.current.state).toEqual({
      kind: "transferring",
      storyId: STORY,
      progress: null,
      phase: null,
    });
  });

  it("retry() re-runs the last request", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    const { result } = renderHook(() => useStoryTransfer());
    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(startTransferStory).toHaveBeenCalledTimes(1));
    act(() => result.current.retry());
    await waitFor(() => expect(startTransferStory).toHaveBeenCalledTimes(2));
    expect(vi.mocked(startTransferStory).mock.lastCall?.[0]).toEqual({
      storyId: STORY,
      deviceIdentifier: DEVICE,
    });
  });

  it("ignores late events from a superseded transfer", async () => {
    vi.mocked(startTransferStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    const { result } = renderHook(() => useStoryTransfer());

    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscriptions.length).toBe(1));
    const firstSub = subscriptions[0];

    act(() => result.current.send(STORY, DEVICE));
    await waitFor(() => expect(subscriptions.length).toBe(2));

    // A late progress event from the FIRST (superseded) job must be ignored.
    act(() => firstSub.onProgress(progress("transfer", 9)));
    expect(result.current.state.kind).toBe("transferring");
  });
});
