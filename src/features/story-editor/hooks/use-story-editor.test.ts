import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/story", () => ({
  getStoryDetail: vi.fn(),
  saveStory: vi.fn(),
  createStory: vi.fn(),
  recordDraft: vi.fn(),
  discardDraft: vi.fn(),
}));

vi.mock("../../library/hooks/use-library-overview", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("../../library/hooks/use-library-overview")
    >();
  return {
    ...actual,
    invalidateLibraryOverviewCache: vi.fn(),
  };
});

import {
  discardDraft,
  getStoryDetail,
  recordDraft,
  saveStory,
} from "../../../ipc/commands/story";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { useStoryEditor } from "./use-story-editor";
import type { StoryDetailDto } from "../../../shared/ipc-contracts/story";

const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";

function buildDetail(overrides: Partial<StoryDetailDto> = {}): StoryDetailDto {
  return {
    id: STORY_ID,
    title: "Titre initial",
    schemaVersion: 1,
    structureJson: '{"schemaVersion":1,"nodes":[]}',
    contentChecksum: "a".repeat(64),
    createdAt: "2026-04-23T09:00:00.000Z",
    updatedAt: "2026-04-23T09:00:00.000Z",
    ...overrides,
  };
}

async function flushPromises(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(getStoryDetail).mockReset();
  vi.mocked(saveStory).mockReset();
  // Default resolved value so the unmount flush path never crashes on a
  // test that never set up a saveStory mock explicitly. Tests that care
  // about a specific return use `mockResolvedValueOnce` / `mockRejectedValueOnce`.
  vi.mocked(saveStory).mockResolvedValue({
    id: STORY_ID,
    title: "",
    updatedAt: "2026-04-23T00:00:00.000Z",
  });
  // Default mock for `recordDraft`: silently resolve. Tests that care
  // about the call (count, args, rejection) override with mockResolvedValueOnce
  // or mockRejectedValueOnce.
  vi.mocked(recordDraft).mockReset();
  vi.mocked(recordDraft).mockResolvedValue();
  vi.mocked(discardDraft).mockReset();
  vi.mocked(discardDraft).mockResolvedValue();
  vi.mocked(invalidateLibraryOverviewCache).mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useStoryEditor", () => {
  it("loads the detail on mount and transitions loading → ready", async () => {
    const detail = buildDetail();
    vi.mocked(getStoryDetail).mockResolvedValueOnce(detail);

    const { result } = renderHook(() => useStoryEditor(STORY_ID));

    expect(result.current.state.kind).toBe("loading");
    await flushPromises();
    expect(result.current.state).toEqual({
      kind: "ready",
      detail,
      draftTitle: "Titre initial",
      saveStatus: { kind: "idle" },
    });
    expect(getStoryDetail).toHaveBeenCalledWith({ storyId: STORY_ID });
  });

  it("maps null from the backend to kind: not-found", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(null);
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();
    expect(result.current.state.kind).toBe("not-found");
  });

  it("maps an AppError rejection to kind: error preserving the code", async () => {
    const rustError = {
      code: "LIBRARY_INCONSISTENT",
      message: "La bibliothèque locale contient des histoires en double.",
      userAction: "Recharge Rustory pour reconstruire la vue cohérente.",
      details: null,
    };
    vi.mocked(getStoryDetail).mockRejectedValueOnce(rustError);
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();
    if (result.current.state.kind !== "error") {
      throw new Error(`expected error state, got ${result.current.state.kind}`);
    }
    expect(result.current.state.error.code).toBe("LIBRARY_INCONSISTENT");
  });

  it("maps a malformed payload to a LIBRARY_INCONSISTENT error", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce({
      not: "a detail",
    } as unknown as StoryDetailDto);
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();
    if (result.current.state.kind !== "error") {
      throw new Error(`expected error state`);
    }
    expect(result.current.state.error.code).toBe("LIBRARY_INCONSISTENT");
  });

  it("treats an undefined storyId as not-found immediately", () => {
    const { result } = renderHook(() => useStoryEditor(undefined));
    expect(result.current.state.kind).toBe("not-found");
    expect(getStoryDetail).not.toHaveBeenCalled();
  });

  it("leaves saveStatus idle when the draft matches the persisted title after normalization", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("  Titre initial  ");
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus).toEqual({ kind: "idle" });
    expect(saveStory).not.toHaveBeenCalled();
  });

  it("debounces then calls saveStory with the normalized title", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Nouveau",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("  Nouveau  ");
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus).toEqual({ kind: "pending" });
    expect(saveStory).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(499);
    });
    expect(saveStory).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(saveStory).toHaveBeenCalledWith({
      id: STORY_ID,
      title: "Nouveau",
    });
  });

  it("transitions to saved on success and reverts to idle after the visible window", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Final",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Final");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();

    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Final");
    expect(result.current.state.detail.updatedAt).toBe(
      "2026-04-23T10:00:00.000Z",
    );
    expect(result.current.state.saveStatus).toEqual({
      kind: "saved",
      at: "2026-04-23T10:00:00.000Z",
    });

    act(() => {
      vi.advanceTimersByTime(3000);
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus).toEqual({ kind: "idle" });
  });

  it("invalidates the library overview cache exactly once per successful save", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "X",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("X");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();

    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
  });

  it("preserves detail.title and detail.updatedAt on save failure (AC3)", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const rustError = {
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Rustory n'a pas pu enregistrer ta modification.",
      userAction: "Réessaie dans un instant.",
      details: { source: "sqlite_update", kind: "busy" },
    };
    vi.mocked(saveStory).mockRejectedValueOnce(rustError);
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Essai échec");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();

    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Titre initial");
    expect(result.current.state.detail.updatedAt).toBe(
      "2026-04-23T09:00:00.000Z",
    );
    expect(result.current.state.draftTitle).toBe("Essai échec");
    expect(result.current.state.saveStatus).toEqual({
      kind: "failed",
      error: rustError,
      attemptedTitle: "Essai échec",
    });
    // AC3: atomic failure — the library cache is NOT invalidated on
    // failure. The prior persisted state is unchanged, so `/library`
    // still reflects the truth without a refetch.
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("retrySave re-fires the save with the attempted title from the failed state", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory)
      .mockRejectedValueOnce({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "m",
        userAction: "a",
        details: null,
      })
      .mockResolvedValueOnce({
        id: STORY_ID,
        title: "Réessayé",
        updatedAt: "2026-04-23T10:05:00.000Z",
      });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Réessayé");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();

    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus.kind).toBe("failed");

    act(() => {
      result.current.retrySave();
    });
    await flushPromises();

    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Réessayé");
    expect(result.current.state.saveStatus.kind).toBe("saved");
    expect(saveStory).toHaveBeenLastCalledWith({
      id: STORY_ID,
      title: "Réessayé",
    });
  });

  it("resets the debounce on successive keystrokes so only one save fires", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Final",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("A");
    });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    act(() => {
      result.current.setDraftTitle("Final");
    });
    act(() => {
      vi.advanceTimersByTime(499);
    });
    expect(saveStory).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    await flushPromises();
    expect(saveStory).toHaveBeenCalledTimes(1);
    expect(saveStory).toHaveBeenCalledWith({ id: STORY_ID, title: "Final" });
  });

  it("clears the error state on the first keystroke after a failed save", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "m",
      userAction: "a",
      details: null,
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Crash");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();

    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus.kind).toBe("failed");

    // Typing again immediately should drop the failed alert and return to
    // pending (the debounce restarts).
    act(() => {
      result.current.setDraftTitle("Crash+");
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus).toEqual({ kind: "pending" });
  });

  it("typing back to the persisted value after a failed save returns to idle without firing saveStory", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "m",
      userAction: "a",
      details: null,
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Temporaire");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Titre initial");
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus).toEqual({ kind: "idle" });
    expect(saveStory).toHaveBeenCalledTimes(1); // only the failed attempt
  });

  it("subsequent setDraftTitle after a successful save transitions back to pending", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "v1",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("v1");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus.kind).toBe("saved");

    act(() => {
      result.current.setDraftTitle("v2");
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus).toEqual({ kind: "pending" });
  });

  it("flushAutoSave commits a pending change synchronously without waiting the debounce", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Flushed",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Flushed");
    });
    // Only 100ms elapsed — debounce has NOT fired.
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(saveStory).not.toHaveBeenCalled();

    act(() => {
      result.current.flushAutoSave();
    });
    expect(saveStory).toHaveBeenCalledWith({
      id: STORY_ID,
      title: "Flushed",
    });
  });

  it("retry re-runs the initial fetch after an error", async () => {
    vi.mocked(getStoryDetail)
      .mockRejectedValueOnce({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "m",
        userAction: "a",
        details: null,
      })
      .mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();
    expect(result.current.state.kind).toBe("error");

    act(() => {
      result.current.retry();
    });
    await flushPromises();
    expect(result.current.state.kind).toBe("ready");
  });

  it("refetches when storyId changes mid-session", async () => {
    const first = buildDetail({ title: "Premier" });
    const second = buildDetail({
      id: "0197a5d0-0000-7000-8000-999999999999",
      title: "Second",
    });
    vi.mocked(getStoryDetail)
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(second);

    const { result, rerender } = renderHook(
      (props: { id: string }) => useStoryEditor(props.id),
      { initialProps: { id: STORY_ID } },
    );
    await flushPromises();
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Premier");

    rerender({ id: "0197a5d0-0000-7000-8000-999999999999" });
    await flushPromises();
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Second");
  });

  it("unmount with a pending save fires a best-effort save call", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Unsaved",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    const { result, unmount } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Unsaved");
    });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(saveStory).not.toHaveBeenCalled();

    unmount();
    // flushPromises so the fire-and-forget Promise resolves inside the test.
    await flushPromises();
    expect(saveStory).toHaveBeenCalledWith({
      id: STORY_ID,
      title: "Unsaved",
    });
  });

  it("does not paint Enregistré when a save succeeds for a value the user has already moved past", async () => {
    let resolveSave: (v: {
      id: string;
      title: string;
      updatedAt: string;
    }) => void = () => undefined;
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("A");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    // Save is in flight for "A"; user types "AB" before it resolves.
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus.kind).toBe("saving");
    act(() => {
      result.current.setDraftTitle("AB");
    });
    // Now the save for "A" resolves — it committed "A" to the DB, but
    // the draft has moved on to "AB". The chip must NOT flash
    // "Enregistré" for the stale value.
    await act(async () => {
      resolveSave({
        id: STORY_ID,
        title: "A",
        updatedAt: "2026-04-23T10:00:00.000Z",
      });
      await Promise.resolve();
      await Promise.resolve();
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("A");
    expect(result.current.state.saveStatus).toEqual({ kind: "pending" });
  });

  it("invalidates the library cache even when the save ACK arrives after unmount", async () => {
    let resolveSave: (v: {
      id: string;
      title: string;
      updatedAt: string;
    }) => void = () => undefined;
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );

    const { result, unmount } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Après");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    // Save is in flight; unmount the hook (simulating navigate away).
    unmount();
    // The ACK arrives AFTER unmount. Cache invalidation must still
    // happen so the next /library mount reads fresh truth.
    await act(async () => {
      resolveSave({
        id: STORY_ID,
        title: "Après",
        updatedAt: "2026-04-23T10:00:00.000Z",
      });
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
  });

  it("reschedules a debounced save when a prior save committed an older value (no silent pending)", async () => {
    let resolveFirst: (v: {
      id: string;
      title: string;
      updatedAt: string;
    }) => void = () => undefined;
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockResolvedValueOnce({
        id: STORY_ID,
        title: "Final",
        updatedAt: "2026-04-23T10:05:00.000Z",
      });

    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("A");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    // First save is "A", in flight. User types "Final" (new debounce
    // planned).
    act(() => {
      result.current.setDraftTitle("Final");
    });
    // First save resolves with "A". Draft has moved on to "Final" —
    // fireSave stale-path must kick a fresh debounce, not stall in
    // pending.
    await act(async () => {
      resolveFirst({
        id: STORY_ID,
        title: "A",
        updatedAt: "2026-04-23T10:00:00.000Z",
      });
      await Promise.resolve();
      await Promise.resolve();
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("A");
    expect(result.current.state.saveStatus).toEqual({ kind: "pending" });
    // The rescheduled debounce fires another save with "Final",
    // WITHOUT any new keystroke.
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();
    expect(vi.mocked(saveStory)).toHaveBeenLastCalledWith({
      id: STORY_ID,
      title: "Final",
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Final");
    expect(result.current.state.saveStatus.kind).toBe("saved");
  });

  it("retrySave clicked twice rapidly only fires one save (re-entrancy guard)", async () => {
    let resolveRetry: (v: {
      id: string;
      title: string;
      updatedAt: string;
    }) => void = () => undefined;
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory)
      .mockRejectedValueOnce({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "m",
        userAction: "a",
        details: null,
      })
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveRetry = resolve;
          }),
      );

    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Rejoué");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await flushPromises();
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus.kind).toBe("failed");

    // Double-click: two synchronous invocations of retrySave before the
    // first response arrives. `saveInFlightRef` flips synchronously in
    // the first `fireSave`, so the second call early-returns. Exactly
    // one retry in flight — no duplicate IPC, no stale-success race.
    const callsBefore = vi.mocked(saveStory).mock.calls.length;
    act(() => {
      result.current.retrySave();
      result.current.retrySave();
    });
    const callsAfter = vi.mocked(saveStory).mock.calls.length;
    expect(callsAfter - callsBefore).toBe(1);
    // Now resolve the in-flight retry. With one save in flight and the
    // matching draft, the hook transitions to `saved`.
    await act(async () => {
      resolveRetry({
        id: STORY_ID,
        title: "Rejoué",
        updatedAt: "2026-04-24T10:00:00.000Z",
      });
      await Promise.resolve();
      await Promise.resolve();
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus.kind).toBe("saved");
    expect(result.current.state.detail.title).toBe("Rejoué");
  });

  it("survives a StrictMode-style double mount without dispatching stale state", async () => {
    // React StrictMode mounts → unmounts → mounts a component in dev.
    // The `mountedRef` + `activeCallRef` pair must neutralize any
    // first-mount response so it cannot clobber the second-mount state.
    vi.mocked(getStoryDetail)
      .mockResolvedValueOnce(buildDetail({ title: "Premier mount" }))
      .mockResolvedValueOnce(buildDetail({ title: "Second mount" }));

    const { result, unmount } = renderHook(() => useStoryEditor(STORY_ID));
    // Unmount before the first fetch resolves — simulates StrictMode.
    unmount();
    await flushPromises();

    // Mount again (fresh `renderHook`) — the second fetch resolves and
    // must be the one reflected. The first fetch's result (now an
    // "orphan" promise) must never touch state.
    const { result: result2 } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();
    if (result2.current.state.kind !== "ready") throw new Error("ready");
    expect(result2.current.state.detail.title).toBe("Second mount");
    // `result` was unmounted — its state snapshot is frozen.
    expect(result.current.state.kind).toBe("loading");
  });

  it("flushAutoSave fires a save even while a prior save is in flight so the latest draft wins", async () => {
    let resolveFirst: (v: {
      id: string;
      title: string;
      updatedAt: string;
    }) => void = () => undefined;
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockResolvedValueOnce({
        id: STORY_ID,
        title: "Final",
        updatedAt: "2026-04-23T10:05:00.000Z",
      });

    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      result.current.setDraftTitle("Intermédiaire");
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.saveStatus.kind).toBe("saving");

    // User types a new value then clicks "Retour" (flush) before the
    // first save resolves. Flush MUST still fire a save for "Final".
    act(() => {
      result.current.setDraftTitle("Final");
    });
    act(() => {
      result.current.flushAutoSave();
    });
    expect(vi.mocked(saveStory)).toHaveBeenCalledTimes(2);
    expect(vi.mocked(saveStory)).toHaveBeenLastCalledWith({
      id: STORY_ID,
      title: "Final",
    });

    // The stale first response now arrives — must be ignored (superseded).
    await act(async () => {
      resolveFirst({
        id: STORY_ID,
        title: "Intermédiaire",
        updatedAt: "2026-04-23T10:00:00.000Z",
      });
      await Promise.resolve();
      await Promise.resolve();
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    // After the second save resolves, detail.title is "Final", not
    // "Intermédiaire".
    expect(result.current.state.detail.title).toBe("Final");
  });

  it("does not overwrite a fresher response with an older in-flight call (storyId change race)", async () => {
    const first = buildDetail({ title: "Ancien" });
    const second = buildDetail({
      id: "0197a5d0-0000-7000-8000-aaaaaaaaaaaa",
      title: "Récent",
    });
    // The first promise is still pending when we switch storyId.
    let resolveFirst: (d: StoryDetailDto) => void = () => undefined;
    vi.mocked(getStoryDetail)
      .mockImplementationOnce(
        () =>
          new Promise<StoryDetailDto | null>((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockResolvedValueOnce(second);

    const { result, rerender } = renderHook(
      (props: { id: string }) => useStoryEditor(props.id),
      { initialProps: { id: STORY_ID } },
    );
    rerender({ id: "0197a5d0-0000-7000-8000-aaaaaaaaaaaa" });
    await flushPromises();
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Récent");

    // Now resolve the first request — it must not clobber the "Récent" state.
    await act(async () => {
      resolveFirst(first);
      await Promise.resolve();
      await Promise.resolve();
    });
    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Récent");
  });
});

describe("useStoryEditor — recovery draft buffering", () => {
  it("setDraftTitle schedules a recordDraft with debounce 150 ms", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("New keystroke");
    });
    expect(recordDraft).not.toHaveBeenCalled();

    // Advance just past the 150 ms record-draft debounce. The autosave
    // 500 ms timer is independent and does not fire here.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(149);
    });
    expect(recordDraft).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2);
    });

    expect(recordDraft).toHaveBeenCalledWith({
      storyId: STORY_ID,
      draftTitle: "New keystroke",
    });
    expect(saveStory).not.toHaveBeenCalled();
  });

  it("setDraftTitle equal to persisted title does NOT call recordDraft", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("Titre initial");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(500);
    });
    expect(recordDraft).not.toHaveBeenCalled();
  });

  it("recordDraft failure is silent and does not affect the autosave state machine", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(recordDraft).mockRejectedValueOnce({
      code: "RECOVERY_DRAFT_UNAVAILABLE",
      message: "fail",
      userAction: "retry",
      details: null,
    });
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Saved",
      updatedAt: "2026-04-25T12:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("Saved");
    });
    // Trip the record_draft failure first.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(150);
    });
    // Then the autosave fires and succeeds.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
      await Promise.resolve();
      await Promise.resolve();
    });

    if (result.current.state.kind !== "ready") throw new Error("ready");
    // The autosave reached `saved` despite the recordDraft failure —
    // the buffer is best-effort.
    expect(result.current.state.saveStatus.kind === "saved").toBe(true);
  });

  it("successive keystrokes coalesce into a single recordDraft after the debounce", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("a");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });
    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("ab");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });
    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("abc");
    });
    // 150 ms after the LAST keystroke, only one record fires with the
    // most recent value.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(150);
    });
    expect(recordDraft).toHaveBeenCalledTimes(1);
    expect(recordDraft).toHaveBeenLastCalledWith({
      storyId: STORY_ID,
      draftTitle: "abc",
    });
  });

  it("typing past the debounce eventually fires the autosave AND a record per pause", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Saved",
      updatedAt: "2026-04-25T12:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("Saved");
    });
    // Past 150 ms: record fires.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(150);
    });
    expect(recordDraft).toHaveBeenCalledTimes(1);
    // Past 500 ms total since keystroke: autosave fires.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(saveStory).toHaveBeenCalledTimes(1);
  });
});

