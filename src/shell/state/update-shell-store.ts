import { create } from "zustand";

import type { UpdateAvailability } from "../../shared/ipc-contracts/settings";

/**
 * Minimal, UI-continuity-only relay of the launch's update-availability
 * verdict (`Update Availability Contract`). This is NEVER a business
 * truth (the verdict lives Rust-side, memoized for the session, and
 * arrives through the one-shot bootstrap) — it only carries the read
 * verdict from the module-level bootstrap to its two calm consumers:
 * the settings status line and the library's discreet signal. `null` =
 * no verdict exists (check not settled, or the read failed) — the
 * surfaces render NOTHING. No persistence (a fresh launch checks anew),
 * per the architecture Zustand contract.
 */
export interface UpdateShellState {
  /** The launch's verdict — `null` while none exists. */
  availability: UpdateAvailability | null;
  /** Pour the read verdict in (bootstrap side, once per launch). */
  setAvailability: (availability: UpdateAvailability) => void;
}

export const useUpdateShell = create<UpdateShellState>((set) => ({
  availability: null,
  setAvailability: (availability) => set({ availability }),
}));
