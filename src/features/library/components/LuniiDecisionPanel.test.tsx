import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { LuniiDecisionPanel, type TransferView } from "./LuniiDecisionPanel";

describe("<LuniiDecisionPanel />", () => {
  const noop = () => {};

  const checksumBlocker = {
    axis: "structure" as const,
    cause: "checksumMismatch" as const,
    message: "Les données locales de l'histoire ont changé.",
    userAction: "Restaure une sauvegarde saine de l'histoire.",
  };
  const profileBlocker = {
    axis: "deviceProfile" as const,
    cause: "metadataUnsupported" as const,
    // Mirrors the family-neutral wire copy (a broken FLAM reaches this
    // reason too since FLAM recognition).
    message: "Le profil de l'appareil connecté n'est pas pris en charge.",
    userAction:
      "Consulte le profil de support pour voir les appareils compatibles.",
  };

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

  describe("recognized without capability (zero-capability supported profile)", () => {
    const zeroCapabilities = {
      readLibrary: false,
      inspectStory: false,
      importStory: false,
      writeStory: false,
    };

    it("renders the STATIC 'Appareil reconnu — …' chip instead of 'Appareil prêt' (recognized ≠ ready)", () => {
      // Generic zero-capability profile: since the FLAM read capabilities
      // activated, no live family carries this state — it survives,
      // DECLARED, for any future zero-capability family.
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Conteuse X"
          supportedOperations={zeroCapabilities}
          onEdit={noop}
        />,
      );
      expect(
        screen.getByText(/appareil reconnu — conteuse x/i),
      ).toBeInTheDocument();
      expect(screen.queryByText(/appareil prêt/i)).toBeNull();
      // A durable state, never an action error: no alert role on the
      // recognized chip nor on the explanation.
      const device = screen.getByRole("region", {
        name: /état de l'appareil/i,
      });
      expect(within(device).queryAllByRole("alert")).toHaveLength(0);
    });

    it("suffixes the recognized chip with the FAMILY name, never a cohort label", () => {
      // Contract: `Appareil reconnu — {famille}` (product-language.md).
      // A hypothetical zero-capability profile whose cohort label
      // differs from its family name must render the family.
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Lunii V3"
          supportedOperations={zeroCapabilities}
          deviceFamily="lunii"
          onEdit={noop}
        />,
      );
      expect(screen.getByText(/^appareil reconnu — lunii$/i)).toBeInTheDocument();
      expect(screen.queryByText(/appareil reconnu — lunii v3/i)).toBeNull();
    });

    it("renders the four capability lines as '—' for a generic zero-capability profile", () => {
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Conteuse X"
          supportedOperations={zeroCapabilities}
          onEdit={noop}
        />,
      );
      const list = screen.getByRole("list", {
        name: /opérations supportées par l'appareil détecté/i,
      });
      const lines = within(list)
        .getAllByRole("listitem")
        .map((li) => li.textContent);
      expect(lines).toEqual([
        "— Lecture bibliothèque appareil",
        "— Inspection d'histoire",
        "— Copie dans la bibliothèque locale",
        "— Transfert vers la Lunii",
      ]);
    });

    it("renders the FLAM read capabilities active with the family-correct transfer line", () => {
      // The real FLAM Gen1 matrix line (✅✅✅❌): three activated lines,
      // the non-activated transfer line with the device-generic wording.
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="FLAM"
          supportedOperations={{
            readLibrary: true,
            inspectStory: true,
            importStory: true,
            writeStory: false,
          }}
          deviceFamily="flam"
          onEdit={noop}
        />,
      );
      const list = screen.getByRole("list", {
        name: /opérations supportées par l'appareil détecté/i,
      });
      const lines = within(list)
        .getAllByRole("listitem")
        .map((li) => li.textContent);
      expect(lines).toEqual([
        "✓ Lecture bibliothèque appareil",
        "✓ Inspection d'histoire",
        "✓ Copie dans la bibliothèque locale",
        "— Transfert vers l'appareil",
      ]);
      // The Lunii-specific wording must NOT legend a FLAM line.
      expect(within(list).queryByText(/transfert vers la lunii/i)).toBeNull();
    });

    it("keeps 'Transfert vers la Lunii' on a capability-bearing Lunii panel", () => {
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Lunii V3"
          supportedOperations={{
            readLibrary: true,
            inspectStory: true,
            importStory: false,
            writeStory: false,
          }}
          deviceFamily="lunii"
          onEdit={noop}
        />,
      );
      expect(screen.getByText(/appareil prêt — lunii v3/i)).toBeInTheDocument();
      expect(
        screen.getByText(/— transfert vers la lunii/i),
      ).toBeInTheDocument();
    });

    it("renders the text-only support-profile explanation in the recognized idle state, WITHOUT navigation", () => {
      const onConsultSupportProfile = vi.fn();
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Conteuse X"
          supportedOperations={zeroCapabilities}
          onEdit={noop}
          onConsultSupportProfile={onConsultSupportProfile}
        />,
      );
      // The explanation carries the support-profile pointer as TEXT
      // ONLY (zero navigation, zero network — NFR14): the external-link
      // CTA must NOT be offered in this state, even when the route
      // wires onConsultSupportProfile for the other states.
      expect(
        screen.getByText(
          /appareil reconnu, aucune opération activée dans cette version\. consulte le profil de support pour comprendre ce qui est permis\./i,
        ),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("button", {
          name: /consulter le profil de support/i,
        }),
      ).toBeNull();
      expect(onConsultSupportProfile).not.toHaveBeenCalled();
    });

    it("does NOT render the explanation on a capability-bearing idle (Lunii)", () => {
      const onConsultSupportProfile = vi.fn();
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Lunii V3"
          supportedOperations={{
            readLibrary: true,
            inspectStory: true,
            importStory: false,
            writeStory: false,
          }}
          deviceFamily="lunii"
          onEdit={noop}
          onConsultSupportProfile={onConsultSupportProfile}
        />,
      );
      expect(
        screen.queryByText(
          /appareil reconnu, aucune opération activée dans cette version\./i,
        ),
      ).toBeNull();
      expect(
        screen.queryByRole("button", {
          name: /consulter le profil de support/i,
        }),
      ).toBeNull();
    });

    it("legacy send CTA follows the capability-closed path, never the 'MVP Phase 1' promise", () => {
      // Without a transfer prop (legacy fallback), the idle reason for a
      // zero-capability profile is the V3-pattern copy — the "transfert
      // pas encore activé (MVP Phase 1)" copy stays EXCLUSIVE to
      // write-planned Lunii cohorts.
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Conteuse X"
          supportedOperations={zeroCapabilities}
          onEdit={noop}
        />,
      );
      const cta = screen.getByRole("button", {
        name: /envoyer vers la lunii/i,
      });
      const reasonId = cta.getAttribute("aria-describedby");
      const reason = document.getElementById(reasonId as string);
      expect(reason).toHaveTextContent(
        /envoi indisponible: profil non supporté/i,
      );
      expect(screen.queryByText(/mvp phase 1/i)).toBeNull();
    });
  });

  describe("FLAM panel (read capabilities active)", () => {
    const flamCapabilities = {
      readLibrary: true,
      inspectStory: true,
      importStory: true,
      writeStory: false,
    };

    it("renders 'Appareil prêt — FLAM' through the EXISTING hasAnyCapability rule", () => {
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="FLAM"
          supportedOperations={flamCapabilities}
          deviceFamily="flam"
          onEdit={noop}
        />,
      );
      expect(screen.getByText(/appareil prêt — flam/i)).toBeInTheDocument();
      expect(screen.queryByText(/appareil reconnu/i)).toBeNull();
    });

    it("does NOT render the zero-capability explanation on a capability-bearing FLAM idle", () => {
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="FLAM"
          supportedOperations={flamCapabilities}
          deviceFamily="flam"
          onEdit={noop}
        />,
      );
      expect(
        screen.queryByText(
          /appareil reconnu, aucune opération activée dans cette version\./i,
        ),
      ).toBeNull();
    });

    it("legacy fallback never renders the 'MVP Phase 1' promise for a capability-bearing FLAM", () => {
      // Without a transfer prop (legacy fallback), an idle FLAM with its
      // read capabilities active must follow the capability-closed path:
      // the "MVP Phase 1" promise stays EXCLUSIVE to write-planned Lunii
      // cohorts (a FLAM write is not planned in this phase).
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="FLAM"
          supportedOperations={flamCapabilities}
          deviceFamily="flam"
          onEdit={noop}
        />,
      );
      const cta = screen.getByRole("button", {
        name: /envoyer vers l'appareil/i,
      });
      const reasonId = cta.getAttribute("aria-describedby");
      const reason = document.getElementById(reasonId as string);
      expect(reason).toHaveTextContent(
        /envoi indisponible: profil non supporté/i,
      );
      expect(screen.queryByText(/mvp phase 1/i)).toBeNull();
    });

    it("renders the family-correct 'Envoyer vers l'appareil' send CTA on the transfer region", () => {
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="FLAM"
          supportedOperations={flamCapabilities}
          deviceFamily="flam"
          transfer={{
            kind: "unavailable",
            reason: "Envoi indisponible: profil non supporté",
          }}
          onEdit={noop}
        />,
      );
      const cta = screen.getByRole("button", {
        name: /envoyer vers l'appareil/i,
      });
      expect(cta).toHaveAttribute("aria-disabled", "true");
      expect(screen.queryByText(/envoyer vers la lunii/i)).toBeNull();
      // The capability-closed reason (the V3 pattern) legends the CTA.
      const reasonId = cta.getAttribute("aria-describedby");
      const reason = document.getElementById(reasonId as string);
      expect(reason).toHaveTextContent(
        /envoi indisponible: profil non supporté/i,
      );
    });

    it("renders the shared validation/comparison copies family-correct on a FLAM panel", () => {
      // The shared sections reach a FLAM panel now that its read
      // capabilities light the flow: their copies must never name the
      // Lunii there.
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="FLAM"
          supportedOperations={flamCapabilities}
          deviceFamily="flam"
          comparison={{ kind: "none", reason: "no-device" }}
          validation={{ kind: "none" }}
          onEdit={noop}
        />,
      );
      expect(
        screen.getByText(
          /branche un appareil lisible pour comparer l'histoire sélectionnée avant l'envoi\./i,
        ),
      ).toBeInTheDocument();
      expect(
        screen.getByText(
          /sélectionne une histoire locale et branche un appareil lisible pour vérifier la compatibilité avant l'envoi\./i,
        ),
      ).toBeInTheDocument();
      expect(screen.queryByText(/lunii lisible/i)).toBeNull();
    });

    it("headings a deviceProfile blocker group 'Compatibilité appareil' on a FLAM panel", () => {
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="FLAM"
          supportedOperations={flamCapabilities}
          deviceFamily="flam"
          validation={{
            kind: "ready",
            verdict: "blocked",
            blockers: [
              {
                axis: "deviceProfile",
                cause: "metadataUnsupported",
                message: "Le profil de l'appareil connecté n'est pas pris en charge.",
                userAction:
                  "Consulte le profil de support pour voir les appareils compatibles.",
              },
            ],
          }}
          onEdit={noop}
        />,
      );
      expect(
        screen.getByRole("heading", { name: /compatibilité appareil/i }),
      ).toBeInTheDocument();
      expect(screen.queryByText(/compatibilité lunii/i)).toBeNull();
    });

    it("keeps the Lunii wording VERBATIM for the shared copies on a Lunii panel", () => {
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Lunii Origine"
          supportedOperations={{
            readLibrary: true,
            inspectStory: true,
            importStory: true,
            writeStory: true,
          }}
          deviceFamily="lunii"
          comparison={{ kind: "none", reason: "no-device" }}
          validation={{
            kind: "ready",
            verdict: "blocked",
            blockers: [
              {
                axis: "deviceProfile",
                cause: "metadataUnsupported",
                message: "Le profil de l'appareil connecté n'est pas pris en charge.",
                userAction:
                  "Consulte le profil de support pour voir les appareils compatibles.",
              },
            ],
          }}
          onEdit={noop}
        />,
      );
      expect(
        screen.getByText(
          /branche une lunii lisible pour comparer l'histoire sélectionnée avant l'envoi\./i,
        ),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("heading", { name: /compatibilité lunii/i }),
      ).toBeInTheDocument();
    });

    it("keeps the Lunii send CTA VERBATIM on a Lunii panel", () => {
      render(
        <LuniiDecisionPanel
          deviceState="idle"
          deviceLabel="Lunii Origine"
          supportedOperations={{
            readLibrary: true,
            inspectStory: true,
            importStory: true,
            writeStory: true,
          }}
          deviceFamily="lunii"
          transfer={{
            kind: "unavailable",
            reason: "Envoi indisponible: prépare l'histoire d'abord",
          }}
          onEdit={noop}
        />,
      );
      expect(
        screen.getByRole("button", { name: /envoyer vers la lunii/i }),
      ).toBeInTheDocument();
      expect(screen.queryByText(/envoyer vers l'appareil/i)).toBeNull();
    });
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

  it("enables the send CTA ONLY on a ready transfer view, never in any other transfer state", async () => {
    // `ready` (writable cohort + Préparée + clear target) → active CTA + onSend.
    const onSend = vi.fn();
    const { unmount } = render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "ready" }}
        onSend={onSend}
        onEdit={noop}
      />,
    );
    const ready = screen.getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    expect(ready).not.toHaveAttribute("aria-disabled", "true");
    await userEvent.click(ready);
    expect(onSend).toHaveBeenCalledTimes(1);
    unmount();

    // Every other transfer state keeps the send affordance non-active: either a
    // disabled CTA (unavailable) or no enabled send button at all (job/terminal).
    const nonReady: TransferView[] = [
      { kind: "unavailable", reason: "Envoi indisponible: profil non supporté" },
      { kind: "transferring", progress: null, phase: null },
      { kind: "verifying" },
      { kind: "verified", changed: "« Mon histoire » est sur la Lunii.", unchanged: "m" },
      { kind: "partial", message: "m", userAction: "a" },
      { kind: "retryable", message: "m", userAction: "a" },
      {
        kind: "error",
        error: {
          code: "TRANSFER_FAILED",
          message: "m",
          userAction: "a",
          details: null,
        },
      },
    ];
    for (const view of nonReady) {
      const { unmount: u } = render(
        <LuniiDecisionPanel deviceState="idle" transfer={view} onEdit={noop} />,
      );
      const cta = screen.queryByRole("button", {
        name: /envoyer vers la lunii/i,
      });
      if (cta) expect(cta).toHaveAttribute("aria-disabled", "true");
      u();
    }
  });

  it("without a transfer prop, keeps the legacy disabled send CTA in the device region (back-compat)", () => {
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

  // --- Pre-transfer validation verdict (AC1, AC2, AC3) ---

  it("does not render the validation section when no validation prop is given", () => {
    render(<LuniiDecisionPanel deviceState="idle" onEdit={noop} />);
    expect(
      screen.queryByRole("region", { name: /validation avant envoi/i }),
    ).toBeNull();
  });

  it("renders a sober hint for the validation 'none' state", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        validation={{ kind: "none" }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /validation avant envoi/i,
    });
    expect(region).toHaveTextContent(/vérifier la compatibilité avant l'envoi/i);
  });

  it("renders a progressbar while the validation loads", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        validation={{ kind: "loading" }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /validation avant envoi/i,
    });
    expect(within(region).getByRole("progressbar")).toBeInTheDocument();
    expect(region).toHaveTextContent(/validation en cours/i);
  });

  it("renders the 'présumée transférable' verdict with a textual (non-color) chip", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        validation={{
          kind: "ready",
          verdict: "presumedTransferable",
          blockers: [],
        }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /validation avant envoi/i,
    });
    // The verdict is conveyed in words, never by color alone (UX-DR32).
    expect(region).toHaveTextContent(/présumée transférable/i);
    expect(region).toHaveTextContent(/aucun blocage/i);
  });

  it("renders the 'à corriger' verdict and the fixable blocker's next gesture (AC2)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        validation={{
          kind: "ready",
          verdict: "toFix",
          blockers: [
            {
              axis: "structure",
              cause: "titleInvalid",
              message: "Le titre enregistré n'est pas valide.",
              userAction: "Renomme l'histoire avec un titre valide.",
            },
          ],
        }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /validation avant envoi/i,
    });
    expect(region).toHaveTextContent(/à corriger/i);
    expect(region).toHaveTextContent(/le titre enregistré n'est pas valide/i);
    expect(region).toHaveTextContent(/renomme l'histoire/i);
  });

  it("groups blockers by axis (canonical vs Lunii) for a 'bloquée' verdict (AC1)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        validation={{
          kind: "ready",
          verdict: "blocked",
          blockers: [checksumBlocker, profileBlocker],
        }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /validation avant envoi/i,
    });
    expect(region).toHaveTextContent(/bloquée/i);
    expect(
      within(region).getByRole("heading", { name: /validité rustory/i }),
    ).toBeInTheDocument();
    expect(
      within(region).getByRole("heading", { name: /compatibilité lunii/i }),
    ).toBeInTheDocument();
    // Each blocker's userAction is rendered verbatim (AC2).
    expect(region).toHaveTextContent(/restaure une sauvegarde saine/i);
    expect(region).toHaveTextContent(/consulte le profil de support/i);
  });

  it("renders a validation error in-context (role=alert) with an actionable retry, never a toast", async () => {
    const user = userEvent.setup();
    const onRetryValidation = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        validation={{
          kind: "error",
          error: {
            code: "DEVICE_SCAN_FAILED",
            message: "L'appareil a changé pendant la validation.",
            userAction: "Vérifie que la Lunii est branchée puis réessaie.",
            details: null,
          },
        }}
        onRetryValidation={onRetryValidation}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /validation avant envoi/i,
    });
    const alert = within(region).getByRole("alert");
    expect(alert).toHaveTextContent(
      /l'appareil a changé pendant la validation/i,
    );
    const retry = within(region).getByRole("button", {
      name: /réessayer la validation/i,
    });
    await user.click(retry);
    expect(onRetryValidation).toHaveBeenCalledTimes(1);
  });

  it("keeps the send CTA disabled even when the verdict is présumée transférable (AC3/FR34)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        validation={{
          kind: "ready",
          verdict: "presumedTransferable",
          blockers: [],
        }}
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

  it("announces the verdict in a polite live region", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        validation={{
          kind: "ready",
          verdict: "blocked",
          blockers: [checksumBlocker],
        }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", {
      name: /validation avant envoi/i,
    });
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveTextContent(/bloquée/i);
  });
});

