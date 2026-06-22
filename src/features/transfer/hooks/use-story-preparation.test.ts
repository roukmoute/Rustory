import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/story-preparation", async () => {
  const actual = await vi.importActual<
    typeof import("../../../ipc/commands/story-preparation")
  >("../../../ipc/commands/story-preparation");
  return {
    ...actual,
    startPrepareStory: vi.fn(),
    readPreparationState: vi.fn(),
  };
});

vi.mock("../../../ipc/events/job-events", () => ({
  subscribeJobEvents: vi.fn(),
}));

import {
  PreparationContractDriftError,
  readPreparationState,
  startPrepareStory,
} from "../../../ipc/commands/story-preparation";
import { subscribeJobEvents } from "../../../ipc/events/job-events";
import type { JobSubscription } from "../../../ipc/events/job-events";
import { useJobShell } from "../../../shell/state/job-shell-store";

import { useStoryPreparation } from "./use-story-preparation";

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

const progress = (phase: "preflight" | "prepare", sequence: number) => ({
  jobId: "j1",
  jobType: "prepare_story",
  targetStoryId: STORY,
  phase,
  progress: null,
  sequence,
  message: null,
});

const completed = (sequence: number) => ({
  jobId: "j1",
  jobType: "prepare_story",
  targetStoryId: STORY,
  sequence,
});

const failed = (sequence: number) => ({
  jobId: "j1",
  jobType: "prepare_story",
  targetStoryId: STORY,
  sequence,
  errorCode: "PREPARATION_FAILED",
  errorMessage: "Préparation interrompue : l'appareil connecté a changé.",
  userAction: "Rebranche la Lunii puis relance la préparation.",
});

