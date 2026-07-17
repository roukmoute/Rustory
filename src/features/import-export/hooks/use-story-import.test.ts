import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/import-export", () => ({
  analyzeArtifactForImport: vi.fn(),
  analyzeDropRequest: vi.fn(),
  analyzeOsOpenRequest: vi.fn(),
  acceptArtifactImport: vi.fn(),
}));

vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: vi.fn(),
}));

import {
  acceptArtifactImport,
  analyzeArtifactForImport,
  analyzeDropRequest,
  analyzeOsOpenRequest,
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
    vi.mocked(analyzeDropRequest).mockReset();
    vi.mocked(analyzeOsOpenRequest).mockReset();
    vi.mocked(acceptArtifactImport).mockReset();
    vi.mocked(invalidateLibraryOverviewCache).mockReset();
  });

  it("starts idle with the dialog origin", () => {
    const { result } = renderHook(() => useStoryImport());
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.origin).toBe("dialog");
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

  // ===== OS-open entry point (same machine, no dialog) =====

  it("analyzeFromOsOpen feeds an analyzed verdict into the SAME review machine", async () => {
    vi.mocked(analyzeOsOpenRequest).mockResolvedValueOnce(ANALYZED_PARTIAL);
    const { result } = renderHook(() => useStoryImport());
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromOsOpen();
    });
    expect(outcome).toEqual({ kind: "review" });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
    expect(result.current.origin).toBe("osOpen");
    // Analysis NEVER commits and NEVER touches the overview cache.
    expect(acceptArtifactImport).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("analyzeFromOsOpen with no pending intent is a total no-op that restores the prior status", async () => {
    // Land a dialog review first — the `none` pull must not wipe it.
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    vi.mocked(analyzeOsOpenRequest).mockResolvedValueOnce({ kind: "none" });
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromOsOpen();
    });
    expect(outcome).toEqual({ kind: "none" });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
    expect(result.current.origin).toBe("dialog");
  });

  it("analyzeFromOsOpen returns the multipleFiles calm limit without touching the machine", async () => {
    const message =
      "Rustory ouvre un fichier à la fois. Rouvre chaque fichier séparément.";
    vi.mocked(analyzeOsOpenRequest).mockResolvedValueOnce({
      kind: "multipleFiles",
      message,
    });
    const { result } = renderHook(() => useStoryImport());
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromOsOpen();
    });
    expect(outcome).toEqual({ kind: "multipleFiles", message });
    // A calm limit is NOT a machine state — the flow stays idle.
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("analyzeFromOsOpen lands a read failure in the failed state (intent replayable)", async () => {
    const rustError = {
      code: "IMPORT_FAILED",
      message: "Import impossible: fichier illisible.",
      userAction:
        "Vérifie que le fichier existe, qu'il s'agit bien d'un artefact Rustory, puis réessaie.",
      details: { source: "file_read", stage: "metadata" },
    };
    vi.mocked(analyzeOsOpenRequest).mockRejectedValueOnce(rustError);
    const { result } = renderHook(() => useStoryImport());
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromOsOpen();
    });
    expect(outcome).toEqual({ kind: "failed" });
    expect(result.current.status).toEqual({ kind: "failed", error: rustError });
    expect(result.current.origin).toBe("osOpen");

    // `Réessayer` replays the SAME (still pending) intent — the machine
    // accepts a fresh OS-open analysis from the failed state.
    vi.mocked(analyzeOsOpenRequest).mockResolvedValueOnce(ANALYZED_PARTIAL);
    await act(async () => {
      await result.current.analyzeFromOsOpen();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
  });

  it("a dialog pick after an OS-open flow resets the origin to dialog", async () => {
    vi.mocked(analyzeOsOpenRequest).mockResolvedValueOnce(ANALYZED_PARTIAL);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromOsOpen();
    });
    expect(result.current.origin).toBe("osOpen");

    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    expect(result.current.origin).toBe("dialog");
  });

  it("a cancelled pick restores BOTH the OS-open review verdict and its origin", async () => {
    vi.mocked(analyzeOsOpenRequest).mockResolvedValueOnce(ANALYZED_PARTIAL);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromOsOpen();
    });
    // The user opens the picker from the OS-open review, then cancels:
    // the verdict AND its origin must both survive (Réessayer semantics
    // stay OS-open).
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
    expect(result.current.origin).toBe("osOpen");
  });

  it("accepts an OS-open reviewed verdict through the UNCHANGED accept phase", async () => {
    vi.mocked(analyzeOsOpenRequest).mockResolvedValueOnce(ANALYZED_PARTIAL);
    vi.mocked(acceptArtifactImport).mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromOsOpen();
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

  // ===== Review-hardening: serialization, terminal gestures, accept retry =====

  function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (error: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    return { promise, resolve, reject };
  }

  const READ_ERROR = {
    code: "IMPORT_FAILED",
    message: "Import impossible: fichier illisible.",
    userAction:
      "Vérifie que le fichier existe, qu'il s'agit bien d'un artefact Rustory, puis réessaie.",
    details: { source: "file_read", stage: "metadata" },
  };

  it("serializes overlapping OS-open pulls: mono-slot queue, the LAST settlement wins", async () => {
    const ANALYZED_B = { ...ANALYZED_PARTIAL, sourceName: "b.rustory" };
    const first = deferred<typeof ANALYZED_PARTIAL>();
    vi.mocked(analyzeOsOpenRequest)
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(ANALYZED_B);
    const { result } = renderHook(() => useStoryImport());

    let firstPull!: Promise<unknown>;
    let secondPull!: Promise<unknown>;
    await act(async () => {
      firstPull = result.current.analyzeFromOsOpen();
      // The warm signal for B lands while A's pull is still in flight:
      // it must QUEUE (never a dropped fake `none`).
      secondPull = result.current.analyzeFromOsOpen();
      await Promise.resolve();
    });
    // Only the FIRST pull reached the wire — the second waits its turn.
    expect(analyzeOsOpenRequest).toHaveBeenCalledTimes(1);

    await act(async () => {
      first.resolve(ANALYZED_PARTIAL);
      await Promise.all([firstPull, secondPull]);
    });
    // The queued pull ran AFTER the first settlement…
    expect(analyzeOsOpenRequest).toHaveBeenCalledTimes(2);
    // …and the LAST settlement (the newest gesture, B) is what stays.
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_B,
    });
  });

  it("drops a late OS-open SUCCESS settling after Fermer (a terminal gesture is terminal)", async () => {
    vi.mocked(analyzeOsOpenRequest).mockRejectedValueOnce(READ_ERROR);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromOsOpen();
    });
    expect(result.current.status.kind).toBe("failed");

    // `Réessayer` is in flight; the user clicks `Fermer` while it reads.
    const retry = deferred<typeof ANALYZED_PARTIAL>();
    vi.mocked(analyzeOsOpenRequest).mockReturnValueOnce(retry.promise);
    let pendingRetry!: Promise<unknown>;
    await act(async () => {
      pendingRetry = result.current.analyzeFromOsOpen();
      await Promise.resolve();
    });
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.status).toEqual({ kind: "idle" });

    // The late SUCCESS settles — and is dropped, never a resurrected review.
    await act(async () => {
      retry.resolve(ANALYZED_PARTIAL);
      await pendingRetry;
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("drops a late OS-open FAILURE settling after Fermer (no resurrected alert)", async () => {
    vi.mocked(analyzeOsOpenRequest).mockRejectedValueOnce(READ_ERROR);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromOsOpen();
    });
    expect(result.current.status.kind).toBe("failed");

    const retry = deferred<never>();
    vi.mocked(analyzeOsOpenRequest).mockReturnValueOnce(
      retry.promise as never,
    );
    let pendingRetry!: Promise<unknown>;
    await act(async () => {
      pendingRetry = result.current.analyzeFromOsOpen();
      await Promise.resolve();
    });
    act(() => {
      result.current.dismiss();
    });

    await act(async () => {
      retry.reject(READ_ERROR);
      await pendingRetry;
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("retries a failed accept with the PRESERVED verdict — never a dead re-pull", async () => {
    const commitError = {
      code: "IMPORT_FAILED",
      message: "Import impossible: enregistrement local refusé.",
      userAction: "Réessaie.",
      details: { source: "db_commit", stage: "commit" },
    };
    vi.mocked(analyzeOsOpenRequest).mockResolvedValueOnce(ANALYZED_PARTIAL);
    vi.mocked(acceptArtifactImport)
      .mockRejectedValueOnce(commitError)
      .mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromOsOpen();
    });
    await act(async () => {
      await result.current.acceptRecognized();
    });
    // The failed COMMIT is tagged as the accept phase (the one-shot intent
    // is long consumed — a re-pull would answer `none` and retry nothing).
    expect(result.current.status.kind).toBe("failed");
    expect(result.current.failedPhase).toBe("accept");

    await act(async () => {
      await result.current.retryAccept();
    });
    expect(acceptArtifactImport).toHaveBeenCalledTimes(2);
    expect(acceptArtifactImport).toHaveBeenLastCalledWith({
      content: IMPORTABLE_CONTENT,
      sourceName: "histoire.rustory",
      artifactChecksum: "b".repeat(64),
    });
    expect(result.current.status).toEqual({
      kind: "imported",
      story: CREATED_CARD,
    });
    // The intent was never re-pulled — the preserved verdict carried it.
    expect(analyzeOsOpenRequest).toHaveBeenCalledTimes(1);
  });

  it("tags a failed READ as the analyze phase (the retry semantics differ)", async () => {
    vi.mocked(analyzeOsOpenRequest).mockRejectedValueOnce(READ_ERROR);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromOsOpen();
    });
    expect(result.current.failedPhase).toBe("analyze");
    // retryAccept is a strict no-op outside the accept phase.
    await act(async () => {
      await result.current.retryAccept();
    });
    expect(acceptArtifactImport).not.toHaveBeenCalled();
  });

  it("exposes the internal OS-open busy while a silent pull is in flight", async () => {
    const pull = deferred<{ kind: "none" }>();
    vi.mocked(analyzeOsOpenRequest).mockReturnValueOnce(pull.promise);
    const { result } = renderHook(() => useStoryImport());

    let pending!: Promise<unknown>;
    await act(async () => {
      pending = result.current.analyzeFromOsOpen();
      await Promise.resolve();
    });
    // Silent for the USER (no machine state)… but busy for the FLOWS.
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.isOsOpenSettling).toBe(true);

    await act(async () => {
      pull.resolve({ kind: "none" });
      await pending;
    });
    expect(result.current.isOsOpenSettling).toBe(false);
  });

  // ===== Drop entry point (same machine for a FILE, handed-back folder) =====

  const DROP_ARTIFACT = { ...ANALYZED_PARTIAL, kind: "artifact" as const };

  // The REAL wire shape of a creatable drop verdict: exactly the five
  // folder aspects (the facade guard refuses anything less — a hook test
  // must never accept a form the IPC facade would reject).
  const DROP_FOLDER = {
    kind: "folder" as const,
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

  it("analyzeFromDrop feeds a dropped FILE into the SAME review machine (origin drop)", async () => {
    vi.mocked(analyzeDropRequest).mockResolvedValueOnce(DROP_ARTIFACT);
    const { result } = renderHook(() => useStoryImport());
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromDrop();
    });
    expect(outcome).toEqual({ kind: "review" });
    // The wire `artifact` tag is re-tagged into the machine's verdict shape.
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
    expect(result.current.origin).toBe("drop");
    expect(acceptArtifactImport).not.toHaveBeenCalled();
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("analyzeFromDrop with no pending intent is a total no-op that keeps the prior status", async () => {
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    vi.mocked(analyzeDropRequest).mockResolvedValueOnce({ kind: "none" });
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromDrop();
    });
    expect(outcome).toEqual({ kind: "none" });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
    expect(result.current.origin).toBe("dialog");
  });

  it("analyzeFromDrop returns the multipleItems calm limit without touching the machine", async () => {
    const message =
      "Rustory traite un seul élément déposé à la fois. Dépose chaque élément séparément.";
    vi.mocked(analyzeDropRequest).mockResolvedValueOnce({
      kind: "multipleItems",
      message,
    });
    const { result } = renderHook(() => useStoryImport());
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromDrop();
    });
    expect(outcome).toEqual({ kind: "multipleItems", message });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("analyzeFromDrop hands a FOLDER verdict back without feeding this machine", async () => {
    vi.mocked(analyzeDropRequest).mockResolvedValueOnce(DROP_FOLDER);
    const { result } = renderHook(() => useStoryImport());
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromDrop();
    });
    expect(outcome).toEqual({ kind: "folder", verdict: DROP_FOLDER });
    // The folder belongs to the SIBLING machine — this one stays idle.
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("a FOLDER settlement clears an EARLIER drop-fed import surface (newest settlement wins)", async () => {
    vi.mocked(analyzeDropRequest)
      .mockResolvedValueOnce(DROP_ARTIFACT)
      .mockResolvedValueOnce(DROP_FOLDER);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromDrop();
    });
    expect(result.current.status.kind).toBe("review");

    await act(async () => {
      await result.current.analyzeFromDrop();
    });
    // The drop channel displays ONE settlement at a time: the earlier
    // drop-fed review stepped aside for the folder verdict.
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("a FOLDER settlement never clears a DIALOG-fed review (other gesture, untouched)", async () => {
    vi.mocked(analyzeArtifactForImport).mockResolvedValueOnce(ANALYZED_PARTIAL);
    vi.mocked(analyzeDropRequest).mockResolvedValueOnce(DROP_FOLDER);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.pickAndAnalyze();
    });
    await act(async () => {
      await result.current.analyzeFromDrop();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
    expect(result.current.origin).toBe("dialog");
  });

  it("analyzeFromDrop lands a read failure in the failed state (intent replayable)", async () => {
    vi.mocked(analyzeDropRequest).mockRejectedValueOnce(READ_ERROR);
    const { result } = renderHook(() => useStoryImport());
    let outcome;
    await act(async () => {
      outcome = await result.current.analyzeFromDrop();
    });
    expect(outcome).toEqual({ kind: "failed" });
    expect(result.current.status).toEqual({
      kind: "failed",
      error: READ_ERROR,
    });
    expect(result.current.origin).toBe("drop");
    expect(result.current.failedPhase).toBe("analyze");

    // `Réessayer` replays the SAME (still pending) intent.
    vi.mocked(analyzeDropRequest).mockResolvedValueOnce(DROP_ARTIFACT);
    await act(async () => {
      await result.current.analyzeFromDrop();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: ANALYZED_PARTIAL,
    });
  });

  it("retries a failed drop accept with the PRESERVED verdict — never a dead re-pull", async () => {
    const commitError = {
      code: "IMPORT_FAILED",
      message: "Import impossible: enregistrement local refusé.",
      userAction: "Réessaie.",
      details: { source: "db_commit", stage: "commit" },
    };
    vi.mocked(analyzeDropRequest).mockResolvedValueOnce(DROP_ARTIFACT);
    vi.mocked(acceptArtifactImport)
      .mockRejectedValueOnce(commitError)
      .mockResolvedValueOnce(CREATED_CARD);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromDrop();
    });
    await act(async () => {
      await result.current.acceptRecognized();
    });
    expect(result.current.status.kind).toBe("failed");
    expect(result.current.failedPhase).toBe("accept");

    await act(async () => {
      await result.current.retryAccept();
    });
    expect(acceptArtifactImport).toHaveBeenCalledTimes(2);
    expect(result.current.status).toEqual({
      kind: "imported",
      story: CREATED_CARD,
    });
    // The intent was never re-pulled — the preserved verdict carried it.
    expect(analyzeDropRequest).toHaveBeenCalledTimes(1);
  });

  it("serializes overlapping drop pulls: DEDICATED mono-slot queue, the LAST settlement wins", async () => {
    const DROP_B = { ...DROP_ARTIFACT, sourceName: "b.rustory" };
    const first = deferred<typeof DROP_ARTIFACT>();
    vi.mocked(analyzeDropRequest)
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(DROP_B);
    const { result } = renderHook(() => useStoryImport());

    let firstPull!: Promise<unknown>;
    let secondPull!: Promise<unknown>;
    await act(async () => {
      firstPull = result.current.analyzeFromDrop();
      // The signal for B lands while A's pull is still in flight: it must
      // QUEUE on the DROP queue (never a dropped fake `none`).
      secondPull = result.current.analyzeFromDrop();
      await Promise.resolve();
    });
    expect(analyzeDropRequest).toHaveBeenCalledTimes(1);

    await act(async () => {
      first.resolve(DROP_ARTIFACT);
      await Promise.all([firstPull, secondPull]);
    });
    expect(analyzeDropRequest).toHaveBeenCalledTimes(2);
    expect(result.current.status).toEqual({
      kind: "review",
      verdict: { ...ANALYZED_PARTIAL, sourceName: "b.rustory" },
    });
  });

  it("drops a late drop settlement after Fermer (a terminal gesture is terminal)", async () => {
    vi.mocked(analyzeDropRequest).mockRejectedValueOnce(READ_ERROR);
    const { result } = renderHook(() => useStoryImport());
    await act(async () => {
      await result.current.analyzeFromDrop();
    });
    expect(result.current.status.kind).toBe("failed");

    // `Réessayer` is in flight; the user clicks `Fermer` while it reads.
    const retry = deferred<typeof DROP_ARTIFACT>();
    vi.mocked(analyzeDropRequest).mockReturnValueOnce(retry.promise);
    let pendingRetry!: Promise<unknown>;
    await act(async () => {
      pendingRetry = result.current.analyzeFromDrop();
      await Promise.resolve();
    });
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.status).toEqual({ kind: "idle" });

    // The late SUCCESS settles — and is dropped, never a resurrected review.
    await act(async () => {
      retry.resolve(DROP_ARTIFACT);
      await pendingRetry;
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("invalidateDropSettlements drops a late FOLDER settlement (terminal gesture on the sibling machine)", async () => {
    // The user closes a drop-fed FOLDER review while a newer drop pull is
    // still reading: the route calls invalidateDropSettlements — the late
    // `folder` outcome must degrade to `none` so nothing reopens.
    const pull = deferred<typeof DROP_FOLDER>();
    vi.mocked(analyzeDropRequest).mockReturnValueOnce(pull.promise);
    const { result } = renderHook(() => useStoryImport());

    let pending!: Promise<unknown>;
    await act(async () => {
      pending = result.current.analyzeFromDrop();
      await Promise.resolve();
    });
    act(() => {
      result.current.invalidateDropSettlements();
    });

    let outcome: unknown;
    await act(async () => {
      pull.resolve(DROP_FOLDER);
      outcome = await pending;
    });
    expect(outcome).toEqual({ kind: "none" });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("exposes the internal drop busy while a silent pull is in flight", async () => {
    const pull = deferred<{ kind: "none" }>();
    vi.mocked(analyzeDropRequest).mockReturnValueOnce(pull.promise);
    const { result } = renderHook(() => useStoryImport());

    let pending!: Promise<unknown>;
    await act(async () => {
      pending = result.current.analyzeFromDrop();
      await Promise.resolve();
    });
    // Silent for the USER (no machine state)… but busy for the FLOWS —
    // and NEVER conflated with the OS-open settling flag.
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(result.current.isDropSettling).toBe(true);
    expect(result.current.isOsOpenSettling).toBe(false);

    await act(async () => {
      pull.resolve({ kind: "none" });
      await pending;
    });
    expect(result.current.isDropSettling).toBe(false);
  });
});
