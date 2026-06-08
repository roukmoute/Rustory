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
    expect(
      screen.getAllByText(/aucun appareil connecté/i).length,
    ).toBeGreaterThan(0);
  });

  it("renders the send CTA disabled with a keyboard-reachable canonical absent-device reason", () => {
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
      /envoi indisponible: aucun appareil connecté/i,
    );
  });

  it("renders the support-profile fallback link on unsupported / ambiguous / error states", () => {
    const onConsultSupportProfile = vi.fn();
    for (const state of ["unsupported", "ambiguous", "error"] as const) {
      const { unmount } = render(
        <LuniiDecisionPanel
          deviceState={state}
          onEdit={noop}
          onConsultSupportProfile={onConsultSupportProfile}
        />,
      );
      expect(
        screen.getByRole("button", { name: /consulter le profil de support/i }),
      ).toBeInTheDocument();
      unmount();
    }
  });

  it("does not render the support-profile link on absent / idle / scanning states", () => {
    const onConsultSupportProfile = vi.fn();
    for (const state of ["absent", "idle", "scanning"] as const) {
      const { unmount } = render(
        <LuniiDecisionPanel
          deviceState={state}
          onEdit={noop}
          onConsultSupportProfile={onConsultSupportProfile}
        />,
      );
      expect(
        screen.queryByRole("button", {
          name: /consulter le profil de support/i,
        }),
      ).toBeNull();
      unmount();
    }
  });

  it("switches the badge wording when deviceState=idle", () => {
    render(<LuniiDecisionPanel deviceState="idle" onEdit={noop} />);
    expect(screen.getByText(/^appareil prêt$/i)).toBeInTheDocument();
  });

  it("appends the deviceLabel suffix when supplied with idle", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        deviceLabel="Lunii Origine 2.x"
        onEdit={noop}
      />,
    );
    expect(
      screen.getByText(/appareil prêt — lunii origine 2\.x/i),
    ).toBeInTheDocument();
  });

  it("renders 'Profil non supporté' when deviceState=unsupported", () => {
    render(
      <LuniiDecisionPanel
        deviceState="unsupported"
        deviceReason="Profil non supporté: format métadonnées v99 non géré"
        onEdit={noop}
      />,
    );
    // The chip and the reason both contain the phrase, so the assertion
    // accepts multiple matches and only requires at least one.
    expect(screen.getAllByText(/profil non supporté/i).length).toBeGreaterThan(0);
    expect(
      screen.getByText(/format métadonnées v99 non géré/i),
    ).toBeInTheDocument();
  });

  it("renders 'Profil ambigu' when deviceState=ambiguous", () => {
    render(
      <LuniiDecisionPanel
        deviceState="ambiguous"
        deviceReason="Profil ambigu: 2 candidats détectés"
        onEdit={noop}
      />,
    );
    expect(screen.getAllByText(/profil ambigu/i).length).toBeGreaterThan(0);
    expect(screen.getByText(/2 candidats détectés/i)).toBeInTheDocument();
  });

  it("renders 'Détection indisponible' when deviceState=error", () => {
    render(<LuniiDecisionPanel deviceState="error" onEdit={noop} />);
    expect(screen.getByText(/détection indisponible/i)).toBeInTheDocument();
    const cta = screen.getByRole("button", { name: /envoyer vers la lunii/i });
    const reasonId = cta.getAttribute("aria-describedby");
    expect(document.getElementById(reasonId as string)).toHaveTextContent(
      /envoi indisponible: détection en échec/i,
    );
  });

  it("renders 'Détection en cours…' transient state and hides the refresh button", () => {
    const onRefreshDevice = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="scanning"
        onEdit={noop}
        onRefreshDevice={onRefreshDevice}
      />,
    );
    expect(screen.getAllByText(/détection en cours/i).length).toBeGreaterThan(0);
    expect(
      screen.queryByRole("button", { name: /réessayer la détection/i }),
    ).toBeNull();
  });

  it("shows the refresh button when onRefreshDevice is provided and not scanning", () => {
    render(
      <LuniiDecisionPanel
        deviceState="absent"
        onEdit={noop}
        onRefreshDevice={() => {}}
      />,
    );
    expect(
      screen.getByRole("button", { name: /réessayer la détection/i }),
    ).toBeInTheDocument();
  });

  it("invokes onRefreshDevice when the refresh button is clicked", async () => {
    const user = userEvent.setup();
    const onRefreshDevice = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="error"
        onEdit={noop}
        onRefreshDevice={onRefreshDevice}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: /réessayer la détection/i }),
    );
    expect(onRefreshDevice).toHaveBeenCalledTimes(1);
  });

  it("never enables the send CTA in any MVP Phase 1 device state", () => {
    for (const state of [
      "absent",
      "idle",
      "unsupported",
      "ambiguous",
      "scanning",
      "error",
    ] as const) {
      const { unmount } = render(
        <LuniiDecisionPanel deviceState={state} onEdit={noop} />,
      );
      const cta = screen.getByRole("button", {
        name: /envoyer vers la lunii/i,
      });
      expect(cta).toHaveAttribute("aria-disabled", "true");
      unmount();
    }
  });

  it("information does not rely on color alone — every state exposes a textual chip label", () => {
    for (const state of [
      "absent",
      "idle",
      "unsupported",
      "ambiguous",
      "scanning",
      "error",
    ] as const) {
      const { unmount } = render(
        <LuniiDecisionPanel deviceState={state} onEdit={noop} />,
      );
      // The device-state region must always carry a non-empty text
      // label that conveys the state in words (UX-DR32).
      const region = screen.getByRole("region", { name: /état de l'appareil/i });
      expect(region.textContent ?? "").not.toEqual("");
      unmount();
    }
  });

  // --- Selection wiring (existing behavior preserved) ---

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

  it("the send CTA's reason text matches the deviceReason override when provided", () => {
    render(
      <LuniiDecisionPanel
        deviceState="error"
        deviceReason="Détection indisponible: vérifie le câble USB."
        onEdit={noop}
      />,
    );
    const cta = screen.getByRole("button", { name: /envoyer vers la lunii/i });
    const reasonId = cta.getAttribute("aria-describedby");
    expect(document.getElementById(reasonId as string)).toHaveTextContent(
      /vérifie le câble usb/i,
    );
  });
});