describe("useStoryPreparation", () => {
  beforeEach(() => {
    subscriptions = [];
    unsubscribeSpies = [];
    vi.mocked(startPrepareStory).mockReset();
    vi.mocked(readPreparationState).mockReset();
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
    vi.mocked(readPreparationState).mockResolvedValue({ kind: "idle" });
  });

  it("starts idle; prepare() with an empty id is a no-op", () => {
    const { result } = renderHook(() => useStoryPreparation());
    expect(result.current.state.kind).toBe("idle");
    act(() => result.current.prepare("", DEVICE));
    act(() => result.current.prepare(STORY, ""));
    expect(startPrepareStory).not.toHaveBeenCalled();
  });

  it("optimistically enters preflight for the targeted story", () => {
    vi.mocked(startPrepareStory).mockReturnValue(new Promise(() => undefined));
    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    expect(result.current.state).toEqual({ kind: "preflight", storyId: STORY });
  });

  it("drives live phases then reaches prepared via the terminal re-read", async () => {
    vi.mocked(startPrepareStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    vi.mocked(readPreparationState)
      .mockResolvedValueOnce({ kind: "idle" }) // catch-up — no-op
      .mockResolvedValueOnce({
        kind: "prepared",
        deviceIdentifier: DEVICE,
        story: { id: STORY, title: "Mon histoire" },
        targetCohort: "origine_v1",
      });

    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());

    act(() => lastSubscription().onProgress(progress("preflight", 1)));
    expect(result.current.state.kind).toBe("preflight");
    act(() => lastSubscription().onProgress(progress("prepare", 2)));
    expect(result.current.state.kind).toBe("preparing");

    act(() => lastSubscription().onCompleted(completed(3)));
    await waitFor(() => expect(result.current.state.kind).toBe("prepared"));
    if (result.current.state.kind === "prepared") {
      expect(result.current.state.storyId).toBe(STORY);
    }
  });

  it("F3: the catch-up re-read reconciles to the terminal when the event is missed", async () => {
    vi.mocked(startPrepareStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    // The job finished before the subscription registered: NO event is fired,
    // but the catch-up re-read still reaches the terminal.
    vi.mocked(readPreparationState).mockResolvedValue({
      kind: "prepared",
      deviceIdentifier: DEVICE,
      story: { id: STORY, title: "Mon histoire" },
      targetCohort: "origine_v1",
    });

    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("prepared"));
  });

  it("P1: a late progress event never regresses a settled terminal", async () => {
    vi.mocked(startPrepareStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    // The catch-up re-read settles `prepared` right after subscribing.
    vi.mocked(readPreparationState).mockResolvedValue({
      kind: "prepared",
      deviceIdentifier: DEVICE,
      story: { id: STORY, title: "Mon histoire" },
      targetCohort: "origine_v1",
    });

    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("prepared"));
    // Settling a terminal stops the live subscription...
    expect(lastUnsubscribe()).toHaveBeenCalled();
    // ...and a late `job:progress` (queued before the subscription) is ignored.
    act(() => lastSubscription().onProgress(progress("prepare", 9)));
    expect(result.current.state.kind).toBe("prepared");
  });

  it("maps a failed terminal to retryable with the re-read blockers", async () => {
    vi.mocked(startPrepareStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    vi.mocked(readPreparationState)
      .mockResolvedValueOnce({ kind: "idle" }) // catch-up — no-op
      .mockResolvedValueOnce({
        kind: "retryable",
        story: { id: STORY, title: "Mon histoire" },
        cause: "preflightNotPassing",
        message: "La préparation ne peut pas démarrer.",
        userAction: "Corrige les points signalés.",
        blockers: [
          {
            axis: "structure",
            cause: "titleInvalid",
            message: "Le titre enregistré n'est pas valide.",
            userAction: "Renomme l'histoire.",
          },
        ],
      });

    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    act(() => lastSubscription().onFailed(failed(2)));

    await waitFor(() => expect(result.current.state.kind).toBe("retryable"));
    if (result.current.state.kind === "retryable") {
      expect(result.current.state.storyId).toBe(STORY);
      expect(result.current.state.message).toMatch(/ne peut pas démarrer/i);
      expect(result.current.state.blockers).toHaveLength(1);
    }
  });

  it("preserves the event's failure message when the re-read folds to idle (device gone)", async () => {
    vi.mocked(startPrepareStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    // Both the catch-up AND the terminal re-read fold to idle (device left).
    vi.mocked(readPreparationState).mockResolvedValue({ kind: "idle" });

    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    act(() => lastSubscription().onFailed(failed(2)));

    await waitFor(() => expect(result.current.state.kind).toBe("retryable"));
    if (result.current.state.kind === "retryable") {
      expect(result.current.state.message).toMatch(/l'appareil connecté a changé/i);
      expect(result.current.state.blockers).toEqual([]);
    }
  });

  it("surfaces an error when start_prepare_story rejects with a drift", async () => {
    vi.mocked(startPrepareStory).mockRejectedValue(
      new PreparationContractDriftError("nope", { raw: {} }),
    );
    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("PREPARATION_FAILED");
      expect(result.current.state.storyId).toBe(STORY);
    }
  });

  it("surfaces a normalized error when start_prepare_story rejects with an AppError", async () => {
    vi.mocked(startPrepareStory).mockRejectedValue({
      code: "DEVICE_SCAN_FAILED",
      message: "Identifiant d'appareil invalide.",
      userAction: "Relance la détection.",
      details: null,
    });
    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("unsubscribes from the job events on unmount", async () => {
    vi.mocked(startPrepareStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    const { result, unmount } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(subscribeJobEvents).toHaveBeenCalled());
    unmount();
    expect(lastUnsubscribe()).toHaveBeenCalled();
  });

  it("does NOT reset when used across renders (it is not selection-keyed)", async () => {
    vi.mocked(startPrepareStory).mockReturnValue(new Promise(() => undefined));
    const { result, rerender } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    expect(result.current.state.kind).toBe("preflight");
    // A re-render (e.g. the library selection changed) must NOT drop the job.
    rerender();
    expect(result.current.state).toEqual({ kind: "preflight", storyId: STORY });
  });

  it("retry() re-runs the last request", async () => {
    vi.mocked(startPrepareStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    const { result } = renderHook(() => useStoryPreparation());
    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(startPrepareStory).toHaveBeenCalledTimes(1));
    act(() => result.current.retry());
    await waitFor(() => expect(startPrepareStory).toHaveBeenCalledTimes(2));
    expect(vi.mocked(startPrepareStory).mock.lastCall?.[0]).toEqual({
      storyId: STORY,
      deviceIdentifier: DEVICE,
    });
  });

  it("ignores late events from a superseded preparation", async () => {
    vi.mocked(startPrepareStory).mockResolvedValue({ jobId: "j1", storyId: STORY });
    const { result } = renderHook(() => useStoryPreparation());

    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(subscriptions.length).toBe(1));
    const firstSub = subscriptions[0];

    act(() => result.current.prepare(STORY, DEVICE));
    await waitFor(() => expect(subscriptions.length).toBe(2));

    // A late progress event from the FIRST (superseded) job must be ignored.
    act(() => firstSub.onProgress(progress("prepare", 9)));
    expect(result.current.state.kind).toBe("preflight");
  });
});
