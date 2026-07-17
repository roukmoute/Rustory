/**
 * Module-level wiring of the three drop signals — OUTSIDE the React
 * lifecycle (StrictMode-safe by position: `main.tsx` calls it exactly
 * once, before the root render), the exact discipline of
 * `bootstrapOsOpenSignal`.
 *
 * `drop:hover` / `drop:hover-ended` drive the decorative overlay flag.
 * `drop:requested` closes the overlay, raises the shell relay and brings
 * the app back to `/library`, the product's stable return base: the app
 * may sit on `/settings` or in the editor — the navigation lets the editor
 * unmount through its NORMAL lifecycle (autosave flush at unmount, no
 * bypass). `replace` keeps the history sane: a system-driven redirection
 * must never trap the back button behind a stacked entry.
 *
 * The signals carry NO data — the library route consumes the relay and
 * pulls the verdict through `analyzeDropRequest` (see the Drop Intent
 * Contract in `docs/architecture/ui-states.md`).
 */

import {
  subscribeDropHover,
  subscribeDropHoverEnded,
  subscribeDropRequested,
} from "../ipc/events/drop-events";
import { useDropShell } from "../shell/state/drop-shell-store";
import { router } from "./router";

/** The single router capability the bootstrap needs — injectable in tests. */
export interface DropNavigator {
  navigate: (
    to: string,
    options: { replace: boolean },
  ) => void | Promise<void>;
}

/**
 * Subscribe the drop signals to the shell relay + the library navigation.
 * Returns a combined unsubscribe (unused in production — the wiring lives
 * for the whole app life — but it keeps tests leak-free).
 *
 * Registration handshake (closes the lost wake-up): the `listen()`
 * registration is asynchronous, so an intent can land AFTER the
 * library-mount pull but BEFORE the listener is effective — its event then
 * has no consumer. Once the `requested` registration SETTLES (success or
 * failure), one catch-up signal is raised: the library pulls
 * authoritatively and serves any intent that slipped through the window (a
 * `none` answer stays a total silent no-op). No navigation on the catch-up
 * — the app boots on `/library`, and an intent pending on another screen
 * is served by that screen's next library visit.
 */
export function bootstrapDropSignals(
  appRouter: DropNavigator = router,
): () => void {
  const unsubscribeHover = subscribeDropHover(() => {
    useDropShell.getState().raiseHover();
  });
  const unsubscribeHoverEnded = subscribeDropHoverEnded(() => {
    useDropShell.getState().clearHover();
  });
  const unsubscribeRequested = subscribeDropRequested(
    () => {
      // Belt and braces: `Leave` is not guaranteed after a `Drop` on every
      // platform — the overlay also closes on the intent signal.
      useDropShell.getState().clearHover();
      useDropShell.getState().raiseSignal();
      void appRouter.navigate("/library", { replace: true });
    },
    () => {
      useDropShell.getState().raiseSignal();
    },
  );

  return () => {
    unsubscribeHover();
    unsubscribeHoverEnded();
    unsubscribeRequested();
  };
}
