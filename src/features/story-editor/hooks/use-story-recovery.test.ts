import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// `vi.mock` is hoisted to the top of the file, so any reference inside
// the factory must be self-contained. We define the drift-error classes
// inline.
vi.mock("../../../ipc/commands/story", () => {
  class ApplyRecoveryContractDriftError extends Error {
    raw: unknown;
    constructor(message: string, options: { raw: unknown }) {
      super(message);
      this.name = "ApplyRecoveryContractDriftError";
      this.raw = options.raw;
    }
  }
  class ReadRecoverableDraftContractDriftError extends Error {
    raw: unknown;
    constructor(message: string, options: { raw: unknown }) {
      super(message);
      this.name = "ReadRecoverableDraftContractDriftError";
      this.raw = options.raw;
    }
  }
  return {
    readRecoverableDraft: vi.fn(),
    applyRecovery: vi.fn(),
    discardDraft: vi.fn(),
    ApplyRecoveryContractDriftError,
    ReadRecoverableDraftContractDriftError,
  };
});

import {
  applyRecovery,
  discardDraft,
  readRecoverableDraft,
} from "../../../ipc/commands/story";
import type { AppError } from "../../../shared/errors/app-error";
import type {
  RecoverableDraft,
  UpdateStoryOutput,
} from "../../../shared/ipc-contracts/story";

import { useStoryRecovery } from "./use-story-recovery";

const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";

const recoverableDraft: RecoverableDraft = {
  kind: "recoverable",
  storyId: STORY_ID,
  draftTitle: "Buffered",
  draftAt: "2026-04-25T12:00:00.000Z",
  persistedTitle: "Persisted",
};

const updateOutput: UpdateStoryOutput = {
  id: STORY_ID,
  title: "Buffered",
  updatedAt: "2026-04-25T12:00:01.000Z",
};

const recoveryError: AppError = {
  code: "RECOVERY_DRAFT_UNAVAILABLE",
  message: "Récupération indisponible.",
  userAction: "Vérifie le disque local.",
  details: { source: "sqlite_select" },
};

beforeEach(() => {
  vi.mocked(readRecoverableDraft).mockReset();
  vi.mocked(applyRecovery).mockReset();
  vi.mocked(discardDraft).mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useStoryRecovery — initial load", () => {
  it("loads on mount and transitions loading → none when backend has no draft", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce({ kind: "none" });
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    expect(result.current.state.kind).toBe("loading");
    await waitFor(() => expect(result.current.state.kind).toBe("none"));
  });

  it("loads on mount and transitions loading → recoverable when backend returns one", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));
    if (result.current.state.kind !== "recoverable") throw new Error();
    expect(result.current.state.draft.draftTitle).toBe("Buffered");
    expect(result.current.state.draft.persistedTitle).toBe("Persisted");
  });

  it("transitions loading → error on AppError rejection during initial read", async () => {
    vi.mocked(readRecoverableDraft).mockRejectedValueOnce(recoveryError);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind !== "error") throw new Error();
    expect(result.current.state.error.code).toBe("RECOVERY_DRAFT_UNAVAILABLE");
    expect(result.current.state.draft).toBeNull();
  });

  it("recoverable with empty draftTitle is rendered as a recoverable state", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce({
      ...recoverableDraft,
      draftTitle: "",
    });
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));
    if (result.current.state.kind !== "recoverable") throw new Error();
    expect(result.current.state.draft.draftTitle).toBe("");
  });

  it("settles to none when storyId is undefined", async () => {
    const { result } = renderHook(() => useStoryRecovery(undefined));
    expect(result.current.state.kind).toBe("none");
    expect(readRecoverableDraft).not.toHaveBeenCalled();
  });

  it("transitioning storyId from defined to undefined supersedes an in-flight read", async () => {
    let resolveLater: (value: RecoverableDraft) => void = () => {};
    vi.mocked(readRecoverableDraft).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveLater = resolve;
      }),
    );
    const { result, rerender } = renderHook(
      ({ id }: { id: string | undefined }) => useStoryRecovery(id),
      { initialProps: { id: STORY_ID as string | undefined } },
    );
    rerender({ id: undefined });
    expect(result.current.state.kind).toBe("none");

    // The first read resolves AFTER the rerender; the active-call
    // guard must drop it instead of flipping back to recoverable.
    act(() => {
      resolveLater(recoverableDraft);
    });
    expect(result.current.state.kind).toBe("none");
  });
});

