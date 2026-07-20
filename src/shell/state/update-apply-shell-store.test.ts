import { beforeEach, describe, expect, it } from "vitest";

import type {
  UpdateApplyPlan,
  UpdateApplyState,
} from "../../shared/ipc-contracts/settings";
import { useUpdateApplyShell } from "./update-apply-shell-store";

const PLAN: UpdateApplyPlan = {
  mode: "integrated",
  headline: "Cette copie peut installer les mises à jour de Rustory.",
  guidance:
    "Le téléchargement vérifie l'authenticité de la mise à jour avant de l'installer.",
};

const RUNNING: UpdateApplyState = {
  status: "running",
  // The wire guard REQUIRES a non-empty jobId on `running` — the
  // fixture stays a state the wire can actually produce.
  jobId: "j1",
  phase: "downloading",
  percent: 12,
  headline: "Téléchargement de la mise à jour en cours…",
  notice: "Tu peux continuer à utiliser Rustory pendant cette opération.",
};

describe("useUpdateApplyShell", () => {
  beforeEach(() => {
    useUpdateApplyShell.setState({
      plan: null,
      state: null,
      jobId: null,
      restartInviteFolded: false,
    });
  });

  it("starts empty — the zone renders nothing", () => {
    const state = useUpdateApplyShell.getState();
    expect(state.plan).toBeNull();
    expect(state.state).toBeNull();
    expect(state.jobId).toBeNull();
    expect(state.restartInviteFolded).toBe(false);
  });

  it("carries the read plan, the session state and the tracked job", () => {
    useUpdateApplyShell.getState().setPlan(PLAN);
    useUpdateApplyShell.getState().setState(RUNNING);
    useUpdateApplyShell.getState().setJobId("j1");
    const state = useUpdateApplyShell.getState();
    expect(state.plan).toEqual(PLAN);
    expect(state.state).toEqual(RUNNING);
    expect(state.jobId).toBe("j1");
  });

  it("folds and unfolds the restart invite as a session-UI choice", () => {
    useUpdateApplyShell.getState().setRestartInviteFolded(true);
    expect(useUpdateApplyShell.getState().restartInviteFolded).toBe(true);
    useUpdateApplyShell.getState().setRestartInviteFolded(false);
    expect(useUpdateApplyShell.getState().restartInviteFolded).toBe(false);
  });

  it("clears the tracked job on terminal", () => {
    useUpdateApplyShell.getState().setJobId("j1");
    useUpdateApplyShell.getState().setJobId(null);
    expect(useUpdateApplyShell.getState().jobId).toBeNull();
  });
});
