import { useCallback, useEffect, useRef, useState } from "react";

import {
  acceptStructuredCreation,
  analyzeStructuredFolderForCreation,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";
import type { StructuredCreationAnalysis } from "../../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";

/** The `analyzed` verdict variant — what the `review` state renders. */
export type AnalyzedFolderVerdict = Extract<
  StructuredCreationAnalysis,
  { kind: "analyzed" }
>;

export type StructuredCreationStatus =
  | { kind: "idle" }
  | { kind: "analyzing" }
  | { kind: "review"; verdict: AnalyzedFolderVerdict }
  | { kind: "creating" }
  | { kind: "created"; story: StoryCardDto }
  | { kind: "failed"; error: AppError };

export interface UseStructuredCreation {
  status: StructuredCreationStatus;
  /** Open the native FOLDER picker (owned by Rust) and analyze the chosen
   *  structured folder. A cancelled dialog is a silent no-op. Resolves when
   *  the analysis has settled so callers/tests can chain a step. */
  pickAndAnalyze(): Promise<void>;
  /** Commit the analyzed folder from a `review` state (`Créer l'histoire`).
   *  No-op outside `review`, or when the verdict is blocked (nothing
   *  creatable). Rust re-analyzes the disk from zero. */
  acceptCreation(): Promise<void>;
  /** Abandon an analyzed folder (pure frontend, NO mutation): drop the
   *  verdict and return to idle. No-op outside `review`. */
  abandon(): void;
  /** Dismiss a terminal status (`created` / `failed`) back to idle. */
  dismiss(): void;
}

/**
 * Orchestrates the two-phase structured-folder creation through the
 * Rust-owned dialog + analysis + commit boundary. Structural sibling of
 * `useStoryImport` — the duplication is DELIBERATE (an import-review
 * orchestrator is a context orchestrator, never a generic reusable
 * component); the shared pieces are the label helpers, not the machines.
 *
 * AC4 — no mutation before acceptance: `pickAndAnalyze` only reads; the
 * library cache is invalidated ONLY after a successful `acceptCreation`.
 * `abandon` is a pure frontend reset.
 */
export function useStructuredCreation(): UseStructuredCreation {
  const [status, setStatus] = useState<StructuredCreationStatus>({
    kind: "idle",
  });

  const statusRef = useRef<StructuredCreationStatus>(status);
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
  // `analyzing` / `creating` only after a state flush, so a double
  // activation in the same tick would otherwise start two flows.
  const inFlightRef = useRef(false);

  const pickAndAnalyze = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    // Capture the FULL status the user was looking at before re-opening the
    // picker: a cancelled dialog must restore it verbatim — never silently
    // wipe an in-progress `review` verdict nor a `failed` alert.
    const priorStatus = statusRef.current;
    try {
      if (mountedRef.current) setStatus({ kind: "analyzing" });
      let verdict: StructuredCreationAnalysis;
      try {
        verdict = await analyzeStructuredFolderForCreation();
      } catch (err) {
        if (!mountedRef.current) return;
        setStatus({ kind: "failed", error: toAppError(err) });
        return;
      }
      if (!mountedRef.current) return;
      if (verdict.kind === "cancelled") {
        // Restore the complete pre-existing status — a cancel is a no-op.
        setStatus(priorStatus);
        return;
      }
      setStatus({ kind: "review", verdict });
    } finally {
      inFlightRef.current = false;
    }
  }, []);

  const acceptCreation = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    const current = statusRef.current;
    if (current.kind !== "review") return;
    // A blocked verdict has nothing to create — only `Abandonner`.
    if (!current.verdict.creatableSummary) return;

    inFlightRef.current = true;
    try {
      if (mountedRef.current) setStatus({ kind: "creating" });
      let story: StoryCardDto;
      try {
        story = await acceptStructuredCreation({
          folderPath: current.verdict.folderPath,
        });
      } catch (err) {
        if (!mountedRef.current) return;
        setStatus({ kind: "failed", error: toAppError(err) });
        return;
      }
      // The canonical store HAS changed — drop the stale overview snapshot
      // BEFORE the mounted guard so an unmount mid-creation still
      // reconciles on the next library mount.
      invalidateLibraryOverviewCache();
      if (!mountedRef.current) return;
      setStatus({ kind: "created", story });
    } finally {
      inFlightRef.current = false;
    }
  }, []);

  const abandon = useCallback((): void => {
    // Pure frontend reset — nothing was mutated before acceptance (AC4).
    if (statusRef.current.kind === "review") {
      setStatus({ kind: "idle" });
    }
  }, []);

  const dismiss = useCallback((): void => {
    const kind = statusRef.current.kind;
    if (kind === "created" || kind === "failed") {
      setStatus({ kind: "idle" });
    }
  }, []);

  return {
    status,
    pickAndAnalyze,
    acceptCreation,
    abandon,
    dismiss,
  };
}
