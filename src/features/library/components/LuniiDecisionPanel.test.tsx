import { render, screen, within } from "@testing-library/react";
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

  it("labels the device→library operation as a copy, not an import (matrix wording)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        supportedOperations={{
          readLibrary: true,
          inspectStory: true,
          importStory: true,
          writeStory: false,
        }}
        onEdit={noop}
      />,
    );
    expect(
      screen.getByText(/copie dans la bibliothèque locale/i),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/import vers la bibliothèque locale/i),
    ).toBeNull();
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

  // --- Pre-transfer comparison (AC1, AC2, AC3) ---

  it("does not render the comparison section when no comparison prop is given", () => {
    render(<LuniiDecisionPanel deviceState="idle" onEdit={noop} />);
    expect(
      screen.queryByRole("region", { name: /comparaison avant envoi/i }),
    ).toBeNull();
  });

  it("renders a distinct sober hint per no-comparison reason", () => {
    const cases: Array<["no-selection" | "multi-selection" | "no-device", RegExp]> = [
      ["no-selection", /sélectionne une histoire locale pour comparer/i],
      ["multi-selection", /sélectionne une seule histoire locale/i],
      ["no-device", /branche une lunii lisible/i],
    ];
    for (const [reason, expected] of cases) {
      const { unmount } = render(
        <LuniiDecisionPanel
          deviceState="idle"
          comparison={{ kind: "none", reason }}
          onEdit={noop}
        />,
      );
      const region = screen.getByRole("region", {
        name: /comparaison avant envoi/i,
      });
      expect(region).toHaveTextContent(expected);
      // The three reasons must not collapse into the same wording.
      const others = cases
        .filter(([r]) => r !== reason)
        .map(([, rx]) => rx);
      for (const otherRx of others) {
        expect(region).not.toHaveTextContent(otherRx);
      }
      unmount();
    }
  });

  it("wires an actionable retry CTA on a comparison error", async () => {
    const user = userEvent.setup();
    const onRetryComparison = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        comparison={{
          kind: "error",
          error: {
            code: "DEVICE_SCAN_FAILED",
            message: "Comparaison indisponible: l'appareil connecté a changé.",
            userAction: "Rebranche la Lunii souhaitée puis réessaie.",
            details: null,
          },
        }}
        onRetryComparison={onRetryComparison}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /comparaison avant envoi/i,
    });
    const retry = within(region).getByRole("button", {
      name: /réessayer la comparaison/i,
    });
    await user.click(retry);
    expect(onRetryComparison).toHaveBeenCalledTimes(1);
  });

  it("renders a progressbar while the comparison loads", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        comparison={{ kind: "loading" }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /comparaison avant envoi/i,
    });
    expect(within(region).getByRole("progressbar")).toBeInTheDocument();
    expect(region).toHaveTextContent(/comparaison en cours/i);
  });

  it("renders the 'new' verdict with a textual (non-color) chip", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        comparison={{ kind: "ready", onDevice: false, unchangedCount: 2 }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /comparaison avant envoi/i,
    });
    // The verdict is conveyed in words, never by color alone (UX-DR32).
    expect(region).toHaveTextContent(/nouvelle sur l'appareil/i);
    expect(region).toHaveTextContent(/serait ajoutée à l'appareil/i);
    expect(region).toHaveTextContent(
      /2 autres histoires de l'appareil resteront inchangées/i,
    );
  });

  it("renders the 'replace' verdict", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        comparison={{ kind: "ready", onDevice: true, unchangedCount: 1 }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /comparaison avant envoi/i,
    });
    expect(region).toHaveTextContent(/déjà présente sur l'appareil/i);
    expect(region).toHaveTextContent(/un envoi la remplacerait/i);
    expect(region).toHaveTextContent(
      /1 autre histoire de l'appareil restera inchangée/i,
    );
  });

  it("pluralizes the unchanged-count line (0 / 1 / many)", () => {
    const cases: Array<[number, RegExp]> = [
      [0, /aucune autre histoire de l'appareil ne sera modifiée/i],
      [1, /1 autre histoire de l'appareil restera inchangée/i],
      [3, /3 autres histoires de l'appareil resteront inchangées/i],
    ];
    for (const [count, expected] of cases) {
      const { unmount } = render(
        <LuniiDecisionPanel
          deviceState="idle"
          comparison={{ kind: "ready", onDevice: false, unchangedCount: count }}
          onEdit={noop}
        />,
      );
      expect(
        screen.getByRole("region", { name: /comparaison avant envoi/i }),
      ).toHaveTextContent(expected);
      unmount();
    }
  });

  it("renders a comparison error in-context (role=alert), never a toast", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        comparison={{
          kind: "error",
          error: {
            code: "DEVICE_SCAN_FAILED",
            message: "Comparaison indisponible: l'appareil connecté a changé.",
            userAction: "Rebranche la Lunii souhaitée puis réessaie.",
            details: null,
          },
        }}
        onEdit={noop}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/l'appareil connecté a changé/i);
    expect(alert).toHaveTextContent(/rebranche la lunii/i);
    // No polite status region carries the critical comparison error.
    screen
      .queryAllByRole("status")
      .forEach((s) => expect(s).not.toHaveTextContent(/comparaison indisponible/i));
  });

  it("keeps the send CTA disabled even when the comparison is ready (AC2)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        comparison={{ kind: "ready", onDevice: false, unchangedCount: 0 }}
        onEdit={noop}
      />,
    );
    const cta = screen.getByRole("button", { name: /envoyer vers la lunii/i });
    expect(cta).toHaveAttribute("aria-disabled", "true");
    const reasonId = cta.getAttribute("aria-describedby");
    expect(document.getElementById(reasonId as string)).toHaveTextContent(
      /transfert pas encore activé/i,
    );
  });

  it("announces the verdict in a polite live region so it is vocalized on async arrival", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        comparison={{ kind: "ready", onDevice: false, unchangedCount: 2 }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /comparaison avant envoi/i,
    });
    // The loading→ready verdict must live in a live region (UX-DR21/UX-DR32).
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveTextContent(/nouvelle sur l'appareil/i);
  });
});
