import { useCallback, useEffect, useRef, useState } from "react";

import {
  PreparationContractDriftError,
  readPreparationState,
  startPrepareStory,
} from "../../../ipc/commands/story-preparation";
import { subscribeJobEvents } from "../../../ipc/events/job-events";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { ValidationBlocker } from "../../../shared/ipc-contracts/story-validation";
import { useJobShell } from "../../../shell/state/job-shell-store";

/** `jobType` of the preparation flow — mirrors the Rust `JOB_TYPE_PREPARE_STORY`. */
const JOB_TYPE_PREPARE_STORY = "prepare_story";

const DRIFT_ERROR: AppError = {
  code: "PREPARATION_FAILED",
  message: "Préparation indisponible: réponse invalide.",
  userAction:
    "Réessaie la préparation. Si le problème persiste, signale-le avec les traces locales.",
  details: null,
};

/**
 * UI state of the preparation flow. Every non-idle variant carries the
 * `storyId` it targets so the surface stays tied to the STORY being prepared,
 * not the transient library selection: an in-flight job or a recoverable failure
 * stays consultable when the user selects another story (AC2). `preflight` /
 * `preparing` are driven by `job:progress`; the terminal `prepared` / `retryable`
 * come from the AUTHORITATIVE re-read (never reconstructed from events alone);
 * `error` is a transport failure of the start or the re-read.
 */
export type StoryPreparationState =
  | { kind: "idle" }
  | { kind: "preflight"; storyId: string }
  | { kind: "preparing"; storyId: string; progress: number | null }
  | { kind: "prepared"; storyId: string }
  | {
      kind: "retryable";
      storyId: string;
      message: string;
      userAction: string;
      blockers: ValidationBlocker[];
    }
  | { kind: "error"; storyId: string; error: AppError };

export interface UseStoryPreparation {
  state: StoryPreparationState;
  /** Start preparing `storyId` for `deviceIdentifier`. No-op if either is empty.
   *  Supersedes any preparation already tracked by this hook. */
  prepare: (storyId: string, deviceIdentifier: string) => void;
  /** Re-run the last preparation after a recoverable failure. */
  retry: () => void;
}

/**
 * Track ONE story preparation, independent of the library selection. The hook is
 * USER-TRIGGERED (`prepare()`) and does NOT reset on selection change: a job
 * started for story A keeps running and stays consultable when the user selects
 * story B (the route renders A's surface only while A is selected, and A's card
 * keeps its badge). It supersedes only on a new `prepare()` or on unmount.
 *
 * Guardrails:
 * - `prepare()` starts the background job, subscribes to the correlated `job:*`
 *   events for the live phase, and on a TERMINAL event performs an authoritative
 *   re-read (`read_preparation_state`).
 * - **Catch-up re-read (race-proof):** because the MVP job can finish before the
 *   subscription is fully registered, an immediate authoritative re-read runs
 *   right after subscribing, so the UI never stays stuck on the optimistic
 *   `preflight` when the terminal event was missed.
 * - a `job:failed` whose re-read can no longer confirm a device (idle) keeps the
 *   recoverable failure message from the event, so AC3's "échec récupérable"
 *   survives a device that left mid-preparation.
 * - StrictMode-safe active-job guard + unsubscribe on unmount / supersession.
 */
export function useStoryPreparation(): UseStoryPreparation {
  const [state, setState] = useState<StoryPreparationState>({ kind: "idle" });
  const activeJobRef = useRef(0);
  const mountedRef = useRef(true);
  const unsubscribeRef = useRef<(() => void) | null>(null);
  const lastRequestRef = useRef<{
    storyId: string;
    deviceIdentifier: string;
  } | null>(null);
  // The callId whose preparation has reached a terminal via a re-read. Guards
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

  // Authoritative re-read of the terminal state. Reaching a terminal (prepared /
  // retryable / error) SETTLES the job: it stops the live subscription and marks
  // the call settled, so a late `job:progress` — e.g. delivered after the
  // catch-up re-read already reached the terminal — can never regress the panel
  // back to a transient phase. `onIdle` runs when the re-read cannot yet derive a
  // definitive state (device gone): the caller keeps the event-derived outcome or
  // waits, and the subscription stays alive so the live phases keep flowing.
  const reread = useCallback(
    (callId: number, jobId: string, sid: string, onIdle: () => void) => {
      const settle = () => {
        settledRef.current = callId;
        teardown();
        clearJob(jobId);
      };
      readPreparationState({ storyId: sid })
        .then((dto) => {
          if (!mountedRef.current || callId !== activeJobRef.current) return;
          if (dto.kind === "prepared") {
            settle();
            setState({ kind: "prepared", storyId: sid });
          } else if (dto.kind === "retryable") {
            settle();
            setState({
              kind: "retryable",
              storyId: sid,
              message: dto.message,
              userAction: dto.userAction,
              blockers: dto.blockers,
            });
          } else {
            // idle (or the transient phases the command never returns) — defer:
            // do not settle; let the event-derived outcome / live events decide.
            onIdle();
          }
        })
        .catch((err) => {
          if (!mountedRef.current || callId !== activeJobRef.current) return;
          settle();
          setState({
            kind: "error",
            storyId: sid,
            error:
              err instanceof PreparationContractDriftError
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
      // Optimistic: the job starts with a preflight phase. Live events refine it.
      setState({ kind: "preflight", storyId });

      startPrepareStory({ storyId, deviceIdentifier })
        .then((acceptance) => {
          if (!mountedRef.current || callId !== activeJobRef.current) return;
          const unsubscribe = subscribeJobEvents({
            jobId: acceptance.jobId,
            jobType: JOB_TYPE_PREPARE_STORY,
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
              setState(
                event.phase === "preflight"
                  ? { kind: "preflight", storyId }
                  : { kind: "preparing", storyId, progress: event.progress },
              );
            },
            onCompleted: (event) => {
              if (!mountedRef.current || callId !== activeJobRef.current) return;
              teardown();
              reread(callId, event.jobId, storyId, () =>
                setState({ kind: "prepared", storyId }),
              );
            },
            onFailed: (event) => {
              if (!mountedRef.current || callId !== activeJobRef.current) return;
              teardown();
              // Preserve the recoverable failure even if the re-read can no longer
              // confirm a device (device left mid-preparation → idle).
              reread(callId, event.jobId, storyId, () =>
                setState({
                  kind: "retryable",
                  storyId,
                  message: event.errorMessage,
                  userAction: event.userAction,
                  blockers: [],
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
          // Catch-up: a job that finished before this subscription registered
          // would otherwise leave the panel on the optimistic preflight. An
          // authoritative re-read reconciles to the terminal regardless; if it
          // can't yet derive one (idle), the live events still drive the phase.
          reread(callId, acceptance.jobId, storyId, () => {});
        })
        .catch((err) => {
          if (!mountedRef.current || callId !== activeJobRef.current) return;
          setState({
            kind: "error",
            storyId,
            error:
              err instanceof PreparationContractDriftError
                ? DRIFT_ERROR
                : toAppError(err),
          });
        });
    },
    [teardown, reread, trackJobProgress],
  );

  const prepare = useCallback(
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

  return { state, prepare, retry };
}
