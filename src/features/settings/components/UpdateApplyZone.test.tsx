import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/settings", () => ({
  readUpdateApplyPlan: vi.fn(),
  readUpdateApplyState: vi.fn(),
  restartForUpdate: vi.fn(),
  startUpdateApply: vi.fn(),
}));
vi.mock("../../../ipc/events/update-apply-events", () => ({
  subscribeUpdateApplyEvents: vi.fn(),
}));

import {
  readUpdateApplyPlan,
  readUpdateApplyState,
  restartForUpdate,
  startUpdateApply,
} from "../../../ipc/commands/settings";
import { subscribeUpdateApplyEvents } from "../../../ipc/events/update-apply-events";
import type {
  UpdateApplyPlan,
  UpdateApplyState,
  UpdateAvailability,
} from "../../../shared/ipc-contracts/settings";
import { useUpdateApplyShell } from "../../../shell/state/update-apply-shell-store";
import { useUpdateShell } from "../../../shell/state/update-shell-store";
import { UpdateApplyZone } from "./UpdateApplyZone";

function updateAvailableVerdict(): UpdateAvailability {
  return {
    status: "updateAvailable",
    headline: "Nouvelle version disponible : 9.9.9.",
    notice:
      "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
    currentVersion: "0.1.0",
    latestVersion: "9.9.9",
  };
}

function integratedPlan(): UpdateApplyPlan {
  return {
    mode: "integrated",
    headline: "Cette copie peut installer les mises à jour de Rustory.",
    guidance:
      "Le téléchargement vérifie l'authenticité de la mise à jour avant de l'installer.",
  };
}

