import { useCallback, useEffect, useRef, useState } from "react";

import {
  acceptStructuredCreation,
  analyzeStructuredFolderForCreation,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";
import type {
  DropAnalysis,
  StructuredCreationAnalysis,
} from "../../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";

/** The `analyzed` verdict variant — what the `review` state renders. */
export type AnalyzedFolderVerdict = Extract<
  StructuredCreationAnalysis,
  { kind: "analyzed" }
>;

/** The drop channel's `folder` wire verdict — the exact field set of
 *  [`AnalyzedFolderVerdict`] under its own tag (`Drop Intent Contract`). */
export type DropFolderVerdict = Extract<DropAnalysis, { kind: "folder" }>;

export type StructuredCreationStatus =
  | { kind: "idle" }
  | { kind: "analyzing" }
  | { kind: "review"; verdict: AnalyzedFolderVerdict }
  | { kind: "creating" }
  | { kind: "created"; story: StoryCardDto }
  | { kind: "failed"; error: AppError };

/** Which entry point fed the CURRENT flow: the native folder picker
 *  (`picker`) or the drop channel (`drop`). The route branches
 *  `Réessayer` / `Fermer` / `Abandonner` on it — a drop-origin retry
 *  replays the pending intent or re-commits, never the picker. */
export type StructuredCreationOrigin = "picker" | "drop";

/** Which phase a `failed` status came from: the analysis (`analyze` — a
 *  picker retry re-opens the picker, a drop retry re-pulls the intent) or
 *  the commit (`accept` — a drop retry re-runs the accept with the
 *  PRESERVED verdict; the one-shot intent is long consumed). */
export type StructuredCreationFailedPhase = "analyze" | "accept";

export interface UseStructuredCreation {
  status: StructuredCreationStatus;
  /** Origin of the current flow (meaningful while `status` is not idle). */
  origin: StructuredCreationOrigin;
  /** Phase of the current `failed` status — `null` outside `failed`. */
  failedPhase: StructuredCreationFailedPhase | null;
  /** Open the native FOLDER picker (owned by Rust) and analyze the chosen
   *  structured folder. A cancelled dialog is a silent no-op. Resolves when
   *  the analysis has settled so callers/tests can chain a step. */
  pickAndAnalyze(): Promise<void>;
  /** Inject an ALREADY-SETTLED drop `folder` verdict into the review (the
   *  drop pull is silent by contract: no transient `analyzing` state — the
   *  verdict lands DIRECTLY in `review`, replacing whatever the machine
   *  showed; the drop channel's newest settlement is the only one
   *  displayed). DECLINED (returns `false`, nothing touched) while a
   *  commit is in flight: the machine may not be rewritten mid-commit —
   *  the displayed `creating` state and the verdict preserved for a
   *  possible `retryAccept` must both survive; the route then renders the
   *  frozen busy copy (the one-shot verdict cannot be re-served — the
   *  user re-drops). */
  injectDropVerdict(verdict: DropFolderVerdict): boolean;
  /** Reset the machine IFF its current surface came from the drop channel
   *  — called by the route when a NEWER drop settlement landed in the
   *  sibling import machine (last gesture wins across the channel). A
   *  picker-origin surface is never touched, and a machine mid-commit is
   *  never touched either (same in-flight guard as the injection). */
  clearDropReview(): void;
  /** Commit the analyzed folder from a `review` state (`Créer l'histoire`).
   *  No-op outside `review`, or when the verdict is blocked (nothing
   *  creatable). Rust re-analyzes the disk from zero. */
  acceptCreation(): Promise<void>;
  /** Re-run the accept phase with the PRESERVED verdict after a failed
   *  commit (`failedPhase === "accept"`). No-op elsewhere. The drop-origin
   *  `Réessayer` path — a re-pull would answer `none` and retry nothing. */
  retryAccept(): Promise<void>;
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
 * AC4 — no mutation before acceptance: `pickAndAnalyze` only reads and
 * `injectDropVerdict` only carries an already-pure verdict; the library
 * cache is invalidated ONLY after a successful `acceptCreation`.
 * `abandon` is a pure frontend reset.
 */
export function useStructuredCreation(): UseStructuredCreation {
  const [status, setStatus] = useState<StructuredCreationStatus>({
    kind: "idle",
  });
  const [origin, setOrigin] = useState<StructuredCreationOrigin>("picker");
  const [failedPhase, setFailedPhase] =
    useState<StructuredCreationFailedPhase | null>(null);

  const statusRef = useRef<StructuredCreationStatus>(status);
  statusRef.current = status;
  const originRef = useRef<StructuredCreationOrigin>(origin);
  originRef.current = origin;
  const failedPhaseRef = useRef<StructuredCreationFailedPhase | null>(
    failedPhase,
  );
  failedPhaseRef.current = failedPhase;

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

  // The verdict behind the current commit attempt, preserved across a
  // failed accept so `retryAccept` can re-run the commit (the one-shot
  // drop intent is long consumed by then). Purged on success and on the
  // explicit terminal gestures.
  const reviewVerdictRef = useRef<AnalyzedFolderVerdict | null>(null);

  const pickAndAnalyze = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    // Capture the FULL status the user was looking at before re-opening the
    // picker: a cancelled dialog must restore it verbatim — never silently
    // wipe an in-progress `review` verdict nor a `failed` alert. The origin
    // and failed phase travel WITH the status: a restored alert keeps its
    // own entry point and retry semantics.
    const priorStatus = statusRef.current;
    const priorOrigin = originRef.current;
    const priorFailedPhase = failedPhaseRef.current;
    try {
      if (mountedRef.current) {
        setOrigin("picker");
        setFailedPhase(null);
        setStatus({ kind: "analyzing" });
      }
      let verdict: StructuredCreationAnalysis;
      try {
        verdict = await analyzeStructuredFolderForCreation();
      } catch (err) {
        if (!mountedRef.current) return;
        setStatus({ kind: "failed", error: toAppError(err) });
        setFailedPhase("analyze");
        return;
      }
      if (!mountedRef.current) return;
      if (verdict.kind === "cancelled") {
        // Restore the complete pre-existing status — a cancel is a no-op.
        setStatus(priorStatus);
        setOrigin(priorOrigin);
        setFailedPhase(priorFailedPhase);
        return;
      }
      setStatus({ kind: "review", verdict });
    } finally {
      inFlightRef.current = false;
    }
  }, []);

  const injectDropVerdict = useCallback(
    (verdict: DropFolderVerdict): boolean => {
      if (!mountedRef.current) return false;
      // A commit in flight OWNS the machine: injecting would overwrite
      // the displayed `creating` state AND wipe the verdict preserved for
      // a possible `retryAccept` (its `Réessayer` would silently do
      // nothing). Declined — the route renders the frozen busy copy, the
      // calm refusal the signal gate would have rendered had the live
      // flow been visible when the signal arrived.
      if (inFlightRef.current) return false;
      // The drop pull is SILENT by contract: the settled verdict lands
      // DIRECTLY in review — no transient analyzing state ever renders.
      // The newest drop settlement replaces whatever the machine showed
      // (the user's last gesture wins).
      reviewVerdictRef.current = null;
      setOrigin("drop");
      setFailedPhase(null);
      setStatus({ kind: "review", verdict: { ...verdict, kind: "analyzed" } });
      return true;
    },
    [],
  );

  const clearDropReview = useCallback((): void => {
    // Internal supersede, never a user gesture: only a DROP-origin surface
    // steps aside for the channel's newer settlement — and never a machine
    // mid-commit (the same in-flight guard as the injection: rewriting a
    // `creating` state would mask the commit and orphan its retry).
    if (inFlightRef.current) return;
    if (originRef.current !== "drop") return;
    if (statusRef.current.kind === "idle") return;
    reviewVerdictRef.current = null;
    setStatus({ kind: "idle" });
    setFailedPhase(null);
  }, []);

  /** Shared commit path of `acceptCreation` and `retryAccept`: preserve
   *  the verdict for a later retry, run the UNCHANGED accept phase, tag a
   *  failure as the `accept` phase. */
  const commitVerdict = useCallback(
    async (verdict: AnalyzedFolderVerdict): Promise<void> => {
      // A blocked verdict has nothing to create — only `Abandonner`.
      if (!verdict.creatableSummary) return;

      inFlightRef.current = true;
      reviewVerdictRef.current = verdict;
      try {
        if (mountedRef.current) {
          setFailedPhase(null);
          setStatus({ kind: "creating" });
        }
        let story: StoryCardDto;
        try {
          story = await acceptStructuredCreation({
            folderPath: verdict.folderPath,
          });
        } catch (err) {
          if (!mountedRef.current) return;
          setStatus({ kind: "failed", error: toAppError(err) });
          setFailedPhase("accept");
          return;
        }
        // The canonical store HAS changed — drop the stale overview snapshot
        // BEFORE the mounted guard so an unmount mid-creation still
        // reconciles on the next library mount.
        invalidateLibraryOverviewCache();
        reviewVerdictRef.current = null;
        if (!mountedRef.current) return;
        setStatus({ kind: "created", story });
      } finally {
        inFlightRef.current = false;
      }
    },
    [],
  );

  const acceptCreation = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    const current = statusRef.current;
    if (current.kind !== "review") return;
    await commitVerdict(current.verdict);
  }, [commitVerdict]);

  const retryAccept = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    if (statusRef.current.kind !== "failed") return;
    if (failedPhaseRef.current !== "accept") return;
    const verdict = reviewVerdictRef.current;
    if (!verdict) return;
    await commitVerdict(verdict);
  }, [commitVerdict]);

  const abandon = useCallback((): void => {
    // Pure frontend reset — nothing was mutated before acceptance (AC4).
    if (statusRef.current.kind === "review") {
      reviewVerdictRef.current = null;
      setStatus({ kind: "idle" });
      setFailedPhase(null);
    }
  }, []);

  const dismiss = useCallback((): void => {
    const kind = statusRef.current.kind;
    if (kind === "created" || kind === "failed") {
      reviewVerdictRef.current = null;
      setStatus({ kind: "idle" });
      setFailedPhase(null);
    }
  }, []);

  return {
    status,
    origin,
    failedPhase,
    pickAndAnalyze,
    injectDropVerdict,
    clearDropReview,
    acceptCreation,
    retryAccept,
    abandon,
    dismiss,
  };
}
