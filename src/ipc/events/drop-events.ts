/**
 * Typed subscription helpers for the three drop signals — the ONLY place
 * that calls `@tauri-apps/api/event::listen` for them (the exact
 * discipline of `subscribeOsOpenRequested`, the channel's sibling).
 *
 * The events are PURE SIGNALS: a drag hovers the window (`drop:hover`),
 * the drag left or a drop landed (`drop:hover-ended`), a drop produced a
 * pending intent (`drop:requested`). Their payloads are empty versionable
 * objects (`{}`) — no path, no count, no kind ever crosses; the handler of
 * `requested` pulls the verdict through `analyzeDropRequest`, never from
 * the event. The framework's BUILTIN drag events (which carry absolute
 * paths toward the webview) are never consumed anywhere in the repo — the
 * non-consumption discipline of the Drop Intent Contract; these three
 * custom signals are the only ones listened to. Each helper returns a
 * synchronous unsubscribe that also cancels a listener still being
 * registered.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

const EVENT_DROP_HOVER = "drop:hover";
const EVENT_DROP_HOVER_ENDED = "drop:hover-ended";
const EVENT_DROP_REQUESTED = "drop:requested";

/** True iff `payload` is the empty versionable signal object. Future
 *  fields are tolerated by design (versionable) — only a non-object is a
 *  malformed signal. */
function isDropSignalPayload(payload: unknown): boolean {
  return typeof payload === "object" && payload !== null;
}

/** Shared subscription plumbing of the three signals: async registration,
 *  synchronous unsubscribe (cancelling a still-registering listener), and
 *  the optional settlement handshake. */
function subscribeDropSignal(
  eventName: string,
  handler: () => void,
  onSettled?: () => void,
): () => void {
  let active = true;
  const unlisten: UnlistenFn[] = [];

  void listen<unknown>(eventName, (event) => {
    if (!active) return;
    if (!isDropSignalPayload(event.payload)) return;
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
      // (requested) and the library-mount pull still serve any pending
      // intent; a lost hover only costs the decorative overlay.
      onSettled?.();
    },
  );

  return () => {
    active = false;
    for (const fn of unlisten) fn();
    unlisten.length = 0;
  };
}

/**
 * Subscribe to the `drop:hover` signal (a drag carrying paths entered the
 * window). Returns an `unsubscribe` that detaches the listener (and
 * cancels one still registering).
 */
export function subscribeDropHover(handler: () => void): () => void {
  return subscribeDropSignal(EVENT_DROP_HOVER, handler);
}

/**
 * Subscribe to the `drop:hover-ended` signal (the drag left the window OR
 * a drop landed — emitted on both because `Leave` is not guaranteed after
 * a `Drop` on every platform; consumers are idempotent).
 */
export function subscribeDropHoverEnded(handler: () => void): () => void {
  return subscribeDropSignal(EVENT_DROP_HOVER_ENDED, handler);
}

/**
 * Subscribe to the `drop:requested` signal (a drop produced a pending
 * intent). `onSettled` (optional) fires ONCE when the async registration
 * settles — success or failure alike. It closes the lost-wake-up window:
 * an intent whose event was emitted BEFORE the listener became effective
 * produced no consumer, so the caller runs one authoritative catch-up pull
 * on settlement (the pending Rust intent is served either way; a `none`
 * answer stays a total no-op).
 */
export function subscribeDropRequested(
  handler: () => void,
  onSettled?: () => void,
): () => void {
  return subscribeDropSignal(EVENT_DROP_REQUESTED, handler, onSettled);
}
