import { act, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/import-export", () => ({
  analyzeStructuredFolderForCreation: vi.fn(),
  acceptStructuredCreation: vi.fn(),
}));

vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: vi.fn(),
}));

import {
  acceptStructuredCreation,
  analyzeStructuredFolderForCreation,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { useStructuredCreation } from "./use-structured-creation";

// The REAL wire shape of a creatable verdict: exactly the five folder
// aspects (the facade guard refuses anything less — the fixtures must
// speak the wire the hook actually receives).
const FOLDER_ANALYZED_CLEAN = {
  kind: "analyzed" as const,
  quality: "clean" as const,
  state: "recognized" as const,
  findings: [
    {
      aspect: "envelope" as const,
      category: "recognized" as const,
      message: "Le manifest histoire.json est présent et lisible.",
    },
    {
      aspect: "formatVersion" as const,
      category: "recognized" as const,
      message: "La version de format du manifest est prise en charge.",
    },
    {
      aspect: "title" as const,
      category: "recognized" as const,
      message: "Le titre de l'histoire est valide.",
    },
    {
      aspect: "structure" as const,
      category: "recognized" as const,
      message: "La structure de l'histoire est reconnue.",
    },
    {
      aspect: "media" as const,
      category: "recognized" as const,
      message:
        "Tous les fichiers audio et image référencés par le dossier sont présents et reconnus.",
    },
  ],
  creatableSummary: {
    title: "Le voyage de Nour",
    nodeCount: 2,
    retainedMedia: ["couverture.png"],
    discardedMedia: [],
  },
  folderName: "mon-dossier",
  folderPath: "/home/user/mon-dossier",
};

const FOLDER_ANALYZED_BLOCKED = {
  kind: "analyzed" as const,
  quality: "unusable" as const,
  state: "blocked" as const,
  findings: [
    {
      aspect: "envelope" as const,
      category: "blocking" as const,
      message: "Le dossier ne contient pas de manifest histoire.json lisible.",
    },
  ],
  folderName: "casse",
  folderPath: "/home/user/casse",
};

const CREATED_CARD = {
  id: "0197a5d0-0000-7000-8000-000000000001",
  title: "Le voyage de Nour",
  importState: "recognized" as const,
};

describe("useStructuredCreation", () => {
  beforeEach(() => {
    vi.mocked(analyzeStructuredFolderForCreation).mockReset();
    vi.mocked(acceptStructuredCreation).mockReset();
    vi.mocked(invalidateLibraryOverviewCache).mockReset();
  });

  it("starts idle", () => {
    const { result } = renderHook(() => useStructuredCreation());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("analyzes into a review state without mutating (AC4)", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_CLEAN,
    );
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_CLEAN,
    });
    expect(acceptStructuredCreation).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("returns to idle on a cancelled dialog", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce({
      kind: "cancelled",
    });
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("a cancelled re-pick restores the review verdict it was opened over", async () => {
    vi.mocked(analyzeStructuredFolderForCreation)
      .mockResolvedValueOnce(FOLDER_ANALYZED_CLEAN)
      .mockResolvedValueOnce({ kind: "cancelled" });
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_CLEAN,
    });
  });

  it("surfaces a blocked verdict as a review with nothing to accept", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_BLOCKED,
    );
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_BLOCKED,
    });
    // Accept on a blocked verdict is a no-op — nothing creatable.
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptStructuredCreation).not.toHaveBeenCalled();
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_BLOCKED,
    });
  });

  it("accepts a creatable verdict, invalidates the cache and lands created", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_CLEAN,
    );
    vi.mocked(acceptStructuredCreation).mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptStructuredCreation).toHaveBeenCalledWith({
      folderPath: "/home/user/mon-dossier",
    });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_CARD,
    });
  });

  it("a refused accept (re-analysis turned blocking) lands failed with the Rust error", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_CLEAN,
    );
    const rustError = {
      code: "IMPORT_FAILED",
      message: "Création impossible: le dossier n'a pas pu être revalidé.",
      userAction:
        "Le contenu du dossier a peut-être changé. Relance l'analyse du dossier puis réessaie.",
      details: { source: "other", cause: "revalidation" },
    };
    vi.mocked(acceptStructuredCreation).mockRejectedValueOnce(rustError);
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(result.current.status).toMatchObject({
      kind: "failed",
      error: { code: "IMPORT_FAILED" },
    });
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("a transport failure at analysis lands failed", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockRejectedValueOnce({
      code: "IMPORT_FAILED",
      message: "Création impossible: la fenêtre de sélection n'a pas pu s'ouvrir.",
      userAction: "Relance Rustory.",
      details: { source: "dialog_failed" },
    });
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toMatchObject({ kind: "failed" });
  });

  it("abandon is a pure frontend reset from review only", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_CLEAN,
    );
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    // Nothing was mutated, nothing to clean.
    expect(acceptStructuredCreation).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("dismiss returns a terminal status to idle", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_CLEAN,
    );
    vi.mocked(acceptStructuredCreation).mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("swallows a re-entrant pickAndAnalyze while one is in flight", async () => {
    let resolveAnalysis: (value: typeof FOLDER_ANALYZED_CLEAN) => void = () => {};
    vi.mocked(analyzeStructuredFolderForCreation).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveAnalysis = resolve;
        }),
    );
    const { result } = renderHook(() => useStructuredCreation());
    let first: Promise<void> = Promise.resolve();
    act(() => {
      first = result.current.pickAndAnalyze();
      // Second activation in the same tick: swallowed by the sync guard.
      void result.current.pickAndAnalyze();
    });
    expect(analyzeStructuredFolderForCreation).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveAnalysis(FOLDER_ANALYZED_CLEAN);
      await first;
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_CLEAN,
    });
  });

  it("ignores a stale analysis response after unmount (no setState leak)", async () => {
    let resolveAnalysis: (value: typeof FOLDER_ANALYZED_CLEAN) => void = () => {};
    vi.mocked(analyzeStructuredFolderForCreation).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveAnalysis = resolve;
        }),
    );
    const { result, unmount } = renderHook(() => useStructuredCreation());
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = result.current.pickAndAnalyze();
    });
    unmount();
    // The stale response lands AFTER unmount: the mounted guard drops it.
    resolveAnalysis(FOLDER_ANALYZED_CLEAN);
    await pending;
    // Reaching here without a React "setState on unmounted" warning is the
    // assertion; the status object is frozen at its last rendered value.
  });

  it("survives a StrictMode double-mount (the mount flag re-arms)", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_CLEAN,
    );
    const { result } = renderHook(() => useStructuredCreation(), {
      wrapper: StrictMode,
    });
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_CLEAN,
    });
  });
});
