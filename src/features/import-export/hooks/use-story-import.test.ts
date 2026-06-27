import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/import-export", () => ({
  analyzeArtifactForImport: vi.fn(),
  acceptArtifactImport: vi.fn(),
}));

vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: vi.fn(),
}));

import {
  acceptArtifactImport,
  analyzeArtifactForImport,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { useStoryImport } from "./use-story-import";

const IMPORTABLE_CONTENT = {
  title: "Le Soleil",
  structureJson: '{"schemaVersion":1,"nodes":[]}',
  contentChecksum: "a".repeat(64),
  createdAt: "2026-06-20T10:00:00.000Z",
  updatedAt: "2026-06-24T14:15:00.000Z",
};

const ANALYZED_PARTIAL = {
  kind: "analyzed" as const,
  quality: "partial" as const,
  state: "needsReview" as const,
  findings: [
    {
      aspect: "title" as const,
      category: "ambiguous" as const,
      message: "Titre normalisé.",
    },
  ],
  importableContent: IMPORTABLE_CONTENT,
  sourceName: "histoire.rustory",
  artifactChecksum: "b".repeat(64),
};

const ANALYZED_BLOCKED = {
  kind: "analyzed" as const,
  quality: "unusable" as const,
  state: "blocked" as const,
  findings: [
    {
      aspect: "integrity" as const,
      category: "blocking" as const,
      message: "Corruption détectée.",
    },
  ],
  sourceName: "corrompu.rustory",
  artifactChecksum: "c".repeat(64),
};

const CREATED_CARD = {
  id: "0197a5d0-0000-7000-8000-000000000001",
  title: "Le Soleil",
  importState: "needsReview" as const,
};

describe("useStoryImport", () => {
  beforeEach(() => {
    vi.mocked(analyzeArtifactForImport).mockReset();
    vi.mocked(acceptArtifactImport).mockReset();
    vi.mocked(invalidateLibraryOverviewCache).mockReset();
  });

  it("starts idle", () => {
    const { result } = renderHook(() => useStoryImport());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("analyzes into a review state without mutating (AC1)", async () => {
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
    // Analysis NEVER commits and NEVER touches the overview cache.
    expect(acceptArtifactImport).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("returns to idle on a cancelled dialog", async () => {
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce({
      kind: "cancelled",
    });
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("accepts a recognized verdict → imported + invalidates the cache", async () => {
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    vi.mocked(acceptArtifactImport).mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptRecognized();
    });
    expect(acceptArtifactImport).toHaveBeenCalledWith({
      content: IMPORTABLE_CONTENT,
      sourceName: "histoire.rustory",
      artifactChecksum: "b".repeat(64),
    });
    expect(result.current.status).toEqual({
      kind: "imported",
      story: CREATED_CARD,
    });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
  });

  it("does not accept a blocked verdict (no importable content)", async () => {
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_BLOCKED);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptRecognized();
    });
    expect(acceptArtifactImport).not.toHaveBeenCalled();
    // Still on the (blocked) review verdict — only Abandonner is offered.
    expect(result.current.status.kind).toBe("review");
  });

  it("abandons a review back to idle without mutating (AC1)", async () => {
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(acceptArtifactImport).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("transitions to failed and keeps the library intact on a commit error", async () => {
    const rustError = {
      code: "IMPORT_FAILED",
      message: "Import impossible: enregistrement local refusé.",
      userAction: "Réessaie plus tard.",
      details: { source: "db_commit" },
    };
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    vi.mocked(acceptArtifactImport).mockRejectedValueOnce(rustError);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptRecognized();
    });
    expect(result.current.status).toEqual({ kind: "failed", error: rustError });
    // Orthogonality: a failed commit leaves the library untouched — the
    // overview cache is never dropped (Rust rolled back atomically).
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("dismisses a terminal status back to idle", async () => {
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    vi.mocked(acceptArtifactImport).mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptRecognized();
    });
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("preserves a prior failed alert when a later analysis is cancelled", async () => {
    const rustError = {
      code: "IMPORT_FAILED",
      message: "Import impossible: fichier illisible.",
      userAction: "Réessaie.",
      details: { source: "file_read" },
    };
    vi.mocked(analyzeArtifactForImport).mockRejectedValueOnce(rustError);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({ kind: "failed", error: rustError });

    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce({
      kind: "cancelled",
    });
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    // Cancel must NOT silently wipe the error the user was reading.
    expect(result.current.status).toEqual({ kind: "failed", error: rustError });
  });

  it("keeps an in-progress review verdict when a later pick is cancelled", async () => {
    // First analysis lands a review verdict (the report the user is reading).
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });

    // Re-opening the picker from the report then cancelling must restore the
    // verdict — never reset to idle without an explicit Abandonner.
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce({
      kind: "cancelled",
    });
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
  });
});
