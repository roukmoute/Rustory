import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/import-export", () => ({
  analyzeStructuredArchiveForCreation: vi.fn(),
  acceptStructuredArchiveCreation: vi.fn(),
}));

vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: vi.fn(),
}));

import {
  acceptStructuredArchiveCreation,
  analyzeStructuredArchiveForCreation,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { useArchiveCreation } from "./use-archive-creation";

// The REAL wire shape of a creatable verdict: exactly the four archive
// aspects (no formatVersion — the foreign pack declares none).
const ARCHIVE_ANALYZED_CLEAN = {
  kind: "analyzed" as const,
  quality: "clean" as const,
  state: "recognized" as const,
  findings: [
    {
      aspect: "envelope" as const,
      category: "recognized" as const,
      message: "Le descripteur story.json est présent et lisible.",
    },
    {
      aspect: "title" as const,
      category: "recognized" as const,
      message: "Le titre de l'histoire est valide.",
    },
    {
      aspect: "structure" as const,
      category: "recognized" as const,
      message: "La structure du pack est reconnue et convertie en histoire.",
    },
    {
      aspect: "media" as const,
      category: "recognized" as const,
      message:
        "Tous les fichiers audio et image référencés sont présents et reconnus.",
    },
  ],
  creatableSummary: {
    title: "Le pack du soir",
    nodeCount: 3,
    retainedMedia: ["cover.png", "intro.mp3"],
    discardedMedia: [],
  },
  archiveName: "Le pack du soir.zip",
  archivePath: "/home/user/Le pack du soir.zip",
};

const ARCHIVE_ANALYZED_BLOCKED = {
  kind: "analyzed" as const,
  quality: "unusable" as const,
  state: "blocked" as const,
  findings: [
    {
      aspect: "envelope" as const,
      category: "blocking" as const,
      message:
        "L'archive ne contient pas de descripteur story.json lisible. Corrige l'archive puis relance l'analyse.",
    },
  ],
  archiveName: "casse.zip",
  archivePath: "/home/user/casse.zip",
};

const CREATED_CARD = {
  id: "0197a5d0-0000-7000-8000-000000000042",
  title: "Le pack du soir",
  importState: "recognized" as const,
};

describe("useArchiveCreation", () => {
  beforeEach(() => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockReset();
    vi.mocked(acceptStructuredArchiveCreation).mockReset();
    vi.mocked(invalidateLibraryOverviewCache).mockReset();
  });

  it("starts idle", () => {
    const { result } = renderHook(() => useArchiveCreation());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("analyzes into a review state without mutating", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockResolvedValueOnce(
      ARCHIVE_ANALYZED_CLEAN,
    );
    const { result } = renderHook(() => useArchiveCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ARCHIVE_ANALYZED_CLEAN,
    });
    expect(acceptStructuredArchiveCreation).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("returns to idle on a cancelled dialog", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockResolvedValueOnce({
      kind: "cancelled",
    });
    const { result } = renderHook(() => useArchiveCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("a cancelled re-pick restores the review verdict it was opened over", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation)
      .mockResolvedValueOnce(ARCHIVE_ANALYZED_CLEAN)
      .mockResolvedValueOnce({ kind: "cancelled" });
    const { result } = renderHook(() => useArchiveCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ARCHIVE_ANALYZED_CLEAN,
    });
  });

  it("accepts a creatable verdict, invalidates the cache and lands created", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockResolvedValueOnce(
      ARCHIVE_ANALYZED_CLEAN,
    );
    vi.mocked(acceptStructuredArchiveCreation).mockResolvedValueOnce(
      CREATED_CARD,
    );
    const { result } = renderHook(() => useArchiveCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptStructuredArchiveCreation).toHaveBeenCalledWith({
      archivePath: ARCHIVE_ANALYZED_CLEAN.archivePath,
    });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_CARD,
    });
  });

  it("a blocked verdict has nothing to accept", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockResolvedValueOnce(
      ARCHIVE_ANALYZED_BLOCKED,
    );
    const { result } = renderHook(() => useArchiveCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptStructuredArchiveCreation).not.toHaveBeenCalled();
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ARCHIVE_ANALYZED_BLOCKED,
    });
  });

  it("a refused accept lands failed(accept) and retryAccept re-runs the commit", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockResolvedValueOnce(
      ARCHIVE_ANALYZED_CLEAN,
    );
    vi.mocked(acceptStructuredArchiveCreation)
      .mockRejectedValueOnce({
        code: "IMPORT_FAILED",
        message: "Création impossible: l'archive n'a pas pu être revalidée.",
        userAction: "Relance l'analyse puis réessaie.",
        details: null,
      })
      .mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useArchiveCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(result.current.status.kind).toBe("failed");
    expect(result.current.failedPhase).toBe("accept");

    await act(async () => {
      await result.current.retryAccept();
    });
    expect(acceptStructuredArchiveCreation).toHaveBeenCalledTimes(2);
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_CARD,
    });
  });

  it("a transport failure at analysis lands failed(analyze)", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockRejectedValueOnce({
      code: "IMPORT_FAILED",
      message: "Création impossible: le nom de l'archive choisie ne peut pas être utilisé par Rustory.",
      userAction: "Renomme l'archive puis relance l'analyse.",
      details: null,
    });
    const { result } = renderHook(() => useArchiveCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status.kind).toBe("failed");
    expect(result.current.failedPhase).toBe("analyze");
  });

  it("abandon is a pure frontend reset from review only", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockResolvedValueOnce(
      ARCHIVE_ANALYZED_CLEAN,
    );
    const { result } = renderHook(() => useArchiveCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("dismiss returns a terminal status to idle", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockResolvedValueOnce(
      ARCHIVE_ANALYZED_CLEAN,
    );
    vi.mocked(acceptStructuredArchiveCreation).mockResolvedValueOnce(
      CREATED_CARD,
    );
    const { result } = renderHook(() => useArchiveCreation());
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
    let resolveFirst!: (v: unknown) => void;
    vi.mocked(analyzeStructuredArchiveForCreation).mockImplementationOnce(
      () =>
        new Promise((res) => {
          resolveFirst = res as never;
        }),
    );
    const { result } = renderHook(() => useArchiveCreation());
    let first: Promise<void>;
    act(() => {
      first = result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(analyzeStructuredArchiveForCreation).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveFirst(ARCHIVE_ANALYZED_CLEAN);
      await first;
    });
    expect(result.current.status.kind).toBe("review");
  });

  it("injectExternalVerdict lands DIRECTLY in review (silent settlement — no analyzing state)", () => {
    const { result } = renderHook(() => useArchiveCreation());
    const { kind: _k, ...fields } = ARCHIVE_ANALYZED_CLEAN;
    let accepted = false;
    act(() => {
      accepted = result.current.injectExternalVerdict(fields, "drop");
    });
    expect(accepted).toBe(true);
    expect(result.current.origin).toBe("drop");
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ARCHIVE_ANALYZED_CLEAN,
    });
    expect(analyzeStructuredArchiveForCreation).not.toHaveBeenCalled();
  });

  it("accepts an injected external verdict through the UNCHANGED accept phase", async () => {
    vi.mocked(acceptStructuredArchiveCreation).mockResolvedValueOnce(
      CREATED_CARD,
    );
    const { result } = renderHook(() => useArchiveCreation());
    const { kind: _k, ...fields } = ARCHIVE_ANALYZED_CLEAN;
    act(() => {
      result.current.injectExternalVerdict(fields, "osOpen");
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptStructuredArchiveCreation).toHaveBeenCalledWith({
      archivePath: ARCHIVE_ANALYZED_CLEAN.archivePath,
    });
    expect(result.current.status.kind).toBe("created");
  });

  it("clearExternalReview resets an external-origin surface only", async () => {
    vi.mocked(analyzeStructuredArchiveForCreation).mockResolvedValueOnce(
      ARCHIVE_ANALYZED_CLEAN,
    );
    const { result } = renderHook(() => useArchiveCreation());
    // Picker origin: never touched by the internal supersede.
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    act(() => {
      result.current.clearExternalReview();
    });
    expect(result.current.status.kind).toBe("review");

    // External origin: steps aside.
    const { kind: _k, ...fields } = ARCHIVE_ANALYZED_CLEAN;
    act(() => {
      result.current.injectExternalVerdict(fields, "drop");
    });
    act(() => {
      result.current.clearExternalReview();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });
});
