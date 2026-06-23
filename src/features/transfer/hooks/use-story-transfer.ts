import { useCallback, useEffect, useRef, useState } from "react";

import {
  readTransferState,
  startTransferStory,
  TransferContractDriftError,
} from "../../../ipc/commands/story-transfer";
import { subscribeJobEvents } from "../../../ipc/events/job-events";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { TransferVerifiedSummary } from "../../../shared/ipc-contracts/story-transfer";
import { useJobShell } from "../../../shell/state/job-shell-store";

/** `jobType` of the transfer flow — mirrors the Rust `JOB_TYPE_TRANSFER_STORY`. */
const JOB_TYPE_TRANSFER_STORY = "transfer_story";

const DRIFT_ERROR: AppError = {
  code: "TRANSFER_FAILED",
  message: "Envoi indisponible: réponse invalide.",
  userAction:
    "Réessaie l'envoi. Si le problème persiste, signale-le avec les traces locales.",
  details: null,
};

/**
 * UI state of the transfer flow. Every non-idle variant carries the `storyId` it
 * targets so the surface stays tied to the STORY being sent, not the transient
 * library selection: an in-flight write or a recoverable failure stays
 * consultable when the user selects another story. `transferring` is driven by
 * `job:progress` (the `preflight` gate, the `transfer` write AND the final
 * `verify` phase all map to it — the panel names the phase); the terminals come
 * from the events + the AUTHORITATIVE re-read (never reconstructed from events
 * alone); `error` is a transport failure.
 *
 * `verified` is the ONLY success terminal (`transférée et vérifiée`) — reached
 * solely when the verify phase PROVED the write. `partial` (`état partiel`) and
 * the verify `failed` verdict (rendered as `retryable` / `échec récupérable`) are
 * honest non-successes, never dressed up as a success.
 */
export type StoryTransferState =
  | { kind: "idle" }
  | {
      kind: "transferring";
      storyId: string;
      progress: number | null;
      /** The live phase ("preflight" gate vs "transfer" write) so the detail can
       *  name it honestly even when no reliable % is known (AC1). */
      phase: string | null;
    }
  | {
      // Verify CONFIRMED the write — the success terminal `transférée et vérifiée`.
      // `summary` carries the AC2 confirmation lines (what changed / stayed
      // unchanged), composed in Rust and rendered verbatim. The ONLY place success
      // vocabulary is ever produced, and only after proof.
      kind: "verified";
      storyId: string;
      summary: TransferVerifiedSummary;
    }
  | {
      // Verify found the device mutated + present but INCOHERENT — `état partiel`.
      // A non-success, never a silent success; distinct from `incomplete`
      // (`transfert incomplet`, a write interruption) and from `retryable`.
      // It carries NO structured `cause`: per the F6 contract a verify terminal
      // ships ONLY `verifyVerdict`, never a write-phase `completeness` / `cause`.
      kind: "partial";
      storyId: string;
      message: string;
      userAction: string;
    }
  | {
      kind: "retryable";
      storyId: string;
      /** Structured failure cause (AC3), when the event carried one. */
      cause?: string;
      message: string;
      userAction: string;
    }
  | {
      // The write STARTED then was interrupted (the device was mutated): the Lunii
      // may hold a partial copy; a relance (full cycle) restores a safe state
      // (AC2). Distinct from `retryable` (device left untouched → `échec
      // récupérable`) and from `transferred` (no success is ever claimed here).
      kind: "incomplete";
      storyId: string;
      cause?: string;
      message: string;
      userAction: string;
    }
  | { kind: "error"; storyId: string; error: AppError };

export interface UseStoryTransfer {
  state: StoryTransferState;
  /** Start sending `storyId` to `deviceIdentifier`. No-op if either is empty.
   *  Supersedes any transfer already tracked by this hook. */
  send: (storyId: string, deviceIdentifier: string) => void;
  /** Re-run the last transfer after a recoverable failure (a full new cycle —
   *  never a hidden partial resume). */
  retry: () => void;
  /** Abandon the current outcome: return to `idle` WITHOUT clearing the last
   *  request, so `retry()` / `send()` stay available. Wired to the "Abandonner"
   *  action on a `partial` / `retryable` / `incomplete` terminal; the local draft
   *  is never touched (AC3). */
  dismiss: () => void;
}

/**
 * Track ONE story transfer, independent of the library selection. The hook is
 * USER-TRIGGERED (`send()`) and does NOT reset on selection change. It is a clone
 * of `useStoryPreparation`, with the transfer wire contract and a non-success
 * terminal:
 * - `send()` starts the background write, subscribes to the correlated `job:*`
 *   events for the live phase, and on a TERMINAL event performs an authoritative
 *   re-read (`read_transfer_state`).
 * - **Catch-up re-read (race-proof):** an immediate authoritative re-read runs
 *   right after subscribing, so a fast write that finished before the
 *   subscription registered never leaves the UI stuck on the optimistic phase.
 * - `transferred` is rendered ONLY when the authoritative re-read CONFIRMS it
 *   (the device is the truth at terminal — AC3). A `job:completed` whose re-read
 *   folds to idle (device gone, state unprovable) becomes an honest recoverable
 *   "non confirmé" terminal, NEVER a claimed write.
 * - a `job:failed` whose re-read can no longer confirm a state (idle) keeps the
 *   recoverable failure message from the event, so the "échec récupérable" stays
 *   actionable even when the device left mid-write.
 * - StrictMode-safe active-job guard + unsubscribe on unmount / supersession.
 */
