import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { LuniiDecisionPanel } from "./LuniiDecisionPanel";

describe("<LuniiDecisionPanel />", () => {
  it("defaults to deviceState=absent and announces 'Aucun appareil connecté'", () => {
    render(<LuniiDecisionPanel />);
    expect(
      screen.getByRole("heading", { name: /panneau de décision/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/aucun appareil connecté/i),
    ).toBeInTheDocument();
  });

  it("renders the send CTA disabled with a keyboard-reachable reason (ui-states canonical wording)", () => {
    render(<LuniiDecisionPanel />);
    const cta = screen.getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    expect(cta).not.toBeDisabled();
    expect(cta).toHaveAttribute("aria-disabled", "true");

    const reasonId = cta.getAttribute("aria-describedby");
    expect(reasonId).toBeTruthy();
    const reason = document.getElementById(reasonId as string);
    // Canonical phrasing lifted verbatim from ui-states.md.
    expect(reason).toHaveTextContent(/envoi indisponible: appareil non supporté/i);
  });

  it("switches the badge wording when deviceState=idle", () => {
    render(<LuniiDecisionPanel deviceState="idle" />);
    expect(screen.getByText(/appareil prêt/i)).toBeInTheDocument();
  });
});
