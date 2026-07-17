import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { UpdateAvailability } from "../../../shared/ipc-contracts/settings";
import { useUpdateShell } from "../../../shell/state/update-shell-store";
import { UpdateStatusLine } from "./UpdateStatusLine";

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

describe("UpdateStatusLine", () => {
  beforeEach(() => {
    useUpdateShell.setState({ availability: null });
  });

  it("renders NOTHING while no verdict exists — never a spinner, never a waiting state", () => {
    const { container } = render(<UpdateStatusLine />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("renders the updateAvailable verdict with the info chip and the verbatim copies", () => {
    useUpdateShell.setState({ availability: updateAvailableVerdict() });
    const { container } = render(<UpdateStatusLine />);
    const line = screen.getByRole("status");
    expect(line).toHaveTextContent("Nouvelle version disponible : 9.9.9.");
    expect(line).toHaveTextContent(
      "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
    );
    // The info chip carries the glyph — color alone never distinguishes.
    expect(container.querySelector(".ds-chip--info")).not.toBeNull();
    // No gesture on the status line: no button, no retry, no link.
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("renders the three calm states without any chip", () => {
    for (const verdict of [
      upToDateVerdict(),
      checkUnavailableVerdict(),
      checkNotRunVerdict(),
    ]) {
      useUpdateShell.setState({ availability: verdict });
      const { container, unmount } = render(<UpdateStatusLine />);
      const line = screen.getByRole("status");
      expect(line).toHaveTextContent(verdict.headline);
      expect(line).toHaveTextContent(verdict.notice);
      expect(container.querySelector(".ds-chip")).toBeNull();
      unmount();
    }
  });

  it("never alarms: no role=alert, no error/warning chip on any state", () => {
    for (const verdict of [
      updateAvailableVerdict(),
      upToDateVerdict(),
      checkUnavailableVerdict(),
      checkNotRunVerdict(),
    ]) {
      useUpdateShell.setState({ availability: verdict });
      const { container, unmount } = render(<UpdateStatusLine />);
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(container.querySelector(".ds-chip--error")).toBeNull();
      expect(container.querySelector(".ds-chip--warning")).toBeNull();
      unmount();
    }
  });
});
