import { create } from "zustand";

/**
 * Minimal, UI-continuity-only relay of the drop signals. This is NEVER the
 * intent truth (the intent lives Rust-side and its verdict is pulled
 * through `analyzeDropRequest`) — it only carries two booleans from the
 * module-level bootstrap to their consumers: `hoverActive` (a drag hovers
 * the window → the decorative overlay renders) and `pendingSignal` (a drop
 * produced an intent → the library route consumes it and pulls the
 * verdict). No payload, no path, no domain state; no persistence (a fresh
 * launch never restores a stale signal), per the architecture Zustand
 * contract.
 */
export interface DropShellState {
  /** True while a drag carrying paths hovers the window. */
  hoverActive: boolean;
  /** True while a drop signal is waiting for the library route to consume it. */
  pendingSignal: boolean;
  /** Raise the hover flag (bootstrap side — outside the React lifecycle). */
  raiseHover: () => void;
  /** Clear the hover flag (drag left, drop landed, or intent signaled). */
  clearHover: () => void;
  /** Raise the drop signal (bootstrap side). */
  raiseSignal: () => void;
  /** Consume the drop signal (library side, before pulling the verdict). */
  clearSignal: () => void;
}

export const useDropShell = create<DropShellState>((set) => ({
  hoverActive: false,
  pendingSignal: false,
  raiseHover: () => set({ hoverActive: true }),
  clearHover: () => set({ hoverActive: false }),
  raiseSignal: () => set({ pendingSignal: true }),
  clearSignal: () => set({ pendingSignal: false }),
}));
