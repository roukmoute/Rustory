import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/import-export", () => ({
  exportStoryWithSaveDialog: vi.fn(),
}));

import { exportStoryWithSaveDialog } from "../../../ipc/commands/import-export";
import { useStoryExport } from "./use-story-export";

const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";
const SUGGESTED_TITLE = "Mon histoire";
const DESTINATION = "/tmp/Mon histoire.rustory";

const SUCCESS_OUTCOME = {
  kind: "exported" as const,
  destinationPath: DESTINATION,
  bytesWritten: 451,
  contentChecksum: "a".repeat(64),
};

describe("useStoryExport", () => {
  beforeEach(() => {
    vi.mocked(exportStoryWithSaveDialog).mockReset();
  });

  it("starts in idle", () => {
    const { result } = renderHook(() => useStoryExport());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("transitions through exporting → exported on success", async () => {
    vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(SUCCESS_OUTCOME);
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    expect(exportStoryWithSaveDialog).toHaveBeenCalledWith({
      storyId: STORY_ID,
      suggestedFilename: "Mon histoire.rustory",
    });
    expect(result.current.status).toEqual({
      kind: "exported",
      destinationPath: DESTINATION,
      bytesWritten: 451,
      contentChecksum: "a".repeat(64),
    });
  });

  it("stays idle when Rust reports a cancelled dialog", async () => {
    vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce({
      kind: "cancelled",
    });
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("transitions to failed with the AppError when Rust rejects", async () => {
    const rustError = {
      code: "EXPORT_DESTINATION_UNAVAILABLE",
      message: "Écriture refusée par le système pour ce dossier.",
      userAction: "Choisis un dossier où tu as les droits en écriture.",
      details: { source: "temp_create", kind: "permission_denied" },
    };
    vi.mocked(exportStoryWithSaveDialog).mockRejectedValueOnce(rustError);
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    expect(result.current.status).toEqual({
      kind: "failed",
      error: rustError,
    });
  });

  it("re-entrancy: a second trigger while a first is in flight is a no-op", async () => {
    let resolveFirst!: (value: typeof SUCCESS_OUTCOME) => void;
    vi.mocked(exportStoryWithSaveDialog).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveFirst = resolve;
      }),
    );

    const { result } = renderHook(() => useStoryExport());
    let firstDone: Promise<void>;
    await act(async () => {
      firstDone = result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
      await Promise.resolve();
    });
    expect(result.current.status.kind).toBe("exporting");
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    expect(exportStoryWithSaveDialog).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveFirst(SUCCESS_OUTCOME);
      await firstDone;
    });
  });

  it("retryExport relaunches the IPC call from a failed state", async () => {
    vi.mocked(exportStoryWithSaveDialog).mockRejectedValueOnce({
      code: "EXPORT_DESTINATION_UNAVAILABLE",
      message: "err",
      userAction: "act",
      details: null,
    });
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    expect(result.current.status.kind).toBe("failed");

    vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(SUCCESS_OUTCOME);
    await act(async () => {
      await result.current.retryExport();
    });
    expect(result.current.status.kind).toBe("exported");
    expect(exportStoryWithSaveDialog).toHaveBeenCalledTimes(2);
  });

  it("retryExport is a no-op on idle", async () => {
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.retryExport();
    });
    expect(exportStoryWithSaveDialog).not.toHaveBeenCalled();
  });

  it("retryExport is a no-op on exported", async () => {
    vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(SUCCESS_OUTCOME);
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    await act(async () => {
      await result.current.retryExport();
    });
    expect(exportStoryWithSaveDialog).toHaveBeenCalledTimes(1);
  });

  it("dismissStatus clears an exported status back to idle", async () => {
    vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(SUCCESS_OUTCOME);
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    expect(result.current.status.kind).toBe("exported");
    act(() => {
      result.current.dismissStatus();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("dismissStatus does NOT clear a failed alert — only retry or a fresh trigger leaves that state", async () => {
    vi.mocked(exportStoryWithSaveDialog).mockRejectedValueOnce({
      code: "EXPORT_DESTINATION_UNAVAILABLE",
      message: "err",
      userAction: "act",
      details: null,
    });
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    expect(result.current.status.kind).toBe("failed");
    act(() => {
      result.current.dismissStatus();
    });
    expect(result.current.status.kind).toBe("failed");
  });

  it("a cancelled dialog after a failed state preserves the alert", async () => {
    vi.mocked(exportStoryWithSaveDialog).mockRejectedValueOnce({
      code: "EXPORT_DESTINATION_UNAVAILABLE",
      message: "err",
      userAction: "act",
      details: null,
    });
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    expect(result.current.status.kind).toBe("failed");

    vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce({
      kind: "cancelled",
    });
    await act(async () => {
      await result.current.triggerExport(STORY_ID, SUGGESTED_TITLE);
    });
    // Cancel must NOT erase the stale alert — the user sees "failed"
    // until they explicitly retry or successfully export.
    expect(result.current.status.kind).toBe("failed");
  });

  it("suggestedTitle is turned into suggestedFilename with a .rustory suffix", async () => {
    vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce({
      kind: "cancelled",
    });
    const { result } = renderHook(() => useStoryExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, "Sanitized_Title");
    });
    expect(exportStoryWithSaveDialog).toHaveBeenCalledWith({
      storyId: STORY_ID,
      suggestedFilename: "Sanitized_Title.rustory",
    });
  });
});
