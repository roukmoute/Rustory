import { describe, expect, it, vi } from "vitest";

import { isAppError } from "../../shared/errors/app-error";
import type { LibraryOverviewDto } from "../../shared/ipc-contracts/library";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("library IPC contract", () => {
  it("accepts an empty overview as a typed LibraryOverviewDto", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockResolvedValueOnce({ stories: [] });

    const { getLibraryOverview } = await import("../commands/library");
    const overview: LibraryOverviewDto = await getLibraryOverview();

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

    await expect(getLibraryOverview()).rejects.toMatchObject({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
    });
  });

  it("rejects with an UNKNOWN-coded error when the Rust side hangs past the timeout", async () => {
    const core = await import("@tauri-apps/api/core");
    // Never resolves: triggers the facade timeout guard.
    vi.mocked(core.invoke).mockImplementationOnce(
      () => new Promise(() => {}),
    );

    const { getLibraryOverview } = await import("../commands/library");

    await expect(getLibraryOverview(20)).rejects.toMatchObject({
      code: "UNKNOWN",
      message: expect.stringMatching(/trop de temps/i),
    });
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