describe("useStoryEditor — auto-discard on return-to-persisted", () => {
  it("fires discardDraft when the user types back to the persisted value after a record was scheduled", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    // Type something different — schedules recordDraft (sets the
    // pending-draft flag) and the autosave debounce.
    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("Modifié");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(150);
    });
    expect(recordDraft).toHaveBeenCalledTimes(1);

    // Now type back to the persisted value BEFORE the autosave fires.
    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("Titre initial");
    });
    // P6: discardDraft is dispatched via `queueMicrotask` to keep the
    // setState updater pure (StrictMode double-fire safety). Drain the
    // microtask queue once before asserting.
    await act(async () => {
      await Promise.resolve();
    });
    expect(discardDraft).toHaveBeenCalledWith({ storyId: STORY_ID });
  });

  it("does NOT fire discardDraft when the user types back without ever scheduling a record", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    // Same value typed again — never planned a record because the
    // value matched the persisted truth from the start.
    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("Titre initial");
    });
    expect(discardDraft).not.toHaveBeenCalled();
  });

  it("does NOT fire discardDraft after a successful autosave (DELETE already happened atomically in Rust)", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Saved",
      updatedAt: "2026-04-25T12:00:00.000Z",
    });
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("Saved");
    });
    // Let both timers (record 150 ms + autosave 500 ms) fire.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(700);
      await Promise.resolve();
      await Promise.resolve();
    });
    // Now the user types back to the previously-persisted "Saved",
    // which is the new persisted truth post-autosave. The pending-draft
    // flag was cleared in the autosave success branch, so discardDraft
    // must NOT fire.
    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.setDraftTitle("Saved");
    });
    expect(discardDraft).not.toHaveBeenCalled();
  });
});

describe("useStoryEditor — reloadDetailFromOutput", () => {
  it("patches detail.title and resets saveStatus to idle", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.reloadDetailFromOutput({
        id: STORY_ID,
        title: "Recovered",
        updatedAt: "2026-04-25T12:00:00.000Z",
      });
    });

    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Recovered");
    expect(result.current.state.detail.updatedAt).toBe(
      "2026-04-25T12:00:00.000Z",
    );
    expect(result.current.state.draftTitle).toBe("Recovered");
    expect(result.current.state.saveStatus.kind).toBe("idle");
  });

  it("does nothing when the output id does not match the current detail", async () => {
    vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
    const { result } = renderHook(() => useStoryEditor(STORY_ID));
    await flushPromises();

    act(() => {
      if (result.current.state.kind !== "ready") throw new Error("ready");
      result.current.reloadDetailFromOutput({
        id: "different",
        title: "Recovered",
        updatedAt: "2026-04-25T12:00:00.000Z",
      });
    });

    if (result.current.state.kind !== "ready") throw new Error("ready");
    expect(result.current.state.detail.title).toBe("Titre initial");
  });
});