describe("useStoryRecovery — apply", () => {
  it("transitions recoverable → applying → none on success and calls onApplied", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    vi.mocked(applyRecovery).mockResolvedValueOnce(updateOutput);
    const onApplied = vi.fn();
    const { result } = renderHook(() => useStoryRecovery(STORY_ID, { onApplied }));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.apply();
    });

    await waitFor(() => expect(result.current.state.kind).toBe("none"));
    expect(applyRecovery).toHaveBeenCalledWith({ storyId: STORY_ID });
    expect(onApplied).toHaveBeenCalledWith(updateOutput);
  });

  it("transitions recoverable → error on AppError rejection and preserves the draft", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    vi.mocked(applyRecovery).mockRejectedValueOnce(recoveryError);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.apply();
    });

    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind !== "error") throw new Error();
    expect(result.current.state.error.code).toBe("RECOVERY_DRAFT_UNAVAILABLE");
    expect(result.current.state.draft).not.toBeNull();
    expect(result.current.state.draft?.draftTitle).toBe("Buffered");
  });

  it("preserves recoverable state on INVALID_STORY_TITLE so the UI can offer Discard", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    const invalidTitleError: AppError = {
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre contient des caractères non autorisés",
      userAction: "Supprime les sauts de ligne, tabulations et caractères invisibles.",
      details: { source: "recovery_draft_invalid", id: STORY_ID },
    };
    vi.mocked(applyRecovery).mockRejectedValueOnce(invalidTitleError);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.apply();
    });

    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind !== "error") throw new Error();
    expect(result.current.state.draft).not.toBeNull();
    // Caller can call discard() from this state, the hook keeps the
    // draft attached to the error variant.
  });

  it("re-entrancy: a second apply while the first is in-flight is a no-op", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    let resolveFirst: (value: UpdateStoryOutput) => void = () => {};
    vi.mocked(applyRecovery).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveFirst = resolve;
      }),
    );
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.apply();
      result.current.apply();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("applying"));
    expect(applyRecovery).toHaveBeenCalledTimes(1);

    act(() => {
      resolveFirst(updateOutput);
    });
    await waitFor(() => expect(result.current.state.kind).toBe("none"));
  });

  it("synchronous throw from applyRecovery does not strand writeInFlightRef", async () => {
    // A drift / encoder error can throw synchronously before the
    // Promise is even constructed. Without try/finally the hook would
    // keep `writeInFlightRef.current = true` forever and refuse every
    // subsequent click.
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    vi.mocked(applyRecovery).mockImplementationOnce(() => {
      throw new Error("synchronous boom");
    });
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.apply();
    });
    // The synchronous throw must transition into `error` state with
    // the draft preserved AND release the in-flight lock.
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind !== "error") throw new Error();
    expect(result.current.state.draft).not.toBeNull();

    // A second apply must NOT be blocked by a stranded in-flight flag.
    vi.mocked(applyRecovery).mockResolvedValueOnce(updateOutput);
    act(() => {
      result.current.apply();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("none"));
  });

  it("synchronous throw from discardDraft does not strand writeInFlightRef", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    vi.mocked(discardDraft).mockImplementationOnce(() => {
      throw new Error("sync boom");
    });
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.discard();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("error"));

    // Subsequent discard succeeds — the lock was released.
    vi.mocked(discardDraft).mockResolvedValueOnce(undefined);
    act(() => {
      result.current.discard();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("none"));
  });

  it("apply called from error state re-fires the apply with the preserved draft", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    vi.mocked(applyRecovery)
      .mockRejectedValueOnce(recoveryError)
      .mockResolvedValueOnce(updateOutput);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.apply();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("error"));

    act(() => {
      result.current.apply();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("none"));
    expect(applyRecovery).toHaveBeenCalledTimes(2);
  });
});

describe("useStoryRecovery — discard", () => {
  it("transitions recoverable → none and calls discardDraft once", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    vi.mocked(discardDraft).mockResolvedValueOnce(undefined);
    const onDiscarded = vi.fn();
    const { result } = renderHook(() => useStoryRecovery(STORY_ID, { onDiscarded }));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.discard();
    });

    await waitFor(() => expect(result.current.state.kind).toBe("none"));
    // P18: the hook forwards the observed `draftAt` so Rust runs a
    // CAS — a concurrent `record_draft` between observation and
    // click is preserved instead of dropped.
    expect(discardDraft).toHaveBeenCalledWith({
      storyId: STORY_ID,
      expectedDraftAt: recoverableDraft.draftAt,
    });
    expect(onDiscarded).toHaveBeenCalledWith(STORY_ID);
  });

  it("transitions recoverable → error on AppError rejection and preserves the draft", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    vi.mocked(discardDraft).mockRejectedValueOnce(recoveryError);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.discard();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind !== "error") throw new Error();
    expect(result.current.state.draft).not.toBeNull();
  });
});

