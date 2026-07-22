import { useCallback, useEffect, useRef, useState } from "react";

import {
  acceptStructuredArchiveCreation,
  analyzeStructuredArchiveForCreation,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";
import type { ArchiveCreationAnalysis } from "../../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";

/** The `analyzed` verdict variant — what the `review` state renders. */
export type AnalyzedArchiveVerdict = Extract<
  ArchiveCreationAnalysis,
  { kind: "analyzed" }
>;

export type ArchiveCreationStatus =
  | { kind: "idle" }
  | { kind: "analyzing" }
  | { kind: "review"; verdict: AnalyzedArchiveVerdict }
  | { kind: "creating" }
  | { kind: "created"; story: StoryCardDto }
  | { kind: "failed"; error: AppError };

/** Which phase a `failed` status came from: the analysis (`analyze` — a
 *  retry re-opens the picker) or the commit (`accept` — a retry re-runs
 *  the accept with the PRESERVED verdict). */
export type ArchiveCreationFailedPhase = "analyze" | "accept";

/** Which entry point fed the CURRENT flow: the native file picker
 *  (`picker`), the drop channel (`drop`) or the OS-open channel
 *  (`osOpen`). The route branches the terminal gestures on it (an
 *  external-origin abandon also discards the Rust-side intent trace). */
export type ArchiveCreationOrigin = "picker" | "drop" | "osOpen";

/** An externally-settled archive verdict (drop or OS-open wire tag): the
 *  exact field set of the picker `analyzed` verdict minus the tag. */
export type ExternalArchiveVerdict = Omit<AnalyzedArchiveVerdict, "kind">;

export interface UseArchiveCreation {
  status: ArchiveCreationStatus;
  /** Origin of the current flow (meaningful while `status` is not idle). */
  origin: ArchiveCreationOrigin;
  /** Phase of the current `failed` status — `null` outside `failed`. */
  failedPhase: ArchiveCreationFailedPhase | null;
  /** Open the native FILE picker (owned by Rust, `.zip` filter) and
   *  analyze the chosen archive. A cancelled dialog is a silent no-op. */
  pickAndAnalyze(): Promise<void>;
  /** Inject an ALREADY-SETTLED external verdict (drop / OS-open) into the
   *  review — the settlement is silent by contract: no transient
   *  `analyzing` state, the verdict lands DIRECTLY in `review`, replacing
   *  whatever the machine showed (the newest gesture wins). DECLINED
   *  (returns `false`, nothing touched) while a commit is in flight: the
   *  machine may not be rewritten mid-commit. */
  injectExternalVerdict(
    verdict: ExternalArchiveVerdict,
    origin: Exclude<ArchiveCreationOrigin, "picker">,
  ): boolean;
  /** Reset the machine IFF its current surface came from an EXTERNAL
   *  channel — called by the route when a newer settlement landed in a
   *  sibling machine (last gesture wins across channels). A picker-origin
   *  surface is never touched, nor a machine mid-commit. */
  clearExternalReview(): void;
  /** Commit the analyzed archive from a `review` state (`Créer
   *  l'histoire`). No-op outside `review`, or when the verdict is blocked.
   *  Rust re-analyzes the archive from zero. */
  acceptCreation(): Promise<void>;
  /** Re-run the accept phase with the PRESERVED verdict after a failed
   *  commit (`failedPhase === "accept"`). No-op elsewhere. */
  retryAccept(): Promise<void>;
  /** Abandon an analyzed archive (pure frontend, NO mutation). */
  abandon(): void;
  /** Dismiss a terminal status (`created` / `failed`) back to idle. */
  dismiss(): void;
}

/**
 * Orchestrates the two-phase structured-ARCHIVE creation through the
 * Rust-owned dialog + analysis + commit boundary. Structural sibling of
 * `useStructuredCreation` minus its drop channel — the duplication is
 * DELIBERATE (context orchestrators are never generic components).
 *
 * No mutation before acceptance: `pickAndAnalyze` only reads; the library
 * cache is invalidated ONLY after a successful `acceptCreation`;
 * `abandon` is a pure frontend reset.
 */
export function useArchiveCreation(): UseArchiveCreation {
  const [status, setStatus] = useState<ArchiveCreationStatus>({
    kind: "idle",
  });
  const [origin, setOrigin] = useState<ArchiveCreationOrigin>("picker");
  const [failedPhase, setFailedPhase] =
    useState<ArchiveCreationFailedPhase | null>(null);

  const statusRef = useRef<ArchiveCreationStatus>(status);
  statusRef.current = status;
  const originRef = useRef<ArchiveCreationOrigin>(origin);
  originRef.current = origin;
  const failedPhaseRef = useRef<ArchiveCreationFailedPhase | null>(
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
  // failed accept so `retryAccept` can re-run the commit. Purged on
  // success and on the explicit terminal gestures.
  const reviewVerdictRef = useRef<AnalyzedArchiveVerdict | null>(null);

  const pickAndAnalyze = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    // Capture the FULL status before re-opening the picker: a cancelled
    // dialog must restore it verbatim — never silently wipe an
    // in-progress `review` verdict nor a `failed` alert.
    const priorStatus = statusRef.current;
    const priorOrigin = originRef.current;
    const priorFailedPhase = failedPhaseRef.current;
    try {
      if (mountedRef.current) {
        setOrigin("picker");
        setFailedPhase(null);
        setStatus({ kind: "analyzing" });
      }
      let verdict: ArchiveCreationAnalysis;
      try {
        verdict = await analyzeStructuredArchiveForCreation();
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

  const injectExternalVerdict = useCallback(
    (
      verdict: ExternalArchiveVerdict,
      injectionOrigin: Exclude<ArchiveCreationOrigin, "picker">,
    ): boolean => {
      if (!mountedRef.current) return false;
      // A commit in flight OWNS the machine: injecting would overwrite
      // the displayed `creating` state AND wipe the verdict preserved for
      // a possible `retryAccept`. Declined — the route renders the frozen
      // busy copy (the one-shot settlement cannot be re-served; the user
      // re-drops / re-opens afterwards).
      if (inFlightRef.current) return false;
      // The external settlement is SILENT by contract: it lands DIRECTLY
      // in review — no transient analyzing state ever renders. The newest
      // settlement replaces whatever the machine showed.
      reviewVerdictRef.current = null;
      setOrigin(injectionOrigin);
      setFailedPhase(null);
      setStatus({ kind: "review", verdict: { ...verdict, kind: "analyzed" } });
      return true;
    },
    [],
  );

  const clearExternalReview = useCallback((): void => {
    // Internal supersede, never a user gesture: only an EXTERNAL-origin
    // surface steps aside for a sibling's newer settlement — and never a
    // machine mid-commit (rewriting a `creating` state would mask the
    // commit and orphan its retry).
    if (inFlightRef.current) return;
    if (originRef.current === "picker") return;
    if (statusRef.current.kind === "idle") return;
    reviewVerdictRef.current = null;
    setStatus({ kind: "idle" });
    setFailedPhase(null);
  }, []);

  /** Shared commit path of `acceptCreation` and `retryAccept`. */
  const commitVerdict = useCallback(
    async (verdict: AnalyzedArchiveVerdict): Promise<void> => {
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
          story = await acceptStructuredArchiveCreation({
            archivePath: verdict.archivePath,
          });
        } catch (err) {
          if (!mountedRef.current) return;
          setStatus({ kind: "failed", error: toAppError(err) });
          setFailedPhase("accept");
          return;
        }
        // The canonical store HAS changed — drop the stale overview
        // snapshot BEFORE the mounted guard so an unmount mid-creation
        // still reconciles on the next library mount.
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
    // Pure frontend reset — nothing was mutated before acceptance.
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
    injectExternalVerdict,
    clearExternalReview,
    acceptCreation,
    retryAccept,
    abandon,
    dismiss,
  };
}
