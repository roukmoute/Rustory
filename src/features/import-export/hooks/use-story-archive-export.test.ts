import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/import-export", () => ({
  exportStoryArchiveWithSaveDialog: vi.fn(),
}));

import { exportStoryArchiveWithSaveDialog } from "../../../ipc/commands/import-export";
import { useArchiveExportShell } from "../../../shell/state/archive-export-shell-store";
import { useStoryArchiveExport } from "./use-story-archive-export";

const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";
const EXPORTED = {
  kind: "exported" as const,
  destinationPath: "/home/moi/Les Bestioles.zip",
  bytesWritten: 12_345_678,
  imageCount: 1,
  audioCount: 2,
};
const DESTINATION_ERROR = {
  code: "EXPORT_DESTINATION_UNAVAILABLE" as const,
  message: "Écriture refusée par le système pour ce dossier.",
  userAction: "Choisis un dossier où tu as les droits en écriture.",
  details: { source: "temp_create", kind: "permission_denied" },
};

/** A promise the test settles by hand, to observe the in-flight state. */
function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useStoryArchiveExport", () => {
  beforeEach(() => {
    vi.mocked(exportStoryArchiveWithSaveDialog).mockReset();
    useArchiveExportShell.setState({ status: { kind: "idle" } });
  });

  it("starts idle", () => {
    const { result } = renderHook(() => useStoryArchiveExport());
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.isExporting).toBe(false);
  });

  it("suggests a sanitized <title>.zip and settles on the exported archive", async () => {
    vi.mocked(exportStoryArchiveWithSaveDialog).mockResolvedValueOnce(EXPORTED);
    const { result } = renderHook(() => useStoryArchiveExport());

    await act(async () => {
      await result.current.triggerExport(STORY_ID, "Les Bestioles: tome 1");
    });

    expect(exportStoryArchiveWithSaveDialog).toHaveBeenCalledWith(
      { storyId: STORY_ID, suggestedFilename: "Les_Bestioles_tome_1.zip" },
      expect.any(Function),
    );
    expect(result.current.status).toEqual({
      kind: "exported",
      storyTitle: "Les Bestioles: tome 1",
      destinationPath: "/home/moi/Les Bestioles.zip",
      bytesWritten: 12_345_678,
      imageCount: 1,
      audioCount: 2,
    });
    expect(result.current.isExporting).toBe(false);
  });

  it("is exporting, without a percent, from the click until the first tick", async () => {
    const flight = deferred<typeof EXPORTED>();
    vi.mocked(exportStoryArchiveWithSaveDialog).mockReturnValueOnce(
      flight.promise,
    );
    const { result } = renderHook(() => useStoryArchiveExport());

    let settled!: Promise<void>;
    act(() => {
      settled = result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    expect(result.current.status).toEqual({
      kind: "exporting",
      storyId: STORY_ID,
      storyTitle: "Les Bestioles",
      percent: null,
    });
    expect(result.current.isExporting).toBe(true);

    await act(async () => {
      flight.resolve(EXPORTED);
      await settled;
    });
    expect(result.current.status.kind).toBe("exported");
  });

  it("shows the streamed percent while exporting", async () => {
    const flight = deferred<typeof EXPORTED>();
    let tick!: (percent: number) => void;
    vi.mocked(exportStoryArchiveWithSaveDialog).mockImplementationOnce(
      (_input, onProgress) => {
        tick = onProgress!;
        return flight.promise;
      },
    );
    const { result } = renderHook(() => useStoryArchiveExport());

    let settled!: Promise<void>;
    act(() => {
      settled = result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    act(() => tick(37));
    expect(result.current.status).toMatchObject({
      kind: "exporting",
      percent: 37,
    });

    await act(async () => {
      flight.resolve(EXPORTED);
      await settled;
    });
    // A late tick never resurrects the in-flight bar.
    act(() => tick(99));
    expect(result.current.status.kind).toBe("exported");
  });

  it("swallows a second export while one is in flight", async () => {
    const flight = deferred<typeof EXPORTED>();
    vi.mocked(exportStoryArchiveWithSaveDialog).mockReturnValueOnce(
      flight.promise,
    );
    const { result } = renderHook(() => useStoryArchiveExport());

    let first!: Promise<void>;
    act(() => {
      first = result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    await act(async () => {
      await result.current.triggerExport("another-story", "Une autre");
    });
    expect(exportStoryArchiveWithSaveDialog).toHaveBeenCalledTimes(1);
    expect(result.current.status).toMatchObject({ storyId: STORY_ID });

    await act(async () => {
      flight.resolve(EXPORTED);
      await first;
    });
  });

  it("returns to idle, silently, when the dialog is cancelled", async () => {
    vi.mocked(exportStoryArchiveWithSaveDialog).mockResolvedValueOnce({
      kind: "cancelled",
    });
    const { result } = renderHook(() => useStoryArchiveExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("keeps the refusal, with the story it concerns", async () => {
    vi.mocked(exportStoryArchiveWithSaveDialog).mockRejectedValueOnce(
      DESTINATION_ERROR,
    );
    const { result } = renderHook(() => useStoryArchiveExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    expect(result.current.status).toEqual({
      kind: "failed",
      storyId: STORY_ID,
      storyTitle: "Les Bestioles",
      error: DESTINATION_ERROR,
    });
  });

  it("normalizes a non-AppError rejection to UNKNOWN", async () => {
    vi.mocked(exportStoryArchiveWithSaveDialog).mockRejectedValueOnce(
      new Error("boom"),
    );
    const { result } = renderHook(() => useStoryArchiveExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    expect(result.current.status).toMatchObject({
      kind: "failed",
      error: { code: "UNKNOWN" },
    });
  });

  it("retries the story a failure concerns, and only from a failure", async () => {
    vi.mocked(exportStoryArchiveWithSaveDialog)
      .mockRejectedValueOnce(DESTINATION_ERROR)
      .mockResolvedValueOnce(EXPORTED);
    const { result } = renderHook(() => useStoryArchiveExport());

    // Nothing to retry while idle.
    await act(async () => {
      await result.current.retryExport();
    });
    expect(exportStoryArchiveWithSaveDialog).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    await act(async () => {
      await result.current.retryExport();
    });
    expect(exportStoryArchiveWithSaveDialog).toHaveBeenCalledTimes(2);
    expect(
      vi.mocked(exportStoryArchiveWithSaveDialog).mock.calls[1][0],
    ).toEqual({ storyId: STORY_ID, suggestedFilename: "Les_Bestioles.zip" });
    expect(result.current.status.kind).toBe("exported");
  });

  it("restores the refusal the user was reading when a retry is cancelled", async () => {
    vi.mocked(exportStoryArchiveWithSaveDialog)
      .mockRejectedValueOnce(DESTINATION_ERROR)
      .mockResolvedValueOnce({ kind: "cancelled" });
    const { result } = renderHook(() => useStoryArchiveExport());
    await act(async () => {
      await result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    await act(async () => {
      await result.current.retryExport();
    });
    expect(result.current.status).toMatchObject({
      kind: "failed",
      error: DESTINATION_ERROR,
    });
  });

  it("dismisses a settled status, never an export in flight", async () => {
    const flight = deferred<typeof EXPORTED>();
    vi.mocked(exportStoryArchiveWithSaveDialog).mockReturnValueOnce(
      flight.promise,
    );
    const { result } = renderHook(() => useStoryArchiveExport());

    let settled!: Promise<void>;
    act(() => {
      settled = result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    act(() => result.current.dismissStatus());
    expect(result.current.status.kind).toBe("exporting");

    await act(async () => {
      flight.resolve(EXPORTED);
      await settled;
    });
    act(() => result.current.dismissStatus());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("keeps tracking an export that outlasts the screen that started it", async () => {
    const flight = deferred<typeof EXPORTED>();
    vi.mocked(exportStoryArchiveWithSaveDialog).mockReturnValueOnce(
      flight.promise,
    );
    const first = renderHook(() => useStoryArchiveExport());
    let settled!: Promise<void>;
    act(() => {
      settled = first.result.current.triggerExport(STORY_ID, "Les Bestioles");
    });
    // The library screen goes away (the user opened the editor)…
    first.unmount();
    await act(async () => {
      flight.resolve(EXPORTED);
      await settled;
    });
    // …and finds the outcome when it comes back.
    const second = renderHook(() => useStoryArchiveExport());
    expect(second.result.current.status).toMatchObject({
      kind: "exported",
      destinationPath: "/home/moi/Les Bestioles.zip",
    });
  });
});
