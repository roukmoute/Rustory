/**
 * Typed subscription helper for the update-apply event channel
 * (`Update Apply Contract`) — the ONLY place that calls
 * `@tauri-apps/api/event::listen` for `update:*` events. The exact
 * `job-events` pattern, on the gesture's DEDICATED family (the `job:*`
 * payloads carry a non-optional `targetStoryId` and stay untouched).
 *
 * Correlates by `jobId`, validates every payload against the wire
 * guards, and enforces idempotence + ordering tolerance via the
 * MONOTONIC `sequence`: an event whose sequence is not strictly greater
 * than the last applied one is dropped (a duplicate or a late delivery
 * never regresses the state). Returns a synchronous unsubscribe that
 * also cancels listeners still being registered.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import {
  isUpdateApplyCompletedEvent,
  isUpdateApplyFailedEvent,
  isUpdateApplyProgressEvent,
  type UpdateApplyCompletedEvent,
  type UpdateApplyFailedEvent,
  type UpdateApplyProgressEvent,
} from "../../shared/ipc-contracts/settings";

const EVENT_UPDATE_PROGRESS = "update:progress";
const EVENT_UPDATE_COMPLETED = "update:completed";
const EVENT_UPDATE_FAILED = "update:failed";

export interface UpdateApplySubscription {
  jobId: string;
  onProgress: (event: UpdateApplyProgressEvent) => void;
  onCompleted: (event: UpdateApplyCompletedEvent) => void;
  onFailed: (event: UpdateApplyFailedEvent) => void;
}

export interface UpdateApplySubscriptionHandle {
  /** Settles once the three `listen()` registrations settled — REGISTERED
   *  or failed (never rejects). This is the safe point for the
   *  authoritative catch-up re-read: reading before it could observe a
   *  pre-terminal state whose terminal event then fires into a not-yet-
   *  registered listener and is lost forever. A failed registration
   *  still resolves — the caller's re-read IS the fallback (the
   *  authoritative state remains the truth). */
  ready: Promise<void>;
  unsubscribe: () => void;
}

/**
 * Subscribe to the `update:*` events for one gesture. Returns a handle
 * whose `unsubscribe` detaches the three listeners (and cancels any that
 * are still registering) and whose `ready` signals when the
 * registrations settled — re-read the authoritative state AFTER it.
 */
export function subscribeUpdateApplyEvents(
  subscription: UpdateApplySubscription,
): UpdateApplySubscriptionHandle {
  let active = true;
  let lastSequence = -1;
  const unlisten: UnlistenFn[] = [];

  // Idempotence + ordering tolerance: only a strictly newer sequence advances.
  const accept = (sequence: number): boolean => {
    if (sequence <= lastSequence) return false;
    lastSequence = sequence;
    return true;
  };

  const register = (
    name: string,
    handle: (payload: unknown) => void,
  ): Promise<void> =>
    listen<unknown>(name, (event) => {
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

  const registrations = [
    register(EVENT_UPDATE_PROGRESS, (payload) => {
      if (!isUpdateApplyProgressEvent(payload)) return;
      if (payload.jobId !== subscription.jobId) return;
      if (!accept(payload.sequence)) return;
      subscription.onProgress(payload);
    }),
    register(EVENT_UPDATE_COMPLETED, (payload) => {
      if (!isUpdateApplyCompletedEvent(payload)) return;
      if (payload.jobId !== subscription.jobId) return;
      if (!accept(payload.sequence)) return;
      subscription.onCompleted(payload);
    }),
    register(EVENT_UPDATE_FAILED, (payload) => {
      if (!isUpdateApplyFailedEvent(payload)) return;
      if (payload.jobId !== subscription.jobId) return;
      if (!accept(payload.sequence)) return;
      subscription.onFailed(payload);
    }),
  ];

  return {
    ready: Promise.all(registrations).then(() => undefined),
    unsubscribe: () => {
      active = false;
      for (const fn of unlisten) fn();
      unlisten.length = 0;
    },
  };
}
