import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-catalog", () => ({
  readPackCover: vi.fn(),
}));

import { readPackCover } from "../../../ipc/commands/device-catalog";
import { invalidatePackCoverCache, usePackCover } from "./use-pack-cover";

const UUID_A = "abababab-abab-abab-abab-ababfac5562d";

describe("usePackCover", () => {
  beforeEach(() => {
    vi.mocked(readPackCover).mockReset();
    invalidatePackCoverCache();
  });

  it("never requests a cover when the pack has none (hasCover=false)", () => {
    const { result } = renderHook(() => usePackCover(UUID_A, false));
    expect(result.current).toBeNull();
    expect(readPackCover).not.toHaveBeenCalled();
  });

  it("resolves the cover data URL when one exists", async () => {
    vi.mocked(readPackCover).mockResolvedValueOnce({
      dataUrl: "data:image/png;base64,AAAA",
    });
    const { result } = renderHook(() => usePackCover(UUID_A, true));
    await waitFor(() =>
      expect(result.current).toBe("data:image/png;base64,AAAA"),
    );
  });

  it("caches the result so a second consumer does not re-hit the backend", async () => {
    vi.mocked(readPackCover).mockResolvedValueOnce({
      dataUrl: "data:image/png;base64,AAAA",
    });
    const first = renderHook(() => usePackCover(UUID_A, true));
    await waitFor(() => expect(first.result.current).not.toBeNull());

    const second = renderHook(() => usePackCover(UUID_A, true));
    await waitFor(() =>
      expect(second.result.current).toBe("data:image/png;base64,AAAA"),
    );
    expect(readPackCover).toHaveBeenCalledTimes(1);
  });

  it("degrades to null (no cover) when the read fails", async () => {
    vi.mocked(readPackCover).mockRejectedValueOnce(new Error("boom"));
    const { result } = renderHook(() => usePackCover(UUID_A, true));
    // Stays null; the failure is swallowed (covers are decorative).
    await waitFor(() => expect(readPackCover).toHaveBeenCalled());
    expect(result.current).toBeNull();
  });
});
