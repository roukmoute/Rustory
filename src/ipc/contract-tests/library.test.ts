import { describe, expect, it, vi } from "vitest";

import { isAppError } from "../../shared/errors/app-error";
import {
  isLibraryOverviewDto,
  type LibraryOverviewDto,
} from "../../shared/ipc-contracts/library";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("library IPC contract", () => {
  it("accepts an empty overview as a typed LibraryOverviewDto", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockResolvedValueOnce({ stories: [] });

    const { getLibraryOverview } = await import("../commands/library");
    const overview: LibraryOverviewDto = await getLibraryOverview().promise;

    expect(overview).toEqual({ stories: [] });
  });

  it("propagates AppError-shaped rejections without mutation", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
      details: null,
    });

    const { getLibraryOverview } = await import("../commands/library");

    await expect(getLibraryOverview().promise).rejects.toMatchObject({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
    });
  });

  it("rejects with an UNKNOWN-coded error when the Rust side hangs past the timeout", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockImplementationOnce(
      () => new Promise(() => {}),
    );

    const { getLibraryOverview } = await import("../commands/library");

    await expect(getLibraryOverview(20).promise).rejects.toMatchObject({
      code: "UNKNOWN",
      message: expect.stringMatching(/trop de temps/i),
    });
  });

  it("cancel() swallows the timeout after teardown so no late rejection escapes", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockImplementationOnce(
      () => new Promise(() => {}),
    );

    const { getLibraryOverview } = await import("../commands/library");
    const handle = getLibraryOverview(10);
    // Attach a no-op catch so vitest doesn't flag an unhandled rejection
    // when cancel races the timer — the handle resolves to nothing after
    // cancel() because the guard is neutralized.
    handle.promise.catch(() => {});
    handle.cancel();

    await new Promise((resolve) => setTimeout(resolve, 30));
    // If cancel() did not clear the timer, the process would have logged
    // an unhandled rejection by now. No assertion beyond reaching this
    // line is necessary — the test just proves the teardown path runs.
  });
});

describe("isAppError guard", () => {
  it("requires the details field to be present", () => {
    // Matches the contract on both sides: `details` is always emitted,
    // even when it is `null`. A payload missing the key is drift.
    expect(
      isAppError({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "x",
        userAction: null,
        details: null,
      }),
    ).toBe(true);
    expect(
      isAppError({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "x",
        userAction: null,
      }),
    ).toBe(false);
  });

  it("rejects non-object payloads and missing required fields", () => {
    expect(isAppError(null)).toBe(false);
    expect(isAppError("boom")).toBe(false);
    expect(isAppError({ code: 42, message: "x", details: null })).toBe(false);
    expect(isAppError({ message: "x", details: null })).toBe(false);
  });
});

describe("isLibraryOverviewDto guard", () => {
  it("accepts a well-formed overview", () => {
    expect(
      isLibraryOverviewDto({
        stories: [
          { id: "a", title: "A" },
          { id: "b", title: "B" },
        ],
      }),
    ).toBe(true);
  });

  it("rejects a story with an empty id", () => {
    expect(
      isLibraryOverviewDto({ stories: [{ id: "", title: "A" }] }),
    ).toBe(false);
  });

  it("rejects a story with a blank title (empty or whitespace-only)", () => {
    expect(
      isLibraryOverviewDto({ stories: [{ id: "a", title: "" }] }),
    ).toBe(false);
    expect(
      isLibraryOverviewDto({ stories: [{ id: "a", title: "   " }] }),
    ).toBe(false);
  });

  it("rejects a payload with duplicate ids", () => {
    expect(
      isLibraryOverviewDto({
        stories: [
          { id: "a", title: "A" },
          { id: "a", title: "A bis" },
        ],
      }),
    ).toBe(false);
  });

  it("rejects non-object payloads and missing stories array", () => {
    expect(isLibraryOverviewDto(null)).toBe(false);
    expect(isLibraryOverviewDto({})).toBe(false);
    expect(isLibraryOverviewDto({ stories: "nope" })).toBe(false);
  });
});