describe("useStoryRecovery — retry", () => {
  it("retry re-fires the initial readRecoverableDraft after an error", async () => {
    vi.mocked(readRecoverableDraft)
      .mockRejectedValueOnce(recoveryError)
      .mockResolvedValueOnce(recoverableDraft);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));

    act(() => {
      result.current.retry();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));
    expect(readRecoverableDraft).toHaveBeenCalledTimes(2);
  });
});

describe("useStoryRecovery — lifecycle", () => {
  it("changing storyId mid-flight aborts the previous load and fetches the new one", async () => {
    let resolveFirst: (value: RecoverableDraft) => void = () => {};
    vi.mocked(readRecoverableDraft)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveFirst = resolve;
        }),
      )
      .mockResolvedValueOnce({ kind: "none" });

    const { result, rerender } = renderHook(
      ({ id }: { id: string }) => useStoryRecovery(id),
      { initialProps: { id: "first" } },
    );
    rerender({ id: "second" });

    // The first read resolves AFTER the second read started; the
    // active-call guard must drop it.
    act(() => {
      resolveFirst(recoverableDraft);
    });
    await waitFor(() => expect(result.current.state.kind).toBe("none"));
    expect(readRecoverableDraft).toHaveBeenCalledTimes(2);
  });

  it("unmount mid-load supersedes the response (no setState after unmount)", async () => {
    let resolveLater: (value: RecoverableDraft) => void = () => {};
    vi.mocked(readRecoverableDraft).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveLater = resolve;
      }),
    );
    const { unmount } = renderHook(() => useStoryRecovery(STORY_ID));
    unmount();
    // No throw, no warning when the promise resolves after unmount.
    act(() => {
      resolveLater(recoverableDraft);
    });
  });
});

describe("useStoryRecovery — race with autosave clearing the draft", () => {
  it("none state survives a successful autosave that cleared the draft mid-mount", async () => {
    // The hook reads the draft just AFTER `update_story` consumed it
    // in the same transaction. The backend returns `none`. The hook
    // must not flicker into `recoverable` — it lands on `none` directly.
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce({ kind: "none" });
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("none"));
  });
});

describe("useStoryRecovery — dismissReadError", () => {
  it("transitions error+draft=null → none", async () => {
    vi.mocked(readRecoverableDraft).mockRejectedValueOnce(recoveryError);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind !== "error") throw new Error();
    expect(result.current.state.draft).toBeNull();

    act(() => {
      result.current.dismissReadError();
    });
    expect(result.current.state.kind).toBe("none");
  });

  it("is a no-op when state is recoverable", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.dismissReadError();
    });
    // State unchanged — only initial-read errors are dismissable this way.
    expect(result.current.state.kind).toBe("recoverable");
  });

  it("is a no-op when state is error WITH a draft (apply/discard failure)", async () => {
    vi.mocked(readRecoverableDraft).mockResolvedValueOnce(recoverableDraft);
    vi.mocked(applyRecovery).mockRejectedValueOnce(recoveryError);
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    await waitFor(() => expect(result.current.state.kind).toBe("recoverable"));

    act(() => {
      result.current.apply();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind !== "error") throw new Error();
    expect(result.current.state.draft).not.toBeNull();

    act(() => {
      result.current.dismissReadError();
    });
    // Draft preserved: dismissReadError must not eat a recoverable
    // payload that the user can still Apply / Discard.
    expect(result.current.state.kind).toBe("error");
  });

  it("supersedes any in-flight initial read so a late resolution does not clobber none", async () => {
    let resolveLater: (value: typeof recoveryError) => void = () => {};
    vi.mocked(readRecoverableDraft).mockReturnValueOnce(
      new Promise<never>((_resolve, reject) => {
        resolveLater = (e) => reject(e);
      }),
    );
    const { result } = renderHook(() => useStoryRecovery(STORY_ID));
    // Force the error state via retry so we have a stable starting point.
    vi.mocked(readRecoverableDraft).mockRejectedValueOnce(recoveryError);
    act(() => {
      result.current.retry();
    });
    await waitFor(() => expect(result.current.state.kind).toBe("error"));

    act(() => {
      result.current.dismissReadError();
    });
    expect(result.current.state.kind).toBe("none");
    // The original superseded promise resolves now — must NOT flip
    // state back to error.
    act(() => {
      resolveLater(recoveryError);
    });
    expect(result.current.state.kind).toBe("none");
  });
});