describe("<LuniiDecisionPanel /> — preparation", () => {
  const noop = () => {};

  const titleBlocker = {
    axis: "structure" as const,
    cause: "titleInvalid" as const,
    message: "Le titre enregistré de l'histoire n'est pas valide.",
    userAction: "Renomme l'histoire avec un titre valide.",
  };

  it("renders the Préparer CTA disabled with the standardized reason when unavailable", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        preparation={{
          kind: "unavailable",
          reason: "Préparation indisponible: corrige les blocages d'abord",
        }}
        onEdit={noop}
      />,
    );
    const cta = screen.getByRole("button", { name: /^préparer$/i });
    expect(cta).toHaveAttribute("aria-disabled", "true");
    const reasonId = cta.getAttribute("aria-describedby");
    const reason = document.getElementById(reasonId as string);
    expect(reason).toHaveTextContent(
      /préparation indisponible: corrige les blocages d'abord/i,
    );
  });

  it("activates the Préparer CTA when ready and calls onPrepare", async () => {
    const onPrepare = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        preparation={{ kind: "ready" }}
        onPrepare={onPrepare}
        onEdit={noop}
      />,
    );
    const cta = screen.getByRole("button", { name: /^préparer$/i });
    expect(cta).not.toHaveAttribute("aria-disabled", "true");
    await userEvent.click(cta);
    expect(onPrepare).toHaveBeenCalledTimes(1);
  });

  it("shows the named phases (en vérification, en préparation) without a fake percentage", () => {
    const { rerender } = render(
      <LuniiDecisionPanel
        deviceState="idle"
        preparation={{ kind: "preflight" }}
        onEdit={noop}
      />,
    );
    expect(screen.getByText(/en vérification/i)).toBeInTheDocument();

    rerender(
      <LuniiDecisionPanel
        deviceState="idle"
        preparation={{ kind: "preparing", progress: null }}
        onEdit={noop}
      />,
    );
    expect(screen.getByText(/en préparation/i)).toBeInTheDocument();
    expect(screen.getByText(/préparation en cours…/i)).toBeInTheDocument();
    // No percentage is shown (honest progress; MVP sends no reliable fraction).
    expect(screen.queryByText(/%/)).toBeNull();
  });

  it("shows the discreet Préparée indicator and STILL keeps the send CTA disabled (FR34)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        preparation={{ kind: "prepared" }}
        onEdit={noop}
      />,
    );
    expect(screen.getByText(/préparée/i)).toBeInTheDocument();
    // Reaching `prepared` NEVER enables the send.
    const send = screen.getByRole("button", { name: /envoyer vers la lunii/i });
    expect(send).toHaveAttribute("aria-disabled", "true");
    const reasonId = send.getAttribute("aria-describedby");
    expect(document.getElementById(reasonId as string)).toHaveTextContent(
      /transfert pas encore activé \(mvp phase 1\)/i,
    );
  });

  it("renders a recoverable failure in-context with Relancer (never a toast) and its blockers", async () => {
    const onRetryPreparation = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        preparation={{
          kind: "retryable",
          message: "La préparation ne peut pas démarrer.",
          userAction: "Corrige les points signalés puis relance la préparation.",
          blockers: [titleBlocker],
        }}
        onRetryPreparation={onRetryPreparation}
        onEdit={noop}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/la préparation ne peut pas démarrer/i);
    expect(alert).toHaveTextContent(/corrige les points signalés/i);
    // The non-passing preflight reports its blocker (reused 3.x grouping).
    expect(alert).toHaveTextContent(
      /le titre enregistré de l'histoire n'est pas valide/i,
    );
    await userEvent.click(
      screen.getByRole("button", { name: /relancer la préparation/i }),
    );
    expect(onRetryPreparation).toHaveBeenCalledTimes(1);
  });

  it("renders a transport error in-context with Réessayer", async () => {
    const onRetryPreparation = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        preparation={{
          kind: "error",
          error: {
            code: "PREPARATION_FAILED",
            message: "Préparation indisponible: réponse invalide.",
            userAction: "Réessaie la préparation.",
            details: null,
          },
        }}
        onRetryPreparation={onRetryPreparation}
        onEdit={noop}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      /préparation indisponible: réponse invalide/i,
    );
    await userEvent.click(
      screen.getByRole("button", { name: /réessayer la préparation/i }),
    );
    expect(onRetryPreparation).toHaveBeenCalledTimes(1);
  });

  it("hosts the preparation section in a polite live region", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        preparation={{ kind: "preflight" }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^préparation$/i });
    expect(region).toHaveAttribute("aria-live", "polite");
  });

  it("does not render the preparation section when the prop is omitted", () => {
    render(<LuniiDecisionPanel deviceState="idle" onEdit={noop} />);
    expect(
      screen.queryByRole("region", { name: /^préparation$/i }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: /^préparer$/i })).toBeNull();
  });
});

