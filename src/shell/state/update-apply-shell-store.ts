import { create } from "zustand";

import type {
  UpdateApplyPlan,
  UpdateApplyState,
} from "../../shared/ipc-contracts/settings";

/**
 * Minimal, UI-continuity-only relay of the update-apply gesture
 * (`Update Apply Contract`). This is NEVER a business truth (the plan is
 * re-decided Rust-side at every start and the session state lives
 * Rust-side, re-read authoritatively) — it only keeps the zone coherent
 * across re-renders and navigations: the last read plan/state, the
 * accepted job id, and the folded restart invite. `null` = not read yet
 * (or the read failed) — nothing renders that would need the missing
 * value (a `null` plan renders NO zone; a manual plan's guidance needs
 * no session state). No persistence per the architecture Zustand
 * contract (a fresh launch reads anew).
 */
export interface UpdateApplyShellState {
  /** The last read gesture plan — `null` while none exists. */
  plan: UpdateApplyPlan | null;
  /** The last known session state — `null` while none exists. */
  state: UpdateApplyState | null;
  /** The accepted job id of the session's gesture, for event
   *  correlation across remounts — `null` outside a tracked flight. */
  jobId: string | null;
  /** `Plus tard` folded the restart invite into its sober line (a
   *  session-UI choice, never re-proposed insistently). */
  restartInviteFolded: boolean;
  setPlan: (plan: UpdateApplyPlan) => void;
  setState: (state: UpdateApplyState) => void;
  setJobId: (jobId: string | null) => void;
  setRestartInviteFolded: (folded: boolean) => void;
}

export const useUpdateApplyShell = create<UpdateApplyShellState>((set) => ({
  plan: null,
  state: null,
  jobId: null,
  restartInviteFolded: false,
  setPlan: (plan) => set({ plan }),
  setState: (state) => set({ state }),
  setJobId: (jobId) => set({ jobId }),
  setRestartInviteFolded: (folded) => set({ restartInviteFolded: folded }),
}));