export function useStoryTransfer(): UseStoryTransfer {
  const [state, setState] = useState<StoryTransferState>({ kind: "idle" });
  const activeJobRef = useRef(0);
  const mountedRef = useRef(true);
  const unsubscribeRef = useRef<(() => void) | null>(null);
  const lastRequestRef = useRef<{
    storyId: string;
    deviceIdentifier: string;
  } | null>(null);
  // The callId whose transfer has reached a terminal via a re-read. Guards
  // against a late `job:progress` regressing the panel out of the terminal.
  const settledRef = useRef(0);
  const trackJobProgress = useJobShell((s) => s.trackJobProgress);
  const clearJob = useJobShell((s) => s.clearJob);

  const teardown = useCallback(() => {
    if (unsubscribeRef.current) {
      unsubscribeRef.current();
      unsubscribeRef.current = null;
    }
  }, []);

  // Authoritative re-read of the terminal state. Reaching a terminal (verified /
  // retryable / error) SETTLES the job: it stops the live subscription and
  // marks the call settled, so a late `job:progress` can never regress the panel
  // back to a transient phase. `onIdle` runs when the re-read cannot yet derive a
  // definitive state (device gone): the caller keeps the event-derived outcome
  // and the subscription stays alive so the live phases keep flowing.
  const reread = useCallback(
    (
      callId: number,
      jobId: string,
      sid: string,
      did: string,
      onIdle: () => void,
    ) => {
      const settle = () => {
        settledRef.current = callId;
        teardown();
        clearJob(jobId);
      };
      // Pin the authoritative re-read to the TARGETED device: it must prove the
      // pack on the Lunii this transfer aimed at, never on any other writable
      // device connected at the terminal — else a multi-Lunii swap could mask a
      // failed write as "transferred" or attribute the terminal to the wrong
      // device (AC3).
      readTransferState({ storyId: sid, deviceIdentifier: did })
        .then((dto) => {
          // Once a re-read has SETTLED this job, ignore any other in-flight
          // re-read for the same call: the catch-up and the terminal re-read can
          // both resolve, and a second settle/setState would re-scan needlessly
          // and could flip an already-settled terminal.
          if (
            !mountedRef.current ||
            callId !== activeJobRef.current ||
            settledRef.current === callId
          ) {
            return;
          }
          if (dto.kind === "verified") {
            settle();
            // The authoritative re-read PROVED the write (indexed + content present
            // + byte-faithful): the success terminal, carrying the AC2 summary lines
            // composed in Rust.
            setState({ kind: "verified", storyId: sid, summary: dto.summary });
          } else if (dto.kind === "retryable") {
            settle();
            // `read_transfer_state` normally folds to idle/verified, but if it ever
            // carries the device-mutation nuance, honor it.
            setState(
              dto.completeness === "incomplete"
                ? {
                    kind: "incomplete",
                    storyId: sid,
                    message: dto.message,
                    userAction: dto.userAction,
                  }
                : {
                    kind: "retryable",
                    storyId: sid,
                    message: dto.message,
                    userAction: dto.userAction,
                  },
            );
          } else {
            // idle / transferring — defer: let the event-derived outcome or the
            // live events decide.
            onIdle();
          }
        })
        .catch((err) => {
          if (
            !mountedRef.current ||
            callId !== activeJobRef.current ||
            settledRef.current === callId
          ) {
            return;
          }
          settle();
          setState({
            kind: "error",
            storyId: sid,
            error:
              err instanceof TransferContractDriftError
                ? DRIFT_ERROR
                : toAppError(err),
          });
        });
    },
    [clearJob, teardown],
  );

  const start = useCallback(
    (storyId: string, deviceIdentifier: string) => {
      const callId = ++activeJobRef.current;
      lastRequestRef.current = { storyId, deviceIdentifier };
      teardown();
      // Optimistic: the write starts in flight. Live events refine the progress.
      setState({ kind: "transferring", storyId, progress: null, phase: null });

      startTransferStory({ storyId, deviceIdentifier })
        .then((acceptance) => {
          if (!mountedRef.current || callId !== activeJobRef.current) return;
          const unsubscribe = subscribeJobEvents({
            jobId: acceptance.jobId,
            jobType: JOB_TYPE_TRANSFER_STORY,
            targetStoryId: storyId,
            onProgress: (event) => {
              // Ignore a live phase once a re-read has settled this job to a
              // terminal — a late event must never regress out of the terminal.
              if (
                !mountedRef.current ||
                callId !== activeJobRef.current ||
                settledRef.current === callId
              ) {
                return;
              }
              trackJobProgress({
                jobId: event.jobId,
                jobType: event.jobType,
                targetStoryId: event.targetStoryId,
                phase: event.phase,
                progress: event.progress,
                sequence: event.sequence,
              });
              // The phase ("preflight" gate vs "transfer" write) is carried so the
              // detail can name it honestly (AC1) when no reliable % is known.
              setState({
                kind: "transferring",
                storyId,
                progress: event.progress,
                phase: event.phase,
              });
            },
            onCompleted: (event) => {
              if (!mountedRef.current || callId !== activeJobRef.current) return;
              teardown();
              // F1 — `job:completed` fires ONLY when verify CONFIRMED the write, and
              // carries the AC2 summary ON the terminal. Settle the verified success
              // STRAIGHT from the event: never via a re-read with the now-stale
              // pre-write identifier (the write mutated `.pi`, so that identifier no
              // longer resolves the device → the re-read would fold to idle and lose
              // a legitimate success). The summary is composed in Rust, rendered
              // verbatim.
              if (event.summary) {
                settledRef.current = callId;
                clearJob(event.jobId);
                setState({ kind: "verified", storyId, summary: event.summary });
                return;
              }
              // Defensive fallback (a transfer completion always carries a summary):
              // an authoritative re-read, and an honest unconfirmed terminal if it
              // cannot derive a definitive success.
              reread(callId, event.jobId, storyId, deviceIdentifier, () =>
                setState({
                  kind: "retryable",
                  storyId,
                  message: "Transfert terminé mais non confirmé sur l'appareil.",
                  userAction: "Rebranche la Lunii et relance pour confirmer.",
                }),
              );
            },
            onFailed: (event) => {
              if (!mountedRef.current || callId !== activeJobRef.current) return;
              teardown();
              // AC2/AC3 — the failure terminal is AUTHORITATIVE from the event: a
              // `job:failed` must NEVER be flipped to a success by a re-read.
              // Settle directly from the event, distinguishing FOUR honest
              // non-successes by their discriminant:
              //   - verify `partial`  → `état partiel` (mutated + present but incoherent)
              //   - verify `failed`   → `échec récupérable` (falls through to retryable)
              //   - write `incomplete`→ `transfert incomplet` (a write interruption)
              //   - write `failed`    → `échec récupérable` (device untouched)
              settledRef.current = callId;
              clearJob(event.jobId);
              setState(
                event.verifyVerdict === "partial"
                  ? {
                      // A verify `partial` terminal carries NO write `cause` (F6):
                      // `event.cause` is always undefined here, so it is omitted.
                      kind: "partial",
                      storyId,
                      message: event.errorMessage,
                      userAction: event.userAction,
                    }
                  : event.completeness === "incomplete"
                    ? {
                        kind: "incomplete",
                        storyId,
                        cause: event.cause,
                        message: event.errorMessage,
                        userAction: event.userAction,
                      }
                    : {
                        kind: "retryable",
                        storyId,
                        cause: event.cause,
                        message: event.errorMessage,
                        userAction: event.userAction,
                      },
              );
            },
          });
          // If superseded / unmounted while the acceptance was in flight, detach.
          if (!mountedRef.current || callId !== activeJobRef.current) {
            unsubscribe();
            return;
          }
          unsubscribeRef.current = unsubscribe;
          // Catch-up: a write that finished before this subscription registered
          // would otherwise leave the panel on the optimistic phase. An
          // authoritative re-read reconciles to the terminal regardless; if it
          // can't yet derive one (idle), the live events still drive the phase.
          reread(callId, acceptance.jobId, storyId, deviceIdentifier, () => {});
        })
        .catch((err) => {
          if (!mountedRef.current || callId !== activeJobRef.current) return;
          setState({
            kind: "error",
            storyId,
            error:
              err instanceof TransferContractDriftError
                ? DRIFT_ERROR
                : toAppError(err),
          });
        });
    },
    [teardown, reread, trackJobProgress],
  );

  const send = useCallback(
    (storyId: string, deviceIdentifier: string) => {
      if (!storyId || !deviceIdentifier) return;
      start(storyId, deviceIdentifier);
    },
    [start],
  );

  const retry = useCallback(() => {
    const last = lastRequestRef.current;
    if (last) start(last.storyId, last.deviceIdentifier);
  }, [start]);

  const dismiss = useCallback(() => {
    // Abandon the current outcome (AC3). Supersede any in-flight callbacks (a
    // pending re-read for the old call becomes a no-op), stop the live
    // subscription, and return to idle WITHOUT clearing `lastRequestRef` so a
    // later `retry()` / `send()` stays possible. The local draft is never touched.
    activeJobRef.current += 1;
    teardown();
    setState({ kind: "idle" });
  }, [teardown]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      teardown();
    };
  }, [teardown]);

  return { state, send, retry, dismiss };
}
