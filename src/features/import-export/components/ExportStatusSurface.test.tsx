import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ExportStatus } from "../hooks/use-story-export";
import { ExportStatusSurface } from "./ExportStatusSurface";

function setup(status: ExportStatus) {
  const onRetry = vi.fn();
  const onDismiss = vi.fn();
  render(
    <ExportStatusSurface
      status={status}
      onRetry={onRetry}
      onDismiss={onDismiss}
    />,
  );
  return { onRetry, onDismiss };
}

describe("<ExportStatusSurface />", () => {
  it("renders no visible chip and no alert on idle (polite region is mounted but empty)", () => {
    render(
      <ExportStatusSurface
        status={{ kind: "idle" }}
        onRetry={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(
      screen.queryByText(/Exportation en cours/i),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // The persistent polite region IS mounted so that a later
    // `exported` transition is reliably announced.
    const politeRegions = document.querySelectorAll(
      "[aria-live='polite']",
    );
    expect(politeRegions.length).toBeGreaterThan(0);
    expect(politeRegions[0].textContent).toBe("");
  });

  it("renders a neutral chip with no aria-live region while exporting", () => {
    setup({ kind: "exporting" });
    expect(screen.getByText(/Exportation en cours/i)).toBeInTheDocument();
    expect(
      screen.queryByRole("status", { name: /Exporté/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("announces Exporté via the persistent aria-live=polite region on exported", () => {
    setup({
      kind: "exported",
      destinationPath: "/tmp/histoire.rustory",
      bytesWritten: 451,
      contentChecksum: "a".repeat(64),
    });
    const politeRegions = document.querySelectorAll(
      "[aria-live='polite']",
    );
    const regionWithAnnouncement = Array.from(politeRegions).find((el) =>
      el.textContent?.includes("Exporté"),
    );
    expect(regionWithAnnouncement).toBeDefined();
    expect(regionWithAnnouncement).toHaveAttribute("aria-atomic", "true");
  });

  it("renders the destination path in a readable line on success", () => {
    setup({
      kind: "exported",
      destinationPath: "/home/u/Documents/histoire.rustory",
      bytesWritten: 451,
      contentChecksum: "a".repeat(64),
    });
    expect(
      screen.getByText(/Exporté vers \/home\/u\/Documents\/histoire.rustory/),
    ).toBeInTheDocument();
  });

  it("renders a role=alert container with message and userAction on failure", () => {
    setup({
      kind: "failed",
      error: {
        code: "EXPORT_DESTINATION_UNAVAILABLE",
        message: "Écriture refusée par le système pour ce dossier.",
        userAction: "Choisis un dossier où tu as les droits en écriture.",
        details: null,
      },
    });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/Exportation échouée/);
    expect(alert).toHaveTextContent(
      /Écriture refusée par le système pour ce dossier/,
    );
    expect(alert).toHaveTextContent(
      /Choisis un dossier où tu as les droits en écriture/,
    );
  });

  it("omits the userAction paragraph when the error does not carry one", () => {
    setup({
      kind: "failed",
      error: {
        code: "EXPORT_DESTINATION_UNAVAILABLE",
        message: "Erreur sans action.",
        userAction: null,
        details: null,
      },
    });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/Erreur sans action/);
    // No trailing userAction paragraph rendered.
    expect(alert.querySelectorAll("p").length).toBe(2);
  });

  it("calls onRetry when the user clicks the retry button on failure", async () => {
    const user = userEvent.setup();
    const { onRetry } = setup({
      kind: "failed",
      error: {
        code: "EXPORT_DESTINATION_UNAVAILABLE",
        message: "err",
        userAction: "act",
        details: null,
      },
      });
    await user.click(
      screen.getByRole("button", { name: /Choisir un autre emplacement/i }),
    );
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("calls onDismiss when the user clicks Fermer on failure", async () => {
    const user = userEvent.setup();
    const { onDismiss } = setup({
      kind: "failed",
      error: {
        code: "EXPORT_DESTINATION_UNAVAILABLE",
        message: "err",
        userAction: "act",
        details: null,
      },
      });
    await user.click(screen.getByRole("button", { name: /Fermer/i }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("does not use a Toast for the success state (sober, non-theatrical)", () => {
    setup({
      kind: "exported",
      destinationPath: "/tmp/x.rustory",
      bytesWritten: 451,
      contentChecksum: "a".repeat(64),
    });
    // No role=status toast-like element with theatrical copy.
    expect(
      screen.queryByRole("status", { name: /Succès/i }),
    ).not.toBeInTheDocument();
  });
});
