import { useCallback, useEffect, useRef, useState } from "react";

import {
  readTransferState,
  startTransferStory,
  TransferContractDriftError,
} from "../../../ipc/commands/story-transfer";
import { subscribeJobEvents } from "../../../ipc/events/job-events";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
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
 * `job:progress` (both the `preflight` gate phase and the `transfer` write phase
 * map to it — the 3.4 scope shows a single calm "en transfert"); the terminal
 * `transferred` / `retryable` come from the AUTHORITATIVE re-read (never
 * reconstructed from events alone); `error` is a transport failure.
 *
 * `transferred` is a NON-SUCCESS terminal: the bytes were written, nothing is
 * verified yet. No success vocabulary is ever produced here.
 */
export type StoryTransferState =
  | { kind: "idle" }
  | { kind: "transferring"; storyId: string; progress: number | null }
  | { kind: "transferred"; storyId: string }
  | {
      kind: "retryable";
      storyId: string;
      message: string;
      userAction: string;
    }
  | { kind: "error"; storyId: string; error: AppError };

export interface UseStoryTransfer {
  state: StoryTransferState;
  /** Start sending `storyId` to `deviceIdentifier`. No-op if either is empty.
   *  Supersedes any transfer already tracked by this hook. */
  send: (storyId: string, deviceIdentifier: string) => void;
  /** Re-run the last transfer after a recoverable failure. */
  retry: () => void;
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

  // Authoritative re-read of the terminal state. Reaching a terminal (transferred
  // / retryable / error) SETTLES the job: it stops the live subscription and
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
          if (dto.kind === "transferred") {
            settle();
            setState({ kind: "transferred", storyId: sid });
          } else if (dto.kind === "retryable") {
            settle();
            setState({
              kind: "retryable",
              storyId: sid,
              message: dto.message,
              userAction: dto.userAction,
            });
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
      setState({ kind: "transferring", storyId, progress: null });

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
              // 3.4 scope: both the preflight gate and the transfer write map to
              // a single calm "en transfert" phase (the rich phase split is 3.5).
              setState({ kind: "transferring", storyId, progress: event.progress });
            },
            onCompleted: (event) => {
              if (!mountedRef.current || callId !== activeJobRef.current) return;
              teardown();
              // AC3 — the device is the truth at terminal: render `transferred`
              // ONLY when the authoritative re-read CONFIRMS it. If the re-read
              // folds to idle (device unplugged / state unprovable), do NOT claim
              // "écriture effectuée" — surface an honest, recoverable UNCONFIRMED
              // terminal. Re-running is safe: the write is idempotent.
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
              // Preserve the recoverable failure even if the re-read can no
              // longer confirm a state (device left mid-write → idle).
              reread(callId, event.jobId, storyId, deviceIdentifier, () =>
                setState({
                  kind: "retryable",
                  storyId,
                  message: event.errorMessage,
                  userAction: event.userAction,
                }),
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

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      teardown();
    };
  }, [teardown]);

  return { state, send, retry };
}
