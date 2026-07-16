import { useCallback, useEffect, useRef, useState } from "react";

import {
  acceptArtifactImport,
  analyzeArtifactForImport,
  analyzeOsOpenRequest,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";
import type {
  AcceptArtifactImportInput,
  ImportArtifactAnalysis,
  OsOpenAnalysis,
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

/** Which entry point fed the CURRENT flow: the native picker (`dialog`) or
 *  the OS-open channel (`osOpen`). The route branches `Réessayer` /
 *  `Fermer` on it — an OS-open retry replays the pending intent instead of
 *  re-opening the picker. */
export type StoryImportOrigin = "dialog" | "osOpen";

/** Which phase a `failed` status came from: the analysis read
 *  (`analyze` — retrying re-pulls the still-pending Rust intent) or the
 *  commit (`accept` — retrying re-runs the accept with the PRESERVED
 *  verdict; the one-shot intent is long consumed). */
export type StoryImportFailedPhase = "analyze" | "accept";

/** What an OS-open analysis produced, for the caller: the two calm cases
 *  the MACHINE does not carry (`none` — total no-op; `multipleFiles` — the
 *  library renders the Rust copy `role="status"`), or the machine states
 *  it fed (`review` / `failed`). */
export type OsOpenAnalyzeOutcome =
  | { kind: "none" }
  | { kind: "multipleFiles"; message: string }
  | { kind: "review" }
  | { kind: "failed" };

export interface UseStoryImport {
  status: StoryImportStatus;
  /** Origin of the current flow (meaningful while `status` is not idle). */
  origin: StoryImportOrigin;
  /** Phase of the current `failed` status — `null` outside `failed`. */
  failedPhase: StoryImportFailedPhase | null;
  /** True while an OS-open settlement is in flight. The pull renders no
   *  transient state itself (silent by contract) — this is the INTERNAL
   *  busy the flows' mutual exclusion consumes so no sibling flow starts
   *  under a live OS read. */
  isOsOpenSettling: boolean;
  /** Open the native file picker (owned by Rust) and analyze the chosen
   *  `.rustory`. A cancelled dialog is a silent no-op. Resolves when the
   *  analysis has settled so callers/tests can chain a step. */
  pickAndAnalyze(): Promise<void>;
  /** Analyze the pending OS-open intent (NO dialog) through the SAME
   *  machine: an `analyzed` verdict lands in `review`, a read failure in
   *  `failed` (the intent stays pending Rust-side — a retry replays it).
   *  `none` and `multipleFiles` leave the current status untouched and are
   *  returned for the library to render. Calls are SERIALIZED (mono-slot):
   *  a pull landing while another settlement is in flight waits for it,
   *  then pulls — a signal is never dropped as a fake `none`. */
  analyzeFromOsOpen(): Promise<OsOpenAnalyzeOutcome>;
  /** Commit the recognized story from a `review` state. No-op outside
   *  `review`, or when the verdict is blocked (no importable content). */
  acceptRecognized(): Promise<void>;
  /** Re-run the accept phase with the PRESERVED verdict after a failed
   *  commit (`failedPhase === "accept"`). No-op elsewhere. */
  retryAccept(): Promise<void>;
  /** Abandon an analyzed import (pure frontend, NO mutation): drop the
   *  verdict and return to idle. No-op outside `review`. */
  abandon(): void;
  /** Dismiss a terminal status (`imported` / `failed`) back to idle. The
   *  `failed` alert is never wiped implicitly, but its explicit `Fermer`
   *  must work — exactly like `Réessayer`. Dismissing (like abandoning)
   *  INVALIDATES any in-flight OS-open settlement: an explicit terminal
   *  gesture is terminal — a late settlement never resurrects the flow. */
  dismiss(): void;
}

/**
 * Orchestrates the two-phase local-artifact import through the Rust-owned
 * dialog + analysis + commit boundary. Structural sibling of
 * `useStoryExport` / `useDeviceStoryImport`: same StrictMode-safe mount
 * flag and synchronous re-entrancy guard.
 *
 * AC1 — no mutation before acceptance: the analyses only read; the
 * library cache is invalidated ONLY after a successful commit.
 * `abandon` is a pure frontend reset.
 */
export function useStoryImport(): UseStoryImport {
  const [status, setStatus] = useState<StoryImportStatus>({ kind: "idle" });
  const [origin, setOrigin] = useState<StoryImportOrigin>("dialog");
  const [failedPhase, setFailedPhase] =
    useState<StoryImportFailedPhase | null>(null);
  const [isOsOpenSettling, setIsOsOpenSettling] = useState(false);

  const statusRef = useRef<StoryImportStatus>(status);
  statusRef.current = status;
  const originRef = useRef<StoryImportOrigin>(origin);
  originRef.current = origin;
  const failedPhaseRef = useRef<StoryImportFailedPhase | null>(failedPhase);
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
  // `analyzing` / `importing` only after a state flush, so a double
  // activation in the same tick would otherwise start two flows.
  const inFlightRef = useRef(false);

  // Explicit-terminal epoch: `Fermer` / `Abandonner` bump it, and any
  // OS-open settlement still in flight at that moment is DROPPED on
  // arrival — a flow the user explicitly closed never resurrects.
  const epochRef = useRef(0);

  // The verdict behind the current commit attempt, preserved across a
  // failed accept so `retryAccept` can re-run the commit (the one-shot
  // Rust intent is long consumed by then). Purged on success and on the
  // explicit terminal gestures.
  const reviewVerdictRef = useRef<AnalyzedVerdict | null>(null);

  const pickAndAnalyze = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    // Capture the FULL status the user was looking at before re-opening the
    // picker. A cancelled dialog must restore it verbatim — never silently
    // wipe an in-progress `review` verdict (its report) nor a `failed` alert
    // the user was still reading. The re-entrancy guard above guarantees this
    // is a settled status (never `analyzing` / `importing`). The origin and
    // failed phase travel WITH the status: a restored alert keeps its own
    // entry point and retry semantics.
    const priorStatus = statusRef.current;
    const priorOrigin = originRef.current;
    const priorFailedPhase = failedPhaseRef.current;
    try {
      if (mountedRef.current) {
        setOrigin("dialog");
        setFailedPhase(null);
        setStatus({ kind: "analyzing" });
      }
      let verdict: ImportArtifactAnalysis;
      try {
        verdict = await analyzeArtifactForImport();
      } catch (err) {
        if (!mountedRef.current) return;
        setStatus({ kind: "failed", error: toAppError(err) });
        setFailedPhase("analyze");
        return;
      }
      if (!mountedRef.current) return;
      if (verdict.kind === "cancelled") {
        // Restore the complete pre-existing status (review / failed / imported
        // / idle) — a cancel is a no-op, it never discards a verdict.
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

  const runOsOpenAnalysis =
    useCallback(async (): Promise<OsOpenAnalyzeOutcome> => {
      // Ultimate guard against a picker/commit racing in the same tick —
      // real overlaps are declined upstream by the route's busy gate, and
      // the Rust intent is untouched here (no invoke ran), so the next
      // pull still serves it: nothing is lost.
      if (inFlightRef.current) return { kind: "none" };
      inFlightRef.current = true;
      const epoch = epochRef.current;
      if (mountedRef.current) setIsOsOpenSettling(true);
      try {
        // The pull is SILENT by contract: most pulls (every library mount)
        // answer `none`, which must leave ZERO visual trace — so the
        // machine is only ever touched once a verdict actually exists. An
        // artifact verdict lands directly in `review`, an unreadable file
        // directly in `failed`; the analysis itself ran Rust-side.
        let verdict: OsOpenAnalysis;
        try {
          verdict = await analyzeOsOpenRequest();
        } catch (err) {
          // Transport failure (unreadable file): the intent STAYS pending
          // Rust-side — `Réessayer` replays it, `Fermer` discards it. A
          // settlement arriving after an explicit terminal gesture (epoch
          // moved) is dropped — never a resurrected alert.
          if (!mountedRef.current || epochRef.current !== epoch) {
            return { kind: "none" };
          }
          setOrigin("osOpen");
          setStatus({ kind: "failed", error: toAppError(err) });
          setFailedPhase("analyze");
          return { kind: "failed" };
        }
        if (!mountedRef.current || epochRef.current !== epoch) {
          return { kind: "none" };
        }
        if (verdict.kind === "none") {
          // Total silent no-op — whatever the user was looking at stays.
          return { kind: "none" };
        }
        if (verdict.kind === "multipleFiles") {
          // A calm limit the LIBRARY renders (`role="status"`) — never a
          // machine state: the current status survives untouched.
          return { kind: "multipleFiles", message: verdict.message };
        }
        setOrigin("osOpen");
        setStatus({ kind: "review", verdict });
        setFailedPhase(null);
        return { kind: "review" };
      } finally {
        inFlightRef.current = false;
        if (mountedRef.current) setIsOsOpenSettling(false);
      }
    }, []);

  // Mono-slot serialization of the OS-open pulls: each call waits for the
  // previous settlement, then pulls. Combined with the Rust-side
  // compare-and-take, the LAST settling pull serves the NEWEST intent —
  // a signal landing mid-pull is queued, never dropped as a fake `none`.
  const osOpenChainRef = useRef<Promise<unknown>>(Promise.resolve());
  const analyzeFromOsOpen = useCallback((): Promise<OsOpenAnalyzeOutcome> => {
    const run = osOpenChainRef.current.then(
      runOsOpenAnalysis,
      runOsOpenAnalysis,
    );
    osOpenChainRef.current = run;
    return run;
  }, [runOsOpenAnalysis]);

  /** Shared commit path of `acceptRecognized` and `retryAccept`: preserve
   *  the verdict for a later retry, run the UNCHANGED accept phase, tag a
   *  failure as the `accept` phase. */
  const commitVerdict = useCallback(
    async (verdict: AnalyzedVerdict): Promise<void> => {
      const content = verdict.importableContent;
      // A blocked verdict has no importable content — only `Abandonner`.
      if (!content) return;

      inFlightRef.current = true;
      reviewVerdictRef.current = verdict;
      const input: AcceptArtifactImportInput = {
        content,
        sourceName: verdict.sourceName,
        artifactChecksum: verdict.artifactChecksum,
      };
      try {
        if (mountedRef.current) {
          setFailedPhase(null);
          setStatus({ kind: "importing" });
        }
        let story: StoryCardDto;
        try {
          story = await acceptArtifactImport(input);
        } catch (err) {
          if (!mountedRef.current) return;
          setStatus({ kind: "failed", error: toAppError(err) });
          setFailedPhase("accept");
          return;
        }
        // The canonical store HAS changed — drop the stale overview snapshot
        // BEFORE the mounted guard so an unmount mid-import still reconciles.
        invalidateLibraryOverviewCache();
        reviewVerdictRef.current = null;
        if (!mountedRef.current) return;
        setStatus({ kind: "imported", story });
      } finally {
        inFlightRef.current = false;
      }
    },
    [],
  );

  const acceptRecognized = useCallback(async (): Promise<void> => {
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
    // Pure frontend reset — nothing was mutated before acceptance (AC1).
    if (statusRef.current.kind === "review") {
      epochRef.current += 1;
      reviewVerdictRef.current = null;
      setStatus({ kind: "idle" });
      setFailedPhase(null);
    }
  }, []);

  const dismiss = useCallback((): void => {
    const kind = statusRef.current.kind;
    if (kind === "imported" || kind === "failed") {
      // An explicit terminal gesture is TERMINAL: any OS-open settlement
      // still in flight is invalidated — a late verdict or a late error
      // must never resurrect what the user just closed.
      epochRef.current += 1;
      reviewVerdictRef.current = null;
      setStatus({ kind: "idle" });
      setFailedPhase(null);
    }
  }, []);

  return {
    status,
    origin,
    failedPhase,
    isOsOpenSettling,
    pickAndAnalyze,
    analyzeFromOsOpen,
    acceptRecognized,
    retryAccept,
    abandon,
    dismiss,
  };
}
