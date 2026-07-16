/**
 * Module-level wiring of the `os-open:requested` signal — OUTSIDE the React
 * lifecycle (StrictMode-safe by position: `main.tsx` calls it exactly once,
 * before the root render). On each signal it raises the shell relay and
 * brings the app back to `/library`, the product's stable return base: the
 * app may sit on `/settings` or in the editor — the navigation lets the
 * editor unmount through its NORMAL lifecycle (autosave flush at unmount,
 * no bypass). `replace` keeps the history sane: a system-driven redirection
 * must never trap the back button behind a stacked entry.
 *
 * The signal carries NO data — the library route consumes the relay and
 * pulls the verdict through `analyzeOsOpenRequest` (see the OS Open
 * Contract in `docs/architecture/ui-states.md`).
 */

import { subscribeOsOpenRequested } from "../ipc/events/os-open-events";
import { useOsOpenShell } from "../shell/state/os-open-shell-store";
import { router } from "./router";

/** The single router capability the bootstrap needs — injectable in tests. */
export interface OsOpenNavigator {
  navigate: (
    to: string,
    options: { replace: boolean },
  ) => void | Promise<void>;
}

/**
 * Subscribe the OS-open signal to the shell relay + the library navigation.
 * Returns the unsubscribe (unused in production — the wiring lives for the
 * whole app life — but it keeps tests leak-free).
 *
 * Registration handshake (closes the lost wake-up): the `listen()`
 * registration is asynchronous, so an intent can land AFTER the
 * library-mount pull but BEFORE the listener is effective — its event then
 * has no consumer. Once the registration SETTLES (success or failure), one
 * catch-up signal is raised: the library pulls authoritatively and serves
 * any intent that slipped through the window (a `none` answer stays a
 * total silent no-op). No navigation on the catch-up — the app boots on
 * `/library`, and an intent pending on another screen is served by that
 * screen's next library visit, exactly like the cold-start seed.
 */
export function bootstrapOsOpenSignal(
  appRouter: OsOpenNavigator = router,
): () => void {
  return subscribeOsOpenRequested(
    () => {
      useOsOpenShell.getState().raise();
      void appRouter.navigate("/library", { replace: true });
    },
    () => {
      useOsOpenShell.getState().raise();
    },
  );
}
