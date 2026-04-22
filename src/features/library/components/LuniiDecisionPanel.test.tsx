import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { LuniiDecisionPanel } from "./LuniiDecisionPanel";

describe("<LuniiDecisionPanel />", () => {
  const noop = () => {};

  it("defaults to deviceState=absent and announces 'Aucun appareil connecté'", () => {
    render(<LuniiDecisionPanel onEdit={noop} />);
    expect(
      screen.getByRole("heading", { name: /panneau de décision/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/aucun appareil connecté/i)).toBeInTheDocument();
  });

  it("renders the send CTA disabled with a keyboard-reachable reason (ui-states canonical wording)", () => {
    render(<LuniiDecisionPanel onEdit={noop} />);
    const cta = screen.getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    expect(cta).not.toBeDisabled();
    expect(cta).toHaveAttribute("aria-disabled", "true");

    const reasonId = cta.getAttribute("aria-describedby");
    expect(reasonId).toBeTruthy();
    const reason = document.getElementById(reasonId as string);
    expect(reason).toHaveTextContent(
      /envoi indisponible: appareil non supporté/i,
    );
  });

  it("switches the badge wording when deviceState=idle", () => {
    render(<LuniiDecisionPanel deviceState="idle" onEdit={noop} />);
    expect(screen.getByText(/appareil prêt/i)).toBeInTheDocument();
  });

  // --- Selection wiring ---

  it("with selectedCount=0, shows 'Aucune histoire sélectionnée' and disables Éditer with the canonical reason", () => {
    render(<LuniiDecisionPanel selectedCount={0} onEdit={noop} />);

    const region = screen.getByRole("region", {
      name: /sélection courante/i,
    });
    expect(region).toHaveTextContent(/aucune histoire sélectionnée/i);

    const edit = screen.getByRole("button", { name: /^éditer$/i });
    expect(edit).toHaveAttribute("aria-disabled", "true");

    const reasonId = edit.getAttribute("aria-describedby");
    expect(reasonId).toBeTruthy();
    const reason = document.getElementById(reasonId as string);
    expect(reason).toHaveTextContent(
      /reprise indisponible: aucune histoire sélectionnée/i,
    );
  });

  it("with selectedCount=1, enables Éditer and invokes onEdit on click", async () => {
    const user = userEvent.setup();
    const onEdit = vi.fn();
    render(<LuniiDecisionPanel selectedCount={1} onEdit={onEdit} />);

    const region = screen.getByRole("region", {
      name: /sélection courante/i,
    });
    expect(region).toHaveTextContent(/1 histoire sélectionnée/i);

    const edit = screen.getByRole("button", { name: /^éditer$/i });
    expect(edit).not.toHaveAttribute("aria-disabled");

    await user.click(edit);
    expect(onEdit).toHaveBeenCalledTimes(1);
  });

  it("with selectedCount=3, disables Éditer with the 'sélection multiple' reason", () => {
    render(<LuniiDecisionPanel selectedCount={3} onEdit={noop} />);

    const region = screen.getByRole("region", {
      name: /sélection courante/i,
    });
    expect(region).toHaveTextContent(/3 histoires sélectionnées/i);

    const edit = screen.getByRole("button", { name: /^éditer$/i });
    expect(edit).toHaveAttribute("aria-disabled", "true");

    const reasonId = edit.getAttribute("aria-describedby");
    const reason = document.getElementById(reasonId as string);
    expect(reason).toHaveTextContent(
      /reprise indisponible: sélection multiple/i,
    );
  });

  it("Éditer button is keyboard-reachable and Enter triggers onEdit when active", async () => {
    const user = userEvent.setup();
    const onEdit = vi.fn();
    render(<LuniiDecisionPanel selectedCount={1} onEdit={onEdit} />);

    const edit = screen.getByRole("button", { name: /^éditer$/i });
    edit.focus();
    await user.keyboard("{Enter}");
    expect(onEdit).toHaveBeenCalledTimes(1);
  });
});
