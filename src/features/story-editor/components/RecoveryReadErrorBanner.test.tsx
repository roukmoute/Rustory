import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AppError } from "../../../shared/errors/app-error";

import { RecoveryReadErrorBanner } from "./RecoveryReadErrorBanner";

const ERROR: AppError = {
  code: "RECOVERY_DRAFT_UNAVAILABLE",
  message: "Récupération indisponible: vérifie le disque local et réessaie.",
  userAction: "Vérifie l'espace disque et les permissions.",
  details: { source: "sqlite_select" },
};

describe("RecoveryReadErrorBanner", () => {
  it("renders the section with role=region and aria-label='Récupération indisponible'", () => {
    render(
      <RecoveryReadErrorBanner
        error={ERROR}
        onRetry={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("region", { name: "Récupération indisponible" }),
    ).toBeInTheDocument();
  });

  it("renders the error message and userAction inside a role=alert region", () => {
    render(
      <RecoveryReadErrorBanner
        error={ERROR}
        onRetry={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      "Récupération indisponible: vérifie le disque local et réessaie.",
    );
    expect(alert).toHaveTextContent(
      "Vérifie l'espace disque et les permissions.",
    );
  });

  it("calls onRetry when Réessayer la récupération is clicked", async () => {
    const onRetry = vi.fn();
    render(
      <RecoveryReadErrorBanner
        error={ERROR}
        onRetry={onRetry}
        onDismiss={vi.fn()}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Réessayer la récupération" }),
    );
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("calls onDismiss when Conserver l'état enregistré is clicked", async () => {
    const onDismiss = vi.fn();
    render(
      <RecoveryReadErrorBanner
        error={ERROR}
        onRetry={vi.fn()}
        onDismiss={onDismiss}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Conserver l'état enregistré" }),
    );
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("renders Réessayer before Conserver in tab order so a keyboard user retries first", () => {
    render(
      <RecoveryReadErrorBanner
        error={ERROR}
        onRetry={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    const buttons = screen.getAllByRole("button");
    const retry = screen.getByRole("button", {
      name: "Réessayer la récupération",
    });
    const dismiss = screen.getByRole("button", {
      name: "Conserver l'état enregistré",
    });
    expect(buttons.indexOf(retry)).toBeLessThan(buttons.indexOf(dismiss));
  });

  it("does NOT render the diff (no draftTitle / persistedTitle on this surface)", () => {
    render(
      <RecoveryReadErrorBanner
        error={ERROR}
        onRetry={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    // The diff is a `<dl>` with the "Tu avais tapé" / "Dernier état
    // enregistré" rows in the diff banner. None of those exist here.
    expect(screen.queryByText("Tu avais tapé :")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Dernier état enregistré :"),
    ).not.toBeInTheDocument();
  });
});
