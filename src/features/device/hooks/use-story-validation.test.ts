import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

vi.mock("../../../ipc/commands/story-validation", async () => {
  const actual = await vi.importActual<
    typeof import("../../../ipc/commands/story-validation")
  >("../../../ipc/commands/story-validation");
  return {
    ...actual,
    readStoryValidation: vi.fn(),
  };
});

import {
  readStoryValidation,
  ReadStoryValidationContractDriftError,
} from "../../../ipc/commands/story-validation";

import { useStoryValidation } from "./use-story-validation";

const ID_A = "0123456789abcdef0123456789abcdef";
const ID_B = "fedcba9876543210fedcba9876543210";
const STORY = "0197a5d0-0000-7000-8000-000000000000";

const ready = (verdict: "presumedTransferable" | "toFix" | "blocked") => ({
  kind: "ready" as const,
  deviceIdentifier: ID_A,
  story: { id: STORY, title: "Mon histoire" },
  verdict,
  blockers: [],
});

function mockHandle(promise: Promise<unknown>): {
  promise: Promise<unknown>;
  cancel: () => void;
} {
  return { promise, cancel: vi.fn() };
}

describe("useStoryValidation", () => {
  beforeEach(() => {
    vi.mocked(readStoryValidation).mockReset();
  });

  it("stays idle and issues no IPC when there is no validable pair", () => {
    const a = renderHook(() => useStoryValidation(null, ID_A));
    expect(a.result.current.state.kind).toBe("idle");
    const b = renderHook(() => useStoryValidation(STORY, null));
    expect(b.result.current.state.kind).toBe("idle");
    expect(readStoryValidation).not.toHaveBeenCalled();
  });

  it("loads then transitions to ready with the composed verdict", async () => {
    vi.mocked(readStoryValidation).mockReturnValueOnce(
      mockHandle(Promise.resolve(ready("presumedTransferable"))) as never,
    );
    const { result } = renderHook(() => useStoryValidation(STORY, ID_A));
    expect(result.current.state.kind).toBe("loading");
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    if (result.current.state.kind === "ready") {
      expect(result.current.state.verdict).toBe("presumedTransferable");
      expect(result.current.state.blockers).toEqual([]);
      expect(result.current.state.storyTitle).toBe("Mon histoire");
    }
  });

  it("surfaces a recoverable device-changed error when the device folds to noDevice", async () => {
    vi.mocked(readStoryValidation).mockReturnValueOnce(
      mockHandle(Promise.resolve({ kind: "noDevice" })) as never,
    );
    const { result } = renderHook(() => useStoryValidation(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.message).toMatch(/l'appareil a changé/i);
    }
  });

  it("phrases the device-changed next gesture family-correct (FLAM vs Lunii verbatim)", async () => {
    vi.mocked(readStoryValidation).mockReturnValueOnce(
      mockHandle(Promise.resolve({ kind: "noDevice" })) as never,
    );
    const flam = renderHook(() => useStoryValidation(STORY, ID_A, "flam"));
    await waitFor(() => expect(flam.result.current.state.kind).toBe("error"));
    if (flam.result.current.state.kind === "error") {
      expect(flam.result.current.state.error.userAction).toBe(
        "Vérifie que l'appareil est toujours branché puis réessaie la validation.",
      );
    }
    flam.unmount();

    vi.mocked(readStoryValidation).mockReturnValueOnce(
      mockHandle(Promise.resolve({ kind: "noDevice" })) as never,
    );
    const lunii = renderHook(() => useStoryValidation(STORY, ID_A, "lunii"));
    await waitFor(() => expect(lunii.result.current.state.kind).toBe("error"));
    if (lunii.result.current.state.kind === "error") {
      expect(lunii.result.current.state.error.userAction).toBe(
        "Vérifie que la Lunii est toujours branchée puis réessaie la validation.",
      );
    }
  });

  it("surfaces a recoverable error when the read rejects", async () => {
    const err = {
      code: "DEVICE_SCAN_FAILED",
      message: "Validation indisponible.",
      userAction: "Réessaie.",
      details: { source: "device_changed" },
    };
    vi.mocked(readStoryValidation).mockReturnValueOnce(
      mockHandle(Promise.reject(err)) as never,
    );
    const { result } = renderHook(() => useStoryValidation(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("converts a drift error into a typed DEVICE_SCAN_FAILED AppError", async () => {
    vi.mocked(readStoryValidation).mockReturnValueOnce(
      mockHandle(
        Promise.reject(new ReadStoryValidationContractDriftError("nope", { raw: {} })),
      ) as never,
    );
    const { result } = renderHook(() => useStoryValidation(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.code).toBe("DEVICE_SCAN_FAILED");
    }
  });

  it("refresh() re-reads the current pair", async () => {
    vi.mocked(readStoryValidation)
      .mockReturnValueOnce(mockHandle(Promise.resolve(ready("blocked"))) as never)
      .mockReturnValueOnce(
        mockHandle(Promise.resolve(ready("presumedTransferable"))) as never,
      );
    const { result } = renderHook(() => useStoryValidation(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    act(() => {
      result.current.refresh();
    });
    await waitFor(() => {
      expect(
        result.current.state.kind === "ready" &&
          result.current.state.verdict === "presumedTransferable",
      ).toBe(true);
    });
  });

  it("re-reads when the device identifier changes (another Lunii plugged)", async () => {
    vi.mocked(readStoryValidation).mockReturnValue(
      mockHandle(Promise.resolve(ready("blocked"))) as never,
    );
    const { rerender } = renderHook(
      ({ id }) => useStoryValidation(STORY, id),
      { initialProps: { id: ID_A as string | null } },
    );
    await waitFor(() =>
      expect(vi.mocked(readStoryValidation).mock.calls.length).toBeGreaterThan(0),
    );
    const before = vi.mocked(readStoryValidation).mock.calls.length;
    rerender({ id: ID_B });
    await waitFor(() =>
      expect(vi.mocked(readStoryValidation).mock.calls.length).toBeGreaterThan(
        before,
      ),
    );
    expect(vi.mocked(readStoryValidation).mock.lastCall?.[0]).toEqual({
      storyId: STORY,
      deviceIdentifier: ID_B,
    });
  });

  it("clears to idle when either side goes null", async () => {
    vi.mocked(readStoryValidation).mockReturnValue(
      mockHandle(Promise.resolve(ready("blocked"))) as never,
    );
    const { result, rerender } = renderHook(
      ({ sid }: { sid: string | null }) => useStoryValidation(sid, ID_A),
      { initialProps: { sid: STORY as string | null } },
    );
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));
    rerender({ sid: null });
    expect(result.current.state.kind).toBe("idle");
  });

  it("cancels the in-flight read on unmount", () => {
    const cancel = vi.fn();
    vi.mocked(readStoryValidation).mockReturnValueOnce({
      promise: new Promise(() => undefined),
      cancel,
    } as never);
    const { unmount } = renderHook(() => useStoryValidation(STORY, ID_A));
    unmount();
    expect(cancel).toHaveBeenCalled();
  });

  it("ignores a stale response from a superseded read", async () => {
    let resolveFirst: ((v: unknown) => void) | undefined;
    vi.mocked(readStoryValidation)
      .mockReturnValueOnce(
        mockHandle(
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
        ) as never,
      )
      .mockReturnValueOnce(
        mockHandle(Promise.resolve(ready("presumedTransferable"))) as never,
      );

    const { result } = renderHook(() => useStoryValidation(STORY, ID_A));
    act(() => {
      result.current.refresh();
    });
    await waitFor(() => {
      expect(
        result.current.state.kind === "ready" &&
          result.current.state.verdict === "presumedTransferable",
      ).toBe(true);
    });

    // Late resolve of the superseded first read MUST be ignored.
    act(() => {
      resolveFirst?.(ready("blocked"));
    });
    expect(
      result.current.state.kind === "ready" &&
        result.current.state.verdict === "presumedTransferable",
    ).toBe(true);
  });

  it("always re-reads fresh on remount — no cached verdict (decision surface)", async () => {
    vi.mocked(readStoryValidation).mockReturnValue(
      mockHandle(Promise.resolve(ready("blocked"))) as never,
    );
    const first = renderHook(() => useStoryValidation(STORY, ID_A));
    await waitFor(() => expect(first.result.current.state.kind).toBe("ready"));
    first.unmount();

    const second = renderHook(() => useStoryValidation(STORY, ID_A));
    expect(second.result.current.state.kind).toBe("loading");
    await waitFor(() => expect(second.result.current.state.kind).toBe("ready"));
  });

  it("surfaces device-changed when the ready payload's deviceIdentifier does not match", async () => {
    vi.mocked(readStoryValidation).mockReturnValueOnce(
      mockHandle(
        Promise.resolve({ ...ready("blocked"), deviceIdentifier: ID_B }),
      ) as never,
    );
    const { result } = renderHook(() => useStoryValidation(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
    if (result.current.state.kind === "error") {
      expect(result.current.state.error.message).toMatch(/l'appareil a changé/i);
    }
  });

  it("surfaces device-changed when the ready payload's story.id does not match", async () => {
    vi.mocked(readStoryValidation).mockReturnValueOnce(
      mockHandle(
        Promise.resolve({
          ...ready("blocked"),
          story: { id: "0197a5d0-0000-7000-8000-ffffffffffff", title: "Autre" },
        }),
      ) as never,
    );
    const { result } = renderHook(() => useStoryValidation(STORY, ID_A));
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
  });
});
