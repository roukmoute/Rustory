import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/device-catalog", () => ({
  getOfficialCatalogStatus: vi.fn(),
  refreshOfficialCatalog: vi.fn(),
  importOfficialCatalog: vi.fn(),
}));

import {
  getOfficialCatalogStatus,
  importOfficialCatalog,
  refreshOfficialCatalog,
} from "../../../ipc/commands/device-catalog";
import { useOfficialCatalog } from "./use-official-catalog";

describe("useOfficialCatalog", () => {
  beforeEach(() => {
    vi.mocked(getOfficialCatalogStatus).mockReset();
    vi.mocked(refreshOfficialCatalog).mockReset();
    vi.mocked(importOfficialCatalog).mockReset();
  });

  it("loads the cached count on mount", async () => {
    vi.mocked(getOfficialCatalogStatus).mockResolvedValueOnce({ count: 7 });
    const { result } = renderHook(() => useOfficialCatalog());
    await waitFor(() =>
      expect(result.current.state).toEqual({ kind: "ready", count: 7 }),
    );
  });

  it("surfaces a status read failure as an error state", async () => {
    vi.mocked(getOfficialCatalogStatus).mockRejectedValueOnce({
      code: "OFFICIAL_CATALOG_UNAVAILABLE",
      message: "x",
      userAction: "y",
      details: null,
    });
    const { result } = renderHook(() => useOfficialCatalog());
    await waitFor(() => expect(result.current.state.kind).toBe("error"));
  });

  it("refresh updates the count and clears the action flag", async () => {
    vi.mocked(getOfficialCatalogStatus).mockResolvedValueOnce({ count: 0 });
    vi.mocked(refreshOfficialCatalog).mockResolvedValueOnce({ count: 1200 });
    const { result } = renderHook(() => useOfficialCatalog());
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    await act(async () => {
      await result.current.refresh();
    });
    expect(refreshOfficialCatalog).toHaveBeenCalledTimes(1);
    expect(result.current.state).toEqual({ kind: "ready", count: 1200 });
    expect(result.current.action).toBe("idle");
    expect(result.current.actionError).toBeNull();
  });

  it("surfaces a refresh failure without clobbering the existing count", async () => {
    vi.mocked(getOfficialCatalogStatus).mockResolvedValueOnce({ count: 5 });
    vi.mocked(refreshOfficialCatalog).mockRejectedValueOnce({
      code: "OFFICIAL_CATALOG_UNAVAILABLE",
      message: "offline",
      userAction: "retry",
      details: { source: "network" },
    });
    const { result } = renderHook(() => useOfficialCatalog());
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.actionError?.code).toBe("OFFICIAL_CATALOG_UNAVAILABLE");
    // The previously cached count is preserved.
    expect(result.current.state).toEqual({ kind: "ready", count: 5 });
  });

  it("a cancelled import leaves the count untouched", async () => {
    vi.mocked(getOfficialCatalogStatus).mockResolvedValueOnce({ count: 3 });
    vi.mocked(importOfficialCatalog).mockResolvedValueOnce({ kind: "cancelled" });
    const { result } = renderHook(() => useOfficialCatalog());
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    await act(async () => {
      await result.current.importFile();
    });
    expect(result.current.state).toEqual({ kind: "ready", count: 3 });
  });

  it("an imported file updates the count", async () => {
    vi.mocked(getOfficialCatalogStatus).mockResolvedValueOnce({ count: 0 });
    vi.mocked(importOfficialCatalog).mockResolvedValueOnce({
      kind: "imported",
      count: 42,
    });
    const { result } = renderHook(() => useOfficialCatalog());
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    await act(async () => {
      await result.current.importFile();
    });
    expect(result.current.state).toEqual({ kind: "ready", count: 42 });
  });

  it("calls onChanged after a successful refresh and a committed import (so the device re-reads)", async () => {
    vi.mocked(getOfficialCatalogStatus).mockResolvedValueOnce({ count: 0 });
    vi.mocked(refreshOfficialCatalog).mockResolvedValueOnce({ count: 1200 });
    vi.mocked(importOfficialCatalog).mockResolvedValueOnce({
      kind: "imported",
      count: 1201,
    });
    const onChanged = vi.fn();
    const { result } = renderHook(() => useOfficialCatalog({ onChanged }));
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    await act(async () => {
      await result.current.refresh();
    });
    await act(async () => {
      await result.current.importFile();
    });
    expect(onChanged).toHaveBeenCalledTimes(2);
  });

  it("does NOT call onChanged when the import is cancelled or the refresh fails", async () => {
    vi.mocked(getOfficialCatalogStatus).mockResolvedValueOnce({ count: 3 });
    vi.mocked(importOfficialCatalog).mockResolvedValueOnce({ kind: "cancelled" });
    vi.mocked(refreshOfficialCatalog).mockRejectedValueOnce({
      code: "OFFICIAL_CATALOG_UNAVAILABLE",
      message: "offline",
      userAction: "retry",
      details: null,
    });
    const onChanged = vi.fn();
    const { result } = renderHook(() => useOfficialCatalog({ onChanged }));
    await waitFor(() => expect(result.current.state.kind).toBe("ready"));

    await act(async () => {
      await result.current.importFile();
    });
    await act(async () => {
      await result.current.refresh();
    });
    expect(onChanged).not.toHaveBeenCalled();
  });
});
