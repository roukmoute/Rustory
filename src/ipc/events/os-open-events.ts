/**
 * Typed subscription helper for the `os-open:requested` signal — the ONLY
 * place that calls `@tauri-apps/api/event::listen` for it (the exact
 * discipline of `subscribeJobEvents`, the channel's first occupant).
 *
 * The event is a PURE SIGNAL: an OS-open intent arrived while the app is
 * already running. Its payload is an empty versionable object (`{}`) — the
 * handler pulls the verdict through `analyzeOsOpenRequest`, never from the
 * event. Returns a synchronous unsubscribe that also cancels a listener
 * still being registered.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

const EVENT_OS_OPEN_REQUESTED = "os-open:requested";

/** True iff `payload` is the empty versionable signal object. Future
 *  fields are tolerated by design (versionable) — only a non-object is a
 *  malformed signal. */
function isOsOpenRequestedPayload(payload: unknown): boolean {
  return typeof payload === "object" && payload !== null;
}

/**
 * Subscribe to the `os-open:requested` signal. Returns an `unsubscribe`
 * that detaches the listener (and cancels one still registering).
 *
 * `onSettled` (optional) fires ONCE when the async registration settles —
 * success or failure alike. It closes the lost-wake-up window: an intent
 * whose event was emitted BEFORE the listener became effective produced no
 * consumer, so the caller runs one authoritative catch-up pull on
 * settlement (the pending Rust intent is served either way; a `none`
 * answer stays a total no-op).
 */
export function subscribeOsOpenRequested(
  handler: () => void,
  onSettled?: () => void,
): () => void {
  let active = true;
  const unlisten: UnlistenFn[] = [];

  void listen<unknown>(EVENT_OS_OPEN_REQUESTED, (event) => {
    if (!active) return;
    if (!isOsOpenRequestedPayload(event.payload)) return;
    handler();
  }).then(
    (fn) => {
      // If we were unsubscribed before this listener resolved, detach now.
      if (active) {
        unlisten.push(fn);
      } else {
        fn();
      }
      onSettled?.();
    },
    () => {
      // A failed `listen` registration is non-fatal: the catch-up pull
      // below and the library-mount pull still serve any pending intent.
      onSettled?.();
    },
  );

  return () => {
    active = false;
    for (const fn of unlisten) fn();
    unlisten.length = 0;
  };
}