describe("<LuniiDecisionPanel /> — transfer", () => {
  const noop = () => {};

  it("renders the send CTA disabled with the standardized reason when unavailable", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "unavailable",
          reason: "Envoi indisponible: prépare l'histoire d'abord",
        }}
        onEdit={noop}
      />,
    );
    const cta = screen.getByRole("button", { name: /envoyer vers la lunii/i });
    expect(cta).toHaveAttribute("aria-disabled", "true");
    const reasonId = cta.getAttribute("aria-describedby");
    expect(document.getElementById(reasonId as string)).toHaveTextContent(
      /envoi indisponible: prépare l'histoire d'abord/i,
    );
  });

  it("disables the send CTA on a V3-shaped 'profil non supporté' reason", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "unavailable",
          reason: "Envoi indisponible: profil non supporté",
        }}
        onEdit={noop}
      />,
    );
    const cta = screen.getByRole("button", { name: /envoyer vers la lunii/i });
    expect(cta).toHaveAttribute("aria-disabled", "true");
    const reasonId = cta.getAttribute("aria-describedby");
    expect(document.getElementById(reasonId as string)).toHaveTextContent(
      /envoi indisponible: profil non supporté/i,
    );
  });

  it("disables the send CTA with the native-non-transferable reason", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "unavailable",
          reason:
            "Envoi indisponible: histoire native non transférable (pas de pack appareil)",
        }}
        onEdit={noop}
      />,
    );
    const cta = screen.getByRole("button", { name: /envoyer vers la lunii/i });
    expect(cta).toHaveAttribute("aria-disabled", "true");
    const reasonId = cta.getAttribute("aria-describedby");
    expect(document.getElementById(reasonId as string)).toHaveTextContent(
      /histoire native non transférable \(pas de pack appareil\)/i,
    );
  });

  it("activates the send CTA when ready and calls onSend (no confirmation modal)", async () => {
    const onSend = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "ready" }}
        onSend={onSend}
        onEdit={noop}
      />,
    );
    const cta = screen.getByRole("button", { name: /envoyer vers la lunii/i });
    expect(cta).not.toHaveAttribute("aria-disabled", "true");
    await userEvent.click(cta);
    expect(onSend).toHaveBeenCalledTimes(1);
    // No confirmation dialog appears (AC1).
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows the named 'en transfert' phase without a fake percentage", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "transferring", progress: null, phase: null }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^transfert$/i });
    expect(within(region).getByText(/en transfert/i)).toBeInTheDocument();
    expect(within(region).getByText(/transfert en cours…/i)).toBeInTheDocument();
    expect(within(region).queryByText(/%/)).toBeNull();
  });

  it("renders the TRANSIENT verifying state — 'écriture effectuée — vérification à venir', not yet a success", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "verifying" }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^transfert$/i });
    expect(region).toHaveTextContent(/écriture effectuée/i);
    expect(region).toHaveTextContent(/vérification à venir/i);
    // The success vocabulary is reserved for the proven `verified` terminal.
    expect(region).not.toHaveTextContent(/transférée et vérifiée/i);
    expect(region).not.toHaveTextContent(/état partiel/i);
  });

  it("renders the 'transférée et vérifiée' success terminal with the AC2 summary (polite, never a toast)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "verified",
          // Composed-in-Rust lines, rendered VERBATIM by the panel.
          changed: "« Mon histoire » est maintenant sur la Lunii.",
          unchanged: "2 autres histoires de l'appareil restent inchangées.",
        }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^transfert$/i });
    // The FIRST appearance of the canonical success label, success-toned glyph.
    const chip = within(region)
      .getByText(/transférée et vérifiée/i)
      .closest(".ds-chip");
    expect(chip).toHaveClass("ds-chip--success");
    // The summary lines are rendered verbatim (what changed + what stayed).
    expect(region).toHaveTextContent(/«\s*Mon histoire\s*».*sur la lunii/i);
    expect(region).toHaveTextContent(/2 autres histoires.*restent inchangées/i);
    // A confirmation is polite, never an alert/toast.
    expect(within(region).queryByRole("alert")).toBeNull();
    expect(screen.queryAllByRole("status")).toHaveLength(0);
  });

  it("renders the update and already-up-to-date verified summaries VERBATIM under the unchanged chip (FR23)", () => {
    // The three summary variants are composed in Rust; the panel renders the
    // lines verbatim and the state chip NEVER varies (controlled vocabulary).
    for (const changed of [
      "« Mon histoire » a été mise à jour sur la Lunii.",
      "« Mon histoire » était déjà à jour sur la Lunii.",
    ]) {
      const { unmount } = render(
        <LuniiDecisionPanel
          deviceState="idle"
          transfer={{
            kind: "verified",
            changed,
            unchanged: "2 autres histoires de l'appareil restent inchangées.",
          }}
          onEdit={noop}
        />,
      );
      const region = screen.getByRole("region", { name: /^transfert$/i });
      // The exact composed line, byte-for-byte.
      expect(within(region).getByText(changed)).toBeInTheDocument();
      // The chip stays the canonical success label — "mise à jour" is never a
      // chip or a state, only a summary sentence.
      const chip = within(region)
        .getByText(/transférée et vérifiée/i)
        .closest(".ds-chip");
      expect(chip).toHaveClass("ds-chip--success");
      expect(within(region).queryByRole("alert")).toBeNull();
      unmount();
    }
  });

  it("renders the devicePackUnprovable refusal through the retryable path with its honest copy and gesture", async () => {
    // FR23/AC1 — the protective refusal is a plain `retryable` for the panel:
    // message + gesture verbatim, role="alert" in-context, never a toast.
    const onRetryTransfer = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "retryable",
          message:
            "Envoi interrompu : la copie présente sur l'appareil est dans un état que Rustory ne reconnaît pas, rien n'a été modifié.",
          userAction:
            "Vérifie l'appareil, débranche-le puis rebranche-le, puis relance l'envoi.",
        }}
        onRetryTransfer={onRetryTransfer}
        onEdit={noop}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      /la copie présente sur l'appareil est dans un état que rustory ne reconnaît pas/i,
    );
    expect(alert).toHaveTextContent(/rien n'a été modifié/i);
    expect(alert).toHaveTextContent(
      /vérifie l'appareil, débranche-le puis rebranche-le/i,
    );
    // The honest copy never claims the device refused.
    expect(alert).not.toHaveTextContent(/la lunii a refusé/i);
    expect(within(alert).getByText(/échec récupérable/i)).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: /relancer le transfert/i }),
    );
    expect(onRetryTransfer).toHaveBeenCalledTimes(1);
  });

  it("renders the 'état partiel' terminal distinct from 'transfert incomplet' AND 'échec récupérable', with Relancer + Abandonner (AC3)", async () => {
    const onRetryTransfer = vi.fn();
    const onDismissTransfer = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "partial",
          message:
            "Envoi dans un état partiel : certains éléments n'ont pas pu être confirmés sur la Lunii.",
          userAction: "Relance l'envoi pour rétablir un état sûr.",
        }}
        onRetryTransfer={onRetryTransfer}
        onDismissTransfer={onDismissTransfer}
        onEdit={noop}
      />,
    );
    const alert = screen.getByRole("alert");
    // The DISTINCT canonical label (never the 3.5 / pack wordings). Exact match on
    // the chip — the message paragraph also contains "état partiel".
    expect(within(alert).getByText("état partiel")).toBeInTheDocument();
    expect(within(alert).queryByText(/transfert incomplet/i)).toBeNull();
    expect(within(alert).queryByText(/échec récupérable/i)).toBeNull();
    // Tone/glyph: warning (like `incomplete`) but NEVER error nor success — the
    // label text carries the distinction (non-color-only).
    const chip = within(alert).getByText("état partiel").closest(".ds-chip");
    expect(chip).toHaveClass("ds-chip--warning");
    expect(chip).not.toHaveClass("ds-chip--error");
    expect(chip).not.toHaveClass("ds-chip--success");
    // NEVER success / verification vocabulary on this non-success terminal.
    expect(alert).not.toHaveTextContent(/transférée et vérifiée/i);
    // Both recovery gestures (AC3): Relancer (full cycle) AND Abandonner.
    await userEvent.click(
      screen.getByRole("button", { name: /relancer le transfert/i }),
    );
    expect(onRetryTransfer).toHaveBeenCalledTimes(1);
    await userEvent.click(
      screen.getByRole("button", { name: /abandonner le transfert/i }),
    );
    expect(onDismissTransfer).toHaveBeenCalledTimes(1);
  });

  it("renders a recoverable failure in-context with Relancer (never a toast)", async () => {
    const onRetryTransfer = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "retryable",
          message: "Le transfert a été interrompu.",
          userAction: "Rebranche la Lunii puis relance l'envoi.",
        }}
        onRetryTransfer={onRetryTransfer}
        onEdit={noop}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/le transfert a été interrompu/i);
    expect(alert).toHaveTextContent(/rebranche la lunii/i);
    expect(within(alert).getByText(/échec récupérable/i)).toBeInTheDocument();
    // No polite status region carries the critical failure (never a toast).
    screen
      .queryAllByRole("status")
      .forEach((s) => expect(s).not.toHaveTextContent(/le transfert a été interrompu/i));
    await userEvent.click(
      screen.getByRole("button", { name: /relancer le transfert/i }),
    );
    expect(onRetryTransfer).toHaveBeenCalledTimes(1);
  });

  it("renders the 'transfert incomplet' terminal distinct from 'échec récupérable', with Relancer + Abandonner (AC2/AC3)", async () => {
    const onRetryTransfer = vi.fn();
    const onDismissTransfer = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "incomplete",
          message: "L'appareil peut contenir une copie partielle.",
          userAction: "Relance l'envoi pour rétablir un état sûr.",
        }}
        onRetryTransfer={onRetryTransfer}
        onDismissTransfer={onDismissTransfer}
        onEdit={noop}
      />,
    );
    const alert = screen.getByRole("alert");
    // Distinct canonical label — the difference is in the TEXT (non-color).
    expect(within(alert).getByText(/transfert incomplet/i)).toBeInTheDocument();
    expect(within(alert).queryByText(/échec récupérable/i)).toBeNull();
    // Non-color distinction (C3): the chip tone/glyph differs from `échoué`'s — a
    // warning↔error swap is caught here, not just by the label text.
    const incompleteChip = within(alert)
      .getByText(/transfert incomplet/i)
      .closest(".ds-chip");
    expect(incompleteChip).toHaveClass("ds-chip--warning");
    expect(incompleteChip).not.toHaveClass("ds-chip--error");
    expect(alert).toHaveTextContent(/copie partielle/i);
    // NEVER success / verification vocabulary on this terminal.
    expect(alert).not.toHaveTextContent(/transférée et vérifiée/i);
    expect(alert).not.toHaveTextContent(/état partiel/i);
    // Both recovery gestures (AC3): Relancer (full cycle) AND Abandonner.
    await userEvent.click(
      screen.getByRole("button", { name: /relancer le transfert/i }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: /abandonner le transfert/i }),
    );
    expect(onRetryTransfer).toHaveBeenCalledTimes(1);
    expect(onDismissTransfer).toHaveBeenCalledTimes(1);
  });

  it("shows a reconnect hint instead of an inert Relancer when no writable device is connected (C1)", async () => {
    const onDismissTransfer = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "incomplete",
          message: "L'appareil peut contenir une copie partielle.",
          userAction: "Relance l'envoi pour rétablir un état sûr.",
        }}
        onDismissTransfer={onDismissTransfer}
        onEdit={noop}
      />,
    );
    const alert = screen.getByRole("alert");
    // No inert Relancer button — an honest reconnect hint instead (C1).
    expect(
      within(alert).queryByRole("button", { name: /relancer le transfert/i }),
    ).toBeNull();
    expect(
      within(alert).getByText(/rebranche la lunii pour relancer/i),
    ).toBeInTheDocument();
    // Abandonner stays available even without a connected device.
    await userEvent.click(
      screen.getByRole("button", { name: /abandonner le transfert/i }),
    );
    expect(onDismissTransfer).toHaveBeenCalledTimes(1);
  });

  it("names a neutral phase before the first progress event (phase null) (C2/AC1)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "transferring", progress: null, phase: null }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^transfert$/i });
    expect(
      within(region).getByText(/préparation de l'envoi/i),
    ).toBeInTheDocument();
    // The wrong "envoi en cours" phase must NOT be claimed before the 1st progress.
    expect(within(region).queryByText(/phase : envoi en cours/i)).toBeNull();
  });

  it("offers Abandonner on a recoverable (échoué) failure too", async () => {
    const onDismissTransfer = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "retryable",
          message: "Le transfert a échoué.",
          userAction: "Relance l'envoi.",
        }}
        onRetryTransfer={noop}
        onDismissTransfer={onDismissTransfer}
        onEdit={noop}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: /abandonner le transfert/i }),
    );
    expect(onDismissTransfer).toHaveBeenCalledTimes(1);
  });

  it("offers a non-destructive 'Consulter le détail' during the transfer (no cancel)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "transferring", progress: 0.4, phase: "transfer" }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^transfert$/i });
    expect(
      within(region).getByText(/consulter le détail/i),
    ).toBeInTheDocument();
    // Explicit cancel is out of scope — no destructive affordance.
    expect(
      within(region).queryByRole("button", { name: /annuler/i }),
    ).toBeNull();
  });

  it("names the real phase in the detail during preflight (F5/AC1)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "transferring", progress: null, phase: "preflight" }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^transfert$/i });
    expect(
      within(region).getByText(/vérification de l'appareil/i),
    ).toBeInTheDocument();
  });

  it("caps the determinate bar at 99 % while transferring — 100 % is reserved for the terminal (F6)", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "transferring", progress: 0.999, phase: "transfer" }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^transfert$/i });
    expect(
      within(region).getByText(/avancement\s*:\s*99\s*%/i),
    ).toBeInTheDocument();
    expect(within(region).queryByText(/100\s*%/)).toBeNull();
  });

  it("renders a transport error in-context with Réessayer", async () => {
    const onRetryTransfer = vi.fn();
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{
          kind: "error",
          error: {
            code: "TRANSFER_FAILED",
            message: "Envoi indisponible: réponse invalide.",
            userAction: "Réessaie l'envoi.",
            details: null,
          },
        }}
        onRetryTransfer={onRetryTransfer}
        onEdit={noop}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      /envoi indisponible: réponse invalide/i,
    );
    await userEvent.click(
      screen.getByRole("button", { name: /réessayer le transfert/i }),
    );
    expect(onRetryTransfer).toHaveBeenCalledTimes(1);
  });

  it("hosts the transfer section in a polite live region", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "transferring", progress: null, phase: null }}
        onEdit={noop}
      />,
    );
    const region = screen.getByRole("region", { name: /^transfert$/i });
    expect(region).toHaveAttribute("aria-live", "polite");
  });

  it("does not duplicate the send CTA: with a transfer prop the device region drops its fallback CTA", () => {
    render(
      <LuniiDecisionPanel
        deviceState="idle"
        transfer={{ kind: "ready" }}
        onSend={noop}
        onEdit={noop}
      />,
    );
    // Exactly one "Envoyer vers la Lunii" button — owned by the Transfert region.
    expect(
      screen.getAllByRole("button", { name: /envoyer vers la lunii/i }),
    ).toHaveLength(1);
  });

  it("does not render the transfer section when the prop is omitted", () => {
    render(<LuniiDecisionPanel deviceState="idle" onEdit={noop} />);
    expect(
      screen.queryByRole("region", { name: /^transfert$/i }),
    ).toBeNull();
  });
});
