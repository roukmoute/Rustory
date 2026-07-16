import { create } from "zustand";

/**
 * Minimal, UI-continuity-only relay of the `os-open:requested` signal. This
 * is NEVER the intent truth (the intent lives Rust-side and its verdict is
 * pulled through `analyzeOsOpenRequest`) — it only carries the boolean
 * "a signal arrived" from the module-level bootstrap to the library route,
 * which clears it and pulls the verdict. No payload, no path, no domain
 * state; no persistence (a fresh launch never restores a stale signal),
 * per the architecture Zustand contract.
 */
export interface OsOpenShellState {
  /** True while a signal is waiting for the library route to consume it. */
  pendingSignal: boolean;
  /** Raise the signal (bootstrap side — outside the React lifecycle). */
  raise: () => void;
  /** Consume the signal (library side, before pulling the verdict). */
  clear: () => void;
}

export const useOsOpenShell = create<OsOpenShellState>((set) => ({
  pendingSignal: false,
  raise: () => set({ pendingSignal: true }),
  clear: () => set({ pendingSignal: false }),
}));