function manualPlan(): UpdateApplyPlan {
  return {
    mode: "manual",
    reason: "trust_chain_not_configured",
    headline:
      "La mise à jour intégrée n'est pas encore activée pour cette copie.",
    guidance:
      "Cette copie ne peut pas vérifier l'authenticité des mises à jour. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  };
}

const IDLE: UpdateApplyState = { status: "idle" };

const START_ARIA = "Télécharger et installer la mise à jour de Rustory";
const RESTART_ARIA = "Redémarrer Rustory pour terminer la mise à jour";

function runningState(percent?: number): UpdateApplyState {
  return {
    status: "running",
    jobId: "j1",
    phase: "downloading",
    ...(percent === undefined ? {} : { percent }),
    headline: "Téléchargement de la mise à jour en cours…",
    notice: "Tu peux continuer à utiliser Rustory pendant cette opération.",
  };
}

function readyState(): UpdateApplyState {
  return {
    status: "readyToRestart",
    headline: "La mise à jour de Rustory est prête.",
    notice:
      "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place.",
  };
}

function failedState(): UpdateApplyState {
  return {
    status: "failed",
    stage: "download",
    headline: "Le téléchargement de la mise à jour n'a pas abouti.",
    notice:
      "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie.",
  };
}

/** Seed the two stores so the zone renders without waiting on mounts. */
function seed(plan: UpdateApplyPlan | null, state: UpdateApplyState | null) {
  useUpdateShell.setState({ availability: updateAvailableVerdict() });
  useUpdateApplyShell.setState({
    plan,
    state,
    jobId: null,
    restartInviteFolded: false,
  });
}

describe("UpdateApplyZone", () => {
  beforeEach(() => {
    useUpdateShell.setState({ availability: null });
    useUpdateApplyShell.setState({
      plan: null,
      state: null,
      jobId: null,
      restartInviteFolded: false,
    });
    vi.mocked(readUpdateApplyPlan).mockReset();
    vi.mocked(readUpdateApplyState).mockReset();
    vi.mocked(startUpdateApply).mockReset();
    vi.mocked(restartForUpdate).mockReset();
    vi.mocked(subscribeUpdateApplyEvents).mockReset();
    // Default resolutions so the mount reads never dangle.
    vi.mocked(readUpdateApplyPlan).mockResolvedValue(integratedPlan());
    vi.mocked(readUpdateApplyState).mockResolvedValue(IDLE);
    vi.mocked(restartForUpdate).mockResolvedValue(undefined);
    vi.mocked(subscribeUpdateApplyEvents).mockReturnValue({
      ready: Promise.resolve(),
      unsubscribe: () => {},
    });
  });

  it("does not exist without a positive availability verdict", () => {
    useUpdateApplyShell.setState({
      plan: integratedPlan(),
      state: IDLE,
      jobId: null,
      restartInviteFolded: false,
    });
    const { container } = render(<UpdateApplyZone />);
    expect(container).toBeEmptyDOMElement();
    // No verdict → the zone does not even read.
    expect(readUpdateApplyPlan).not.toHaveBeenCalled();
    expect(readUpdateApplyState).not.toHaveBeenCalled();
  });

  it("reads the plan and the state on mount with a positive verdict — and NEVER starts by itself", async () => {
    useUpdateShell.setState({ availability: updateAvailableVerdict() });
    render(<UpdateApplyZone />);
    await waitFor(() => {
      expect(readUpdateApplyPlan).toHaveBeenCalled();
      expect(readUpdateApplyState).toHaveBeenCalled();
    });
    // Mounting is a READ: the download gesture is a user click only.
    expect(startUpdateApply).not.toHaveBeenCalled();
    // The reads resolved → the idle CTA is rendered.
    expect(
      await screen.findByRole("button", {
        name: "Télécharger et installer la mise à jour de Rustory",
      }),
    ).toBeInTheDocument();
  });

  it("renders a manual plan as one calm status block with NO button", () => {
    seed(manualPlan(), IDLE);
    render(<UpdateApplyZone />);
    const block = screen.getByRole("status");
    expect(block).toHaveTextContent(
      "La mise à jour intégrée n'est pas encore activée pour cette copie.",
    );
    expect(block).toHaveTextContent(
      "Cette copie ne peut pas vérifier l'authenticité des mises à jour. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("renders the manual guidance even when the state read rejects", async () => {
    // The guidance is the PLAN's face alone: a drift of the sole STATE
    // read must never silence a manual copy (the contract's status
    // block is unconditional once the zone exists).
    useUpdateShell.setState({ availability: updateAvailableVerdict() });
    vi.mocked(readUpdateApplyPlan).mockResolvedValue(manualPlan());
    vi.mocked(readUpdateApplyState).mockRejectedValue(new Error("drift"));
    render(<UpdateApplyZone />);
    const block = await screen.findByRole("status");
    expect(block).toHaveTextContent(
      "La mise à jour intégrée n'est pas encore activée pour cette copie.",
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("starts the gesture on the primary CTA and subscribes to the accepted job", async () => {
    seed(integratedPlan(), IDLE);
    vi.mocked(startUpdateApply).mockResolvedValue({
      outcome: "started",
      jobId: "j1",
    });
    // The mount's authoritative re-read confirms idle; the post-start
    // catch-up then lands the running state.
    vi.mocked(readUpdateApplyState)
      .mockResolvedValueOnce(IDLE)
      .mockResolvedValue(runningState(0));
    const user = userEvent.setup();
    render(<UpdateApplyZone />);

    await user.click(screen.getByRole("button", { name: START_ARIA }));
    await waitFor(() => {
      expect(startUpdateApply).toHaveBeenCalledTimes(1);
      expect(subscribeUpdateApplyEvents).toHaveBeenCalledWith(
        expect.objectContaining({ jobId: "j1" }),
      );
      // Catch-up re-read right after subscribing (start/events race).
      expect(readUpdateApplyState).toHaveBeenCalled();
    });
    expect(useUpdateApplyShell.getState().jobId).toBe("j1");
    // The running state landed from the authoritative re-read.
    expect(await screen.findByRole("progressbar")).toBeInTheDocument();
  });

  it("renders a determinate progress with the integer percent and the common notice", () => {
    seed(integratedPlan(), runningState(42));
    // The mount's authoritative re-read confirms the seeded state — the
    // assertions hold at equilibrium, never on a transient frame.
    vi.mocked(readUpdateApplyState).mockResolvedValue(runningState(42));
    render(<UpdateApplyZone />);
    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuenow", "42");
    expect(
      screen.getByText("Téléchargement de la mise à jour en cours…"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Tu peux continuer à utiliser Rustory pendant cette opération.",
      ),
    ).toBeInTheDocument();
    // The zone never tunnels the app: no dialog, no button lockdown.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("renders an indeterminate progress while no reliable percent exists", () => {
    seed(integratedPlan(), runningState());
    // The mount's authoritative re-read confirms the seeded state.
    vi.mocked(readUpdateApplyState).mockResolvedValue(runningState());
    render(<UpdateApplyZone />);
    const bar = screen.getByRole("progressbar");
    expect(bar).not.toHaveAttribute("aria-valuenow");
  });

  it("renders the restart invite and drives the guarded restart", async () => {
    seed(integratedPlan(), readyState());
    // The mount's authoritative re-read confirms the seeded state.
    vi.mocked(readUpdateApplyState).mockResolvedValue(readyState());
    const user = userEvent.setup();
    render(<UpdateApplyZone />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "La mise à jour de Rustory est prête.",
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place.",
    );
    await user.click(screen.getByRole("button", { name: RESTART_ARIA }));
    expect(restartForUpdate).toHaveBeenCalledTimes(1);
  });

  it("folds the invite on Plus tard — the state stays rendered, the restart stays reachable", async () => {
    seed(integratedPlan(), readyState());
    vi.mocked(readUpdateApplyState).mockResolvedValue(readyState());
    const user = userEvent.setup();
    render(<UpdateApplyZone />);
    await user.click(screen.getByRole("button", { name: "Plus tard" }));
    // The sober folded line: headline still rendered, restart reachable.
    expect(screen.getByRole("status")).toHaveTextContent(
      "La mise à jour de Rustory est prête.",
    );
    expect(
      screen.getByRole("button", { name: RESTART_ARIA }),
    ).toBeInTheDocument();
    // The full notice folded away with the invite.
    expect(
      screen.queryByText(
        "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place.",
      ),
    ).not.toBeInTheDocument();
    expect(useUpdateApplyShell.getState().restartInviteFolded).toBe(true);
  });

  it("renders a failed gesture calmly with the retry gesture", async () => {
    seed(integratedPlan(), failedState());
    vi.mocked(readUpdateApplyState).mockResolvedValue(failedState());
    vi.mocked(startUpdateApply).mockResolvedValue({
      outcome: "started",
      jobId: "j2",
    });
    const user = userEvent.setup();
    render(<UpdateApplyZone />);
    const block = screen.getByRole("status");
    expect(block).toHaveTextContent(
      "Le téléchargement de la mise à jour n'a pas abouti.",
    );
    expect(block).toHaveTextContent(
      "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie.",
    );
    await user.click(
      screen.getByRole("button", { name: "Réessayer la mise à jour" }),
    );
    expect(startUpdateApply).toHaveBeenCalledTimes(1);
  });

  it("never alarms nor links out, on any state", () => {
    for (const [plan, state] of [
      [manualPlan(), IDLE],
      [integratedPlan(), IDLE],
      [integratedPlan(), runningState(3)],
      [integratedPlan(), readyState()],
      [integratedPlan(), failedState()],
    ] as const) {
      seed(plan, state);
      // The mount's authoritative re-read confirms each seeded state.
      vi.mocked(readUpdateApplyState).mockResolvedValue(state);
      const { container, unmount } = render(<UpdateApplyZone />);
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(screen.queryByRole("link")).not.toBeInTheDocument();
      expect(container.querySelector(".ds-chip--error")).toBeNull();
      expect(container.querySelector(".ds-chip--warning")).toBeNull();
      expect(container.querySelector("a")).toBeNull();
      unmount();
    }
  });
});
