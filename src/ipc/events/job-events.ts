/**
 * Typed subscription helper for the long-running-job event channel. First
 * occupant of `src/ipc/events/` — the ONLY place that calls
 * `@tauri-apps/api/event::listen` for `job:*` events.
 *
 * Correlates by `(jobId, jobType, targetStoryId)`, validates every payload
 * against the wire guards, and enforces idempotence + ordering tolerance via the
 * MONOTONIC `sequence`: an event whose sequence is not strictly greater than the
 * last applied one is dropped (a duplicate or a late delivery never regresses the
 * state). Returns a synchronous unsubscribe that also cancels listeners still
 * being registered.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import {
  isJobCompletedEvent,
  isJobFailedEvent,
  isJobProgressEvent,
  type JobCompletedEvent,
  type JobFailedEvent,
  type JobProgressEvent,
} from "../../shared/ipc-contracts/story-preparation";

const EVENT_JOB_PROGRESS = "job:progress";
const EVENT_JOB_COMPLETED = "job:completed";
const EVENT_JOB_FAILED = "job:failed";

export interface JobSubscription {
  jobId: string;
  jobType: string;
  targetStoryId: string;
  onProgress: (event: JobProgressEvent) => void;
  onCompleted: (event: JobCompletedEvent) => void;
  onFailed: (event: JobFailedEvent) => void;
}

/**
 * Subscribe to the `job:*` events for one job. Returns an `unsubscribe` that
 * detaches the three listeners (and cancels any that are still registering).
 */
export function subscribeJobEvents(subscription: JobSubscription): () => void {
  let active = true;
  let lastSequence = -1;
  const unlisten: UnlistenFn[] = [];

  const matches = (
    jobId: string,
    jobType: string,
    targetStoryId: string,
  ): boolean =>
    jobId === subscription.jobId &&
    jobType === subscription.jobType &&
    targetStoryId === subscription.targetStoryId;

  // Idempotence + ordering tolerance: only a strictly newer sequence advances.
  const accept = (sequence: number): boolean => {
    if (sequence <= lastSequence) return false;
    lastSequence = sequence;
    return true;
  };

  const register = (
    name: string,
    handle: (payload: unknown) => void,
  ): void => {
    void listen<unknown>(name, (event) => {
      if (!active) return;
      handle(event.payload);
    }).then(
      (fn) => {
        // If we were unsubscribed before this listener resolved, detach now.
        if (active) {
          unlisten.push(fn);
        } else {
          fn();
        }
      },
      () => {
        // A failed `listen` registration is non-fatal: the authoritative
        // re-read still reconciles the terminal state.
      },
    );
  };

  register(EVENT_JOB_PROGRESS, (payload) => {
    if (!isJobProgressEvent(payload)) return;
    if (!matches(payload.jobId, payload.jobType, payload.targetStoryId)) return;
    if (!accept(payload.sequence)) return;
    subscription.onProgress(payload);
  });

  register(EVENT_JOB_COMPLETED, (payload) => {
    if (!isJobCompletedEvent(payload)) return;
    if (!matches(payload.jobId, payload.jobType, payload.targetStoryId)) return;
    if (!accept(payload.sequence)) return;
    subscription.onCompleted(payload);
  });

  register(EVENT_JOB_FAILED, (payload) => {
    if (!isJobFailedEvent(payload)) return;
    if (!matches(payload.jobId, payload.jobType, payload.targetStoryId)) return;
    if (!accept(payload.sequence)) return;
    subscription.onFailed(payload);
  });

  return () => {
    active = false;
    for (const fn of unlisten) fn();
    unlisten.length = 0;
  };
}
