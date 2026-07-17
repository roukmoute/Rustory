import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it } from "vitest";

import type { UpdateAvailability } from "../../../shared/ipc-contracts/settings";
import { useUpdateShell } from "../../../shell/state/update-shell-store";
import { UpdateAvailabilitySignal } from "./UpdateAvailabilitySignal";

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

function upToDateVerdict(): UpdateAvailability {
  return {
    status: "upToDate",
    headline: "Aucune version plus récente n'est publiée.",
    notice: "Aucune action n'est nécessaire.",
    currentVersion: "0.1.0",
  };
}

function checkUnavailableVerdict(): UpdateAvailability {
  return {
    status: "checkUnavailable",
    headline: "La vérification de version n'a pas pu être faite.",
    notice:
      "Rustory reste pleinement utilisable. La vérification réessaiera au prochain lancement.",
    currentVersion: "0.1.0",
  };
}

function checkNotRunVerdict(): UpdateAvailability {
  return {
    status: "checkNotRun",
    headline: "La vérification de version n'est pas exécutée pour cette copie.",
    notice:
      "Cette copie de Rustory ne provient pas d'un canal de distribution officiel : aucune vérification réseau n'est effectuée.",
    currentVersion: "0.1.0",
  };
}

/** Mount the signal inside a memory router so its in-app navigation is
 *  observable (the `/settings` route renders a probe). */
function renderSignal() {
  const router = createMemoryRouter(
    [
      { path: "/library", element: <UpdateAvailabilitySignal /> },
      { path: "/settings", element: <p>Écran des réglages</p> },
    ],
    { initialEntries: ["/library"] },
  );
  return { ...render(<RouterProvider router={router} />), router };
}

describe("UpdateAvailabilitySignal", () => {
  beforeEach(() => {
    useUpdateShell.setState({ availability: null });
  });

  it("renders NOTHING while no verdict exists — silence during the background check", () => {
    const { container } = renderSignal();
    expect(container).toBeEmptyDOMElement();
  });

  it("renders NOTHING on every non-positive state — silence is the rule", () => {
    for (const verdict of [
      upToDateVerdict(),
      checkUnavailableVerdict(),
      checkNotRunVerdict(),
    ]) {
      useUpdateShell.setState({ availability: verdict });
      const { container, unmount } = renderSignal();
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
  });

  it("renders the compact positive block: info chip, verbatim headline, one gesture", () => {
    useUpdateShell.setState({ availability: updateAvailableVerdict() });
    const { container } = renderSignal();
    const signal = screen.getByRole("status");
    expect(signal).toHaveTextContent("Nouvelle version disponible : 9.9.9.");
    // The class carries the layout contract: the stylesheet's
    // `margin-top: auto` anchors the block at the flex column's REAL
    // foot (free space above, never below) — a renamed/dropped class
    // would silently lose the anchoring.
    expect(signal).toHaveClass("update-availability-signal");
    expect(container.querySelector(".ds-chip--info")).not.toBeNull();
    const details = screen.getByRole("button", {
      name: "Consulter les détails de la mise à jour",
    });
    expect(details).toHaveTextContent("Voir les détails");
    // Never an alarm, never a blocking element, never an outbound link.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    expect(container.querySelector(".ds-chip--error")).toBeNull();
    expect(container.querySelector(".ds-chip--warning")).toBeNull();
  });

  it("navigates IN-APP to /settings on « Voir les détails »", async () => {
    const user = userEvent.setup();
    useUpdateShell.setState({ availability: updateAvailableVerdict() });
    renderSignal();

    await user.click(
      screen.getByRole("button", {
        name: "Consulter les détails de la mise à jour",
      }),
    );
    expect(await screen.findByText("Écran des réglages")).toBeInTheDocument();
  });

  it("is keyboard-reachable: the gesture activates from the keyboard", async () => {
    const user = userEvent.setup();
    useUpdateShell.setState({ availability: updateAvailableVerdict() });
    renderSignal();

    await user.tab();
    expect(
      screen.getByRole("button", {
        name: "Consulter les détails de la mise à jour",
      }),
    ).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(await screen.findByText("Écran des réglages")).toBeInTheDocument();
  });
});
