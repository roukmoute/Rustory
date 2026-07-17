/**
 * Module-level ONE-SHOT read of the launch's update-availability verdict
 * (`Update Availability Contract`) — the `drop-bootstrap` discipline
 * (outside the React lifecycle), plus an explicit re-armable guard: the
 * read must fire ONCE per launch even if the bootstrap were ever called
 * twice (the Rust session memo makes a duplicate benign — this guard
 * keeps the frontend honest anyway).
 *
 * Called AFTER the root render in `main.tsx`: the invoke is asynchronous
 * and nothing awaits it — the < 3 s startup budget never waits for the
 * network. The settled verdict pours into the update shell store, whose
 * two calm consumers render it (or keep rendering NOTHING while no
 * verdict exists). A facade rejection (IPC failure, contract drift) is
 * TOTAL SILENCE: the app lives without a verdict — no surface, no
 * retry, no error state (the command itself is infallible; a rejection
 * here is a drift, not a transport story).
 */

import { readUpdateAvailability } from "../ipc/commands/settings";
import { useUpdateShell } from "../shell/state/update-shell-store";

let bootstrapped = false;

/** Fire the one-shot read (idempotent — later calls are no-ops). */
export function bootstrapUpdateAvailability(): void {
  if (bootstrapped) return;
  bootstrapped = true;
  void readUpdateAvailability().then(
    (availability) => {
      useUpdateShell.getState().setAvailability(availability);
    },
    () => {
      // Silence by contract: no verdict, no surface, no noise.
    },
  );
}

/** Re-arm the one-shot guard — TEST-ONLY (each test boots afresh). */
export function resetUpdateBootstrapForTests(): void {
  bootstrapped = false;
}
