import { useCallback, useEffect, useRef, useState } from "react";

import {
  acceptArtifactImport,
  analyzeArtifactForImport,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";
import type {
  AcceptArtifactImportInput,
  ImportArtifactAnalysis,
} from "../../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";

/** The `analyzed` verdict variant — what the `review` state renders. */
export type AnalyzedVerdict = Extract<
  ImportArtifactAnalysis,
  { kind: "analyzed" }
>;

export type StoryImportStatus =
  | { kind: "idle" }
  | { kind: "analyzing" }
  | { kind: "review"; verdict: AnalyzedVerdict }
  | { kind: "importing" }
  | { kind: "imported"; story: StoryCardDto }
  | { kind: "failed"; error: AppError };

export interface UseStoryImport {
  status: StoryImportStatus;
  /** Open the native file picker (owned by Rust) and analyze the chosen
   *  `.rustory`. A cancelled dialog is a silent no-op. Resolves when the
   *  analysis has settled so callers/tests can chain a step. */
  pickAndAnalyze(): Promise<void>;
  /** Commit the recognized story from a `review` state. No-op outside
   *  `review`, or when the verdict is blocked (no importable content). */
  acceptRecognized(): Promise<void>;
  /** Abandon an analyzed import (pure frontend, NO mutation): drop the
   *  verdict and return to idle. No-op outside `review`. */
  abandon(): void;
  /** Dismiss a terminal status (`imported` / `failed`) back to idle. The
   *  `failed` alert is never wiped implicitly, but its explicit `Fermer`
   *  must work — exactly like `Réessayer`. */
  dismiss(): void;
}

/**
 * Orchestrates the two-phase local-artifact import through the Rust-owned
 * dialog + analysis + commit boundary. Structural sibling of
 * `useStoryExport` / `useDeviceStoryImport`: same StrictMode-safe mount
 * flag and synchronous re-entrancy guard.
 *
 * AC1 — no mutation before acceptance: `pickAndAnalyze` only reads; the
 * library cache is invalidated ONLY after a successful `acceptRecognized`.
 * `abandon` is a pure frontend reset.
 */
export function useStoryImport(): UseStoryImport {
  const [status, setStatus] = useState<StoryImportStatus>({ kind: "idle" });

  const statusRef = useRef<StoryImportStatus>(status);
  statusRef.current = status;

  // StrictMode-safe mount flag: set on every mount phase so a synthetic
  // unmount+remount re-arms it.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Synchronous re-entrancy flag: the rendered status flips to
  // `analyzing` / `importing` only after a state flush, so a double
  // activation in the same tick would otherwise start two flows.
  const inFlightRef = useRef(false);

  const pickAndAnalyze = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    // Capture the FULL status the user was looking at before re-opening the
    // picker. A cancelled dialog must restore it verbatim — never silently
    // wipe an in-progress `review` verdict (its report) nor a `failed` alert
    // the user was still reading. The re-entrancy guard above guarantees this
    // is a settled status (never `analyzing` / `importing`).
    const priorStatus = statusRef.current;
    try {
      if (mountedRef.current) setStatus({ kind: "analyzing" });
      let verdict: ImportArtifactAnalysis;
      try {
        verdict = await analyzeArtifactForImport();
      } catch (err) {
        if (!mountedRef.current) return;
        setStatus({ kind: "failed", error: toAppError(err) });
        return;
      }
      if (!mountedRef.current) return;
      if (verdict.kind === "cancelled") {
        // Restore the complete pre-existing status (review / failed / imported
        // / idle) — a cancel is a no-op, it never discards a verdict.
        setStatus(priorStatus);
        return;
      }
      setStatus({ kind: "review", verdict });
    } finally {
      inFlightRef.current = false;
    }
  }, []);

  const acceptRecognized = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    const current = statusRef.current;
    if (current.kind !== "review") return;
    const content = current.verdict.importableContent;
    // A blocked verdict has no importable content — only `Abandonner`.
    if (!content) return;

    inFlightRef.current = true;
    const input: AcceptArtifactImportInput = {
      content,
      sourceName: current.verdict.sourceName,
      artifactChecksum: current.verdict.artifactChecksum,
    };
    try {
      if (mountedRef.current) setStatus({ kind: "importing" });
      let story: StoryCardDto;
      try {
        story = await acceptArtifactImport(input);
      } catch (err) {
        if (!mountedRef.current) return;
        setStatus({ kind: "failed", error: toAppError(err) });
        return;
      }
      // The canonical store HAS changed — drop the stale overview snapshot
      // BEFORE the mounted guard so an unmount mid-import still reconciles.
      invalidateLibraryOverviewCache();
      if (!mountedRef.current) return;
      setStatus({ kind: "imported", story });
    } finally {
      inFlightRef.current = false;
    }
  }, []);

  const abandon = useCallback((): void => {
    // Pure frontend reset — nothing was mutated before acceptance (AC1).
    if (statusRef.current.kind === "review") {
      setStatus({ kind: "idle" });
    }
  }, []);

  const dismiss = useCallback((): void => {
    const kind = statusRef.current.kind;
    if (kind === "imported" || kind === "failed") {
      setStatus({ kind: "idle" });
    }
  }, []);

  return {
    status,
    pickAndAnalyze,
    acceptRecognized,
    abandon,
    dismiss,
  };
}
