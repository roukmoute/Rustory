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

  // ===== Drop entry point (verdict injection, no picker, no dialog) =====

  const { kind: _cleanTag, ...CLEAN_FIELDS } = FOLDER_ANALYZED_CLEAN;
  const DROP_FOLDER_VERDICT = { kind: "folder" as const, ...CLEAN_FIELDS };

  it("starts on the picker origin", () => {
    const { result } = renderHook(() => useStructuredCreation());
    expect(result.current.origin).toBe("picker");
  });

  it("injectDropVerdict lands DIRECTLY in review (silent pull — no analyzing state)", () => {
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    // The wire `folder` tag is re-tagged into the machine's verdict shape.
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_CLEAN,
    });
    expect(result.current.origin).toBe("drop");
    // Injection carries an already-pure verdict: zero mutation, zero IPC.
    expect(acceptStructuredCreation).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("accepts an injected drop verdict through the UNCHANGED accept phase (folderPath from the DTO)", async () => {
    vi.mocked(acceptStructuredCreation).mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptStructuredCreation).toHaveBeenCalledWith({
      folderPath: "/home/user/mon-dossier",
    });
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_CARD,
    });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
  });

  it("retries a failed drop accept with the PRESERVED verdict — never a re-pick", async () => {
    const commitError = {
      code: "IMPORT_FAILED",
      message: "Création impossible: enregistrement local refusé.",
      userAction: "Réessaie.",
      details: { source: "db_commit" },
    };
    vi.mocked(acceptStructuredCreation)
      .mockRejectedValueOnce(commitError)
      .mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    // The failed COMMIT is tagged as the accept phase — the one-shot drop
    // intent is long consumed, a re-pull would answer `none`.
    expect(result.current.status.kind).toBe("failed");
    expect(result.current.failedPhase).toBe("accept");
    expect(result.current.origin).toBe("drop");

    await act(async () => {
      await result.current.retryAccept();
    });
    expect(acceptStructuredCreation).toHaveBeenCalledTimes(2);
    expect(acceptStructuredCreation).toHaveBeenLastCalledWith({
      folderPath: "/home/user/mon-dossier",
    });
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_CARD,
    });
    // The picker was NEVER opened along the way.
    expect(analyzeStructuredFolderForCreation).not.toHaveBeenCalled();
  });

  it("retryAccept is a strict no-op outside a failed accept", async () => {
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    await act(async () => {
      await result.current.retryAccept();
    });
    expect(acceptStructuredCreation).not.toHaveBeenCalled();
    expect(result.current.status.kind).toBe("review");
  });

  it("a newer injection replaces the displayed review (last gesture wins)", () => {
    const NEWER = {
      ...DROP_FOLDER_VERDICT,
      folderName: "second-dossier",
      folderPath: "/home/user/second-dossier",
    };
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    act(() => {
      result.current.injectDropVerdict(NEWER);
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: { ...NEWER, kind: "analyzed" },
    });
  });

  it("clearDropReview resets a DROP-origin surface only", () => {
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    act(() => {
      result.current.clearDropReview();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("DECLINES an injection while a commit is in flight — the commit and its retry survive", async () => {
    // A drop-fed review A is displayed; the user drops B (its pull is
    // reading) then clicks `Créer l'histoire` on A. B's verdict settles
    // while A's commit is in flight: injecting would overwrite the
    // `creating` screen AND wipe the verdict preserved for retryAccept.
    const commitError = {
      code: "IMPORT_FAILED",
      message: "Création impossible: enregistrement local refusé.",
      userAction: "Réessaie.",
      details: { source: "db_commit" },
    };
    let rejectCommit!: (error: unknown) => void;
    vi.mocked(acceptStructuredCreation)
      .mockImplementationOnce(
        () =>
          new Promise((_resolve, reject) => {
            rejectCommit = reject;
          }),
      )
      .mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    let pendingCommit!: Promise<void>;
    act(() => {
      pendingCommit = result.current.acceptCreation();
    });
    expect(result.current.status).toEqual({ kind: "creating" });

    // B's verdict settles mid-commit: DECLINED, nothing touched.
    const NEWER = {
      ...DROP_FOLDER_VERDICT,
      folderName: "second-dossier",
      folderPath: "/home/user/second-dossier",
    };
    let accepted!: boolean;
    act(() => {
      accepted = result.current.injectDropVerdict(NEWER);
    });
    expect(accepted).toBe(false);
    expect(result.current.status).toEqual({ kind: "creating" });

    // The commit FAILS: the preserved verdict must still drive retryAccept
    // (never a silent no-op — "Errors must preserve dignity").
    await act(async () => {
      rejectCommit(commitError);
      await pendingCommit;
    });
    expect(result.current.status.kind).toBe("failed");
    expect(result.current.failedPhase).toBe("accept");

    await act(async () => {
      await result.current.retryAccept();
    });
    expect(acceptStructuredCreation).toHaveBeenCalledTimes(2);
    expect(acceptStructuredCreation).toHaveBeenLastCalledWith({
      folderPath: "/home/user/mon-dossier",
    });
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_CARD,
    });
  });

  it("clearDropReview never touches a machine mid-commit (same in-flight guard)", async () => {
    // A dropped FILE settles while a drop-fed folder commit is in flight:
    // the route's supersede must not reset the `creating` machine.
    let resolveCommit!: (value: typeof CREATED_CARD) => void;
    vi.mocked(acceptStructuredCreation).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveCommit = resolve;
        }),
    );
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    let pendingCommit!: Promise<void>;
    act(() => {
      pendingCommit = result.current.acceptCreation();
    });
    act(() => {
      result.current.clearDropReview();
    });
    // The commit screen survives; its settlement lands normally.
    expect(result.current.status).toEqual({ kind: "creating" });
    await act(async () => {
      resolveCommit(CREATED_CARD);
      await pendingCommit;
    });
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_CARD,
    });
  });

  it("clearDropReview never touches a PICKER-origin review", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_CLEAN,
    );
    const { result } = renderHook(() => useStructuredCreation());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    act(() => {
      result.current.clearDropReview();
    });
    // A picker-fed review is another gesture — the drop supersede must
    // leave it exactly as the user left it.
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_CLEAN,
    });
    expect(result.current.origin).toBe("picker");
  });

  it("abandoning an injected review is a pure frontend reset", () => {
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(acceptStructuredCreation).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("a picker pick after a drop flow resets the origin to picker", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce(
      FOLDER_ANALYZED_CLEAN,
    );
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.origin).toBe("picker");
  });

  it("a cancelled pick restores BOTH the injected review and its drop origin", async () => {
    vi.mocked(analyzeStructuredFolderForCreation).mockResolvedValueOnce({
      kind: "cancelled",
    });
    const { result } = renderHook(() => useStructuredCreation());
    act(() => {
      result.current.injectDropVerdict(DROP_FOLDER_VERDICT);
    });
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: FOLDER_ANALYZED_CLEAN,
    });
    expect(result.current.origin).toBe("drop");
  });
});
