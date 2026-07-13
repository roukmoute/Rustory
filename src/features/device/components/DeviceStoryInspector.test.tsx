import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

vi.mock("../hooks/use-pack-cover", () => ({
  usePackCover: (_uuid: string, hasCover: boolean) =>
    hasCover ? "data:image/png;base64,COVER" : null,
}));

import type { AppError } from "../../../shared/errors/app-error";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";
import { DeviceStoryInspector } from "./DeviceStoryInspector";

const baseStory: DeviceStoryDto = {
  uuid: "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
  shortId: "A4B5C6D7",
  hidden: false,
  contentPresent: true,
  alreadyImported: false,
  title: null,
  titleSource: null,
  thumbnail: null,
};

const importableOps = {
  readLibrary: true,
  inspectStory: true,
  importStory: true,
  writeStory: false,
};

const importError: AppError = {
  code: "IMPORT_FAILED",
  message: "Copie impossible: lecture de l'appareil interrompue.",
  userAction: "Vérifie la connexion de la Lunii puis réessaie la copie.",
  details: { source: "fs_read" },
};

const profileRefusalError: AppError = {
  code: "DEVICE_UNSUPPORTED",
  message: "Opération non autorisée pour ce profil d'appareil.",
  userAction: "Consulte le profil de support pour comprendre ce qui est permis.",
  details: { source: "capability_gate", operation: "import_story" },
};

function copyButton(): HTMLElement {
  return screen.getByRole("button", {
    name: /copier dans ma bibliothèque/i,
  });
}

describe("<DeviceStoryInspector />", () => {
  it("renders nothing when no story is selected", () => {
    const { container } = render(<DeviceStoryInspector story={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows only verified facts and makes the provenance explicit (AC1)", () => {
    render(
      <DeviceStoryInspector story={baseStory} supportedOperations={importableOps} />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    // Provenance: lives on the device, not yet local.
    expect(
      within(region).getByText(/pas encore dans ta bibliothèque locale/i),
    ).toBeInTheDocument();
    expect(
      within(region).getAllByText(/sur l'appareil/i).length,
    ).toBeGreaterThan(0);
    // Honest identity — no title, opaque ids only.
    expect(
      within(region).getByText(/histoire non reconnue/i),
    ).toBeInTheDocument();
    expect(within(region).getByText("A4B5C6D7")).toBeInTheDocument();
    expect(within(region).getByText(baseStory.uuid)).toBeInTheDocument();
  });

  it("never invents a title nor an asserted content quality (AC2)", () => {
    render(<DeviceStoryInspector story={baseStory} />);
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    // The forbidden, over-asserting vocabulary must never appear.
    expect(region.textContent ?? "").not.toMatch(
      /corrompue|cassée|orpheline|corrupt|broken/i,
    );
  });

  it("signals an incomplete-content ambiguity honestly, never as 'corrupt' (AC2)", () => {
    render(
      <DeviceStoryInspector story={{ ...baseStory, contentPresent: false }} />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(
      within(region).getByText(/contenu incomplet/i),
    ).toBeInTheDocument();
    expect(
      within(region).getByText(/dossier de contenu .* introuvable/i),
    ).toBeInTheDocument();
  });

  it("activates the copy CTA and fires onImport when the gate allows it", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onImport={onImport}
      />,
    );
    const button = copyButton();
    expect(button).not.toHaveAttribute("aria-disabled");
    expect(button).not.toHaveAttribute("aria-describedby");
    await user.click(button);
    expect(onImport).toHaveBeenCalledTimes(1);
    expect(onImport).toHaveBeenCalledWith(baseStory);
    // The retired Phase-1 wording must be gone everywhere.
    expect(screen.queryByText(/pas encore activée/i)).not.toBeInTheDocument();
  });

  it("soft-disables the CTA with aria-busy and calm progress while the copy runs", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        importState={{ kind: "importing" }}
        onImport={onImport}
      />,
    );
    const button = copyButton();
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Copie en cours…")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toBeInTheDocument();
    // Soft-disabled: focusable, but activation is swallowed.
    await user.click(button);
    expect(onImport).not.toHaveBeenCalled();
  });

  it("rewrites the provenance note once a local copy exists — never 'pas encore' on a copied story", () => {
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, alreadyImported: true }}
        supportedOperations={importableOps}
      />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(
      within(region).getByText(
        /vit sur l'appareil et une copie existe déjà dans ta bibliothèque locale/i,
      ),
    ).toBeInTheDocument();
    expect(
      within(region).queryByText(/pas encore dans ta bibliothèque locale/i),
    ).not.toBeInTheDocument();
  });

  it("soft-disables the CTA in the imported state so a re-click cannot relaunch the copy", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onImport={onImport}
        importState={{
          kind: "imported",
          story: { id: "0197a5d0-0000-7000-8000-000000000000", title: "T" },
          packShortId: "A4B5C6D7",
        }}
      />,
    );
    const button = copyButton();
    // The device snapshot still says alreadyImported=false (the re-read
    // has not landed yet) — the imported STATUS alone must disable it.
    expect(button).toHaveAttribute("aria-disabled", "true");
    await user.click(button);
    expect(onImport).not.toHaveBeenCalled();
  });

  it("prioritizes the 'déjà dans ta bibliothèque' reason over every other cause", () => {
    const onImport = vi.fn();
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, alreadyImported: true, contentPresent: false }}
        supportedOperations={{ ...importableOps, importStory: false }}
        onImport={onImport}
      />,
    );
    const button = copyButton();
    expect(button).toHaveAttribute("aria-disabled", "true");
    const reason = document.getElementById(
      button.getAttribute("aria-describedby") as string,
    );
    expect(reason).toHaveTextContent(/déjà dans ta bibliothèque/i);
    // The inspector also surfaces the local-copy marker chip.
    expect(screen.getByText("Dans ta bibliothèque")).toBeInTheDocument();
  });

  it("drives a FLAM story exactly like a Lunii one: CTA active on the FLAM matrix, incomplete content refused", async () => {
    // `importableOps` IS the FLAM Gen1 matrix line (read ✅✅✅, write ❌):
    // a FLAM inventory entry activates the copy CTA through the same
    // gate, and its incomplete-content refusal keeps the same reason.
    const flamStory: DeviceStoryDto = {
      ...baseStory,
      uuid: "12345678-9abc-def0-1122-334455667788",
      shortId: "55667788",
    };
    const onImport = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(
      <DeviceStoryInspector
        story={flamStory}
        supportedOperations={importableOps}
        onImport={onImport}
      />,
    );
    await user.click(copyButton());
    expect(onImport).toHaveBeenCalledWith(flamStory);

    rerender(
      <DeviceStoryInspector
        story={{ ...flamStory, contentPresent: false }}
        supportedOperations={importableOps}
        onImport={onImport}
      />,
    );
    const disabled = copyButton();
    expect(disabled).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByText(/copie indisponible: contenu incomplet sur l'appareil/i),
    ).toBeInTheDocument();
  });

  it("phrases the disabled reason as 'profil non supporté' when import is gated off (V3)", () => {
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={{ ...importableOps, importStory: false }}
        onImport={vi.fn()}
      />,
    );
    const reason = document.getElementById(
      copyButton().getAttribute("aria-describedby") as string,
    );
    expect(reason).toHaveTextContent(/profil non supporté/i);
  });

  it("defaults to 'profil non supporté' (fail-closed) when the operations matrix is absent", () => {
    render(<DeviceStoryInspector story={baseStory} />);
    const reason = document.getElementById(
      copyButton().getAttribute("aria-describedby") as string,
    );
    // Without a known matrix we must NOT imply the profile supports the copy.
    expect(reason).toHaveTextContent(/profil non supporté/i);
    expect(reason).not.toHaveTextContent(/pas encore activée/i);
  });

  it("phrases the disabled reason as 'contenu incomplet' when only the payload is missing", () => {
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, contentPresent: false }}
        supportedOperations={importableOps}
        onImport={vi.fn()}
      />,
    );
    const reason = document.getElementById(
      copyButton().getAttribute("aria-describedby") as string,
    );
    expect(reason).toHaveTextContent(/contenu incomplet sur l'appareil/i);
  });

  it("announces the sober success politely with the created title and an explicit dismiss", async () => {
    const user = userEvent.setup();
    const onDismiss = vi.fn();
    const { container } = render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onImport={vi.fn()}
        importState={{
          kind: "imported",
          story: { id: "local-1", title: "Histoire de ma Lunii (A4B5C6D7)" },
          packShortId: "A4B5C6D7",
        }}
        onDismissImportStatus={onDismiss}
      />,
    );
    const live = container.querySelector('[aria-live="polite"]');
    expect(live).toHaveTextContent("Histoire copiée dans ta bibliothèque");
    expect(
      screen.getByText("Histoire de ma Lunii (A4B5C6D7)"),
    ).toBeInTheDocument();
    // No alert on the success path; dismiss is explicit, never automatic.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /fermer/i }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  function supportAffordance(): HTMLElement | null {
    return screen.queryByRole("button", {
      name: /consulter le profil de support officiel/i,
    });
  }

  it("offers 'Consulter le profil de support' on a profile refusal and fires it (AC1)", async () => {
    const user = userEvent.setup();
    const onConsult = vi.fn();
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={{ ...importableOps, importStory: false }}
        onImport={vi.fn()}
        onConsultSupportProfile={onConsult}
      />,
    );
    // The canonical disabled reason is unchanged…
    const reason = document.getElementById(
      copyButton().getAttribute("aria-describedby") as string,
    );
    expect(reason).toHaveTextContent(/profil non supporté/i);
    // …and a next gesture is offered instead of an opaque grayed-out CTA.
    const consult = supportAffordance();
    expect(consult).toBeInTheDocument();
    await user.click(consult as HTMLElement);
    expect(onConsult).toHaveBeenCalledTimes(1);
  });

  it("does NOT offer the support affordance when the copy is refused as 'déjà dans ta bibliothèque'", () => {
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, alreadyImported: true }}
        supportedOperations={importableOps}
        onImport={vi.fn()}
        onConsultSupportProfile={vi.fn()}
      />,
    );
    expect(supportAffordance()).not.toBeInTheDocument();
  });

  it("does NOT offer the support affordance for an incomplete-content refusal (has its own note)", () => {
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, contentPresent: false }}
        supportedOperations={importableOps}
        onImport={vi.fn()}
        onConsultSupportProfile={vi.fn()}
      />,
    );
    expect(supportAffordance()).not.toBeInTheDocument();
  });

  it("hides the support affordance entirely in inspection-only contexts (no handler wired)", () => {
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={{ ...importableOps, importStory: false }}
        onImport={vi.fn()}
      />,
    );
    expect(supportAffordance()).not.toBeInTheDocument();
  });

  it("does NOT offer the support affordance when only the handler is unwired (profile allows the copy)", () => {
    // importStory === true, content present, not imported, but no onImport:
    // the CTA is fail-closed to 'profil non supporté', yet the profile
    // genuinely allows the copy — consulting support would be misleading.
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onConsultSupportProfile={vi.fn()}
      />,
    );
    const reason = document.getElementById(
      copyButton().getAttribute("aria-describedby") as string,
    );
    expect(reason).toHaveTextContent(/profil non supporté/i);
    expect(supportAffordance()).not.toBeInTheDocument();
  });

  it("offers the support gesture INSTEAD of a futile Réessayer on a runtime profile refusal (AC1)", async () => {
    const user = userEvent.setup();
    const onConsult = vi.fn();
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onImport={vi.fn()}
        importState={{ kind: "failed", error: profileRefusalError }}
        onRetryImport={vi.fn()}
        onConsultSupportProfile={onConsult}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/opération non autorisée/i);
    // A profile refusal is not retryable — Réessayer is replaced.
    expect(
      within(alert).queryByRole("button", { name: /réessayer/i }),
    ).not.toBeInTheDocument();
    const consult = within(alert).getByRole("button", {
      name: /consulter le profil de support officiel/i,
    });
    await user.click(consult);
    expect(onConsult).toHaveBeenCalledTimes(1);
  });

  it("keeps Réessayer (and no support gesture) on a non-profile copy failure", () => {
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onImport={vi.fn()}
        importState={{ kind: "failed", error: importError }}
        onRetryImport={vi.fn()}
        onConsultSupportProfile={vi.fn()}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(
      within(alert).getByRole("button", { name: /réessayer/i }),
    ).toBeInTheDocument();
    expect(
      within(alert).queryByRole("button", {
        name: /consulter le profil de support officiel/i,
      }),
    ).not.toBeInTheDocument();
  });

  it("renders a SINGLE support gesture when a runtime profile refusal coincides with a V3 reclassification", async () => {
    const user = userEvent.setup();
    const onConsult = vi.fn();
    // The device turned out non-importable at runtime (failed
    // DEVICE_UNSUPPORTED) AND the snapshot reclassified to V3
    // (importStory=false) → the pre-click affordance must stay suppressed
    // so only the failure surface carries the next gesture. No duplicate
    // button with the same accessible name in the region.
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={{ ...importableOps, importStory: false }}
        onImport={vi.fn()}
        importState={{ kind: "failed", error: profileRefusalError }}
        onRetryImport={vi.fn()}
        onConsultSupportProfile={onConsult}
      />,
    );
    const consultButtons = screen.getAllByRole("button", {
      name: /consulter le profil de support officiel/i,
    });
    expect(consultButtons).toHaveLength(1);
    // The single gesture lives in the failure alert, not the pre-click body.
    const alert = screen.getByRole("alert");
    expect(
      within(alert).getByRole("button", {
        name: /consulter le profil de support officiel/i,
      }),
    ).toBe(consultButtons[0]);
    await user.click(consultButtons[0]);
    expect(onConsult).toHaveBeenCalledTimes(1);
  });

  it("never shows the support affordance when the copy is allowed", () => {
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onImport={vi.fn()}
        onConsultSupportProfile={vi.fn()}
      />,
    );
    expect(supportAffordance()).not.toBeInTheDocument();
  });

  it("groups a fully-copyable story under 'Ce que Rustory reconnaît' only (AC2)", () => {
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onImport={vi.fn()}
      />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(
      within(region).getByText(/ce que rustory reconnaît/i),
    ).toBeInTheDocument();
    expect(within(region).getByText("Contenu présent")).toBeInTheDocument();
    // No blocker, nothing to review → those headers stay absent.
    expect(
      within(region).queryByText(/ce qui bloque la copie/i),
    ).not.toBeInTheDocument();
    expect(
      within(region).queryByText(/à revoir avant de copier/i),
    ).not.toBeInTheDocument();
    // Anti-catalog: the only name shown is the honest placeholder.
    expect(
      within(region).getByText(/histoire non reconnue/i),
    ).toBeInTheDocument();
  });

  it("classifies incomplete content as blocking, never as 'Contenu présent' (fail-closed, AC2)", () => {
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, contentPresent: false }}
        supportedOperations={importableOps}
        onImport={vi.fn()}
      />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(
      within(region).getByText(/ce qui bloque la copie/i),
    ).toBeInTheDocument();
    expect(within(region).getByText("Contenu incomplet")).toBeInTheDocument();
    expect(within(region).queryByText("Contenu présent")).not.toBeInTheDocument();
  });

  it("classifies an already-copied story as blocking under 'Ce qui bloque la copie' (AC2)", () => {
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, alreadyImported: true }}
        supportedOperations={importableOps}
        onImport={vi.fn()}
      />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(
      within(region).getByText(/ce qui bloque la copie/i),
    ).toBeInTheDocument();
    expect(within(region).getByText("Dans ta bibliothèque")).toBeInTheDocument();
  });

  it("classifies a hidden story under 'À revoir avant de copier' (AC2)", () => {
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, hidden: true }}
        supportedOperations={importableOps}
        onImport={vi.fn()}
      />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(
      within(region).getByText(/à revoir avant de copier/i),
    ).toBeInTheDocument();
    expect(within(region).getByText("Masquée")).toBeInTheDocument();
    // Hidden alone does not block: content is present, so no blocker group.
    expect(
      within(region).queryByText(/ce qui bloque la copie/i),
    ).not.toBeInTheDocument();
  });

  it("renders the failure as an in-context alert with Réessayer before Fermer", async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={importableOps}
        onImport={vi.fn()}
        importState={{ kind: "failed", error: importError }}
        onRetryImport={onRetry}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Copie impossible");
    expect(alert).toHaveTextContent(importError.message);
    expect(alert).toHaveTextContent(importError.userAction as string);
    // Keyboard order: Réessayer is reachable BEFORE Fermer.
    const buttons = within(alert).getAllByRole("button");
    expect(buttons[0]).toHaveTextContent(/réessayer/i);
    expect(buttons[1]).toHaveTextContent(/fermer/i);
    await user.click(buttons[0]);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  // --- Title recognition + naming (story 2.6 — AC1, AC2) ---

  const officialStory: DeviceStoryDto = {
    ...baseStory,
    title: "Suzanne et Gaston",
    titleSource: "official",
  };

  it("shows the recognized title + provenance instead of 'non reconnue' (AC1)", () => {
    render(
      <DeviceStoryInspector
        story={officialStory}
        supportedOperations={importableOps}
      />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(within(region).getByText("Suzanne et Gaston")).toBeInTheDocument();
    expect(within(region).getByText("Titre officiel")).toBeInTheDocument();
    expect(
      within(region).queryByText(/histoire non reconnue/i),
    ).not.toBeInTheDocument();
  });

  it("renders the cached cover when the recognized pack has one", () => {
    const { container } = render(
      <DeviceStoryInspector
        story={{ ...officialStory, thumbnail: "cover.png" }}
        supportedOperations={importableOps}
      />,
    );
    const cover = container.querySelector<HTMLImageElement>(
      ".device-inspector__cover",
    );
    expect(cover).not.toBeNull();
    expect(cover?.src).toContain("data:image/png;base64,COVER");
  });

  it("never labels a user-typed title as official (honesty, AC1)", () => {
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, title: "Mon titre", titleSource: "user" }}
        onSetTitle={vi.fn()}
      />,
    );
    const region = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(within(region).getByText("Titre saisi")).toBeInTheDocument();
    expect(within(region).queryByText("Titre officiel")).not.toBeInTheDocument();
  });

  it("offers 'Nommer cette histoire' for an unrecognized pack and saves the typed title (AC2)", async () => {
    const user = userEvent.setup();
    const onSetTitle = vi.fn().mockResolvedValue(true);
    render(<DeviceStoryInspector story={baseStory} onSetTitle={onSetTitle} />);

    await user.click(
      screen.getByRole("button", { name: /nommer cette histoire/i }),
    );
    const input = screen.getByLabelText(/titre de l'histoire/i);
    await user.type(input, "Le château de cartes");
    await user.click(screen.getByRole("button", { name: /enregistrer/i }));

    expect(onSetTitle).toHaveBeenCalledWith(
      baseStory.uuid,
      "Le château de cartes",
    );
  });

  it("offers 'Renommer' for a user title and prefills the editor with it (AC2)", async () => {
    const user = userEvent.setup();
    render(
      <DeviceStoryInspector
        story={{ ...baseStory, title: "Ancien nom", titleSource: "user" }}
        onSetTitle={vi.fn().mockResolvedValue(true)}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: /renommer cette histoire/i }),
    );
    const input = screen.getByLabelText<HTMLInputElement>(
      /titre de l'histoire/i,
    );
    expect(input.value).toBe("Ancien nom");
  });

  it("does NOT offer naming for an official title (not the user's to overwrite here)", () => {
    render(
      <DeviceStoryInspector story={officialStory} onSetTitle={vi.fn()} />,
    );
    expect(
      screen.queryByRole("button", { name: /nommer cette histoire/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /renommer cette histoire/i }),
    ).not.toBeInTheDocument();
  });

  it("hides the naming affordance entirely when no handler is wired", () => {
    render(<DeviceStoryInspector story={baseStory} />);
    expect(
      screen.queryByRole("button", { name: /nommer cette histoire/i }),
    ).not.toBeInTheDocument();
  });

  it("does not submit a locally-invalid title on Enter (no opaque UNKNOWN)", async () => {
    const user = userEvent.setup();
    const onSetTitle = vi.fn().mockResolvedValue(true);
    render(<DeviceStoryInspector story={baseStory} onSetTitle={onSetTitle} />);
    await user.click(
      screen.getByRole("button", { name: /nommer cette histoire/i }),
    );
    const input = screen.getByLabelText(/titre de l'histoire/i);
    await user.type(input, "a".repeat(121)); // > 120 → locally invalid
    await user.keyboard("{Enter}");
    await user.click(screen.getByRole("button", { name: /enregistrer/i }));
    // The invalid title is never sent — the inline reason guards it.
    expect(onSetTitle).not.toHaveBeenCalled();
  });

  it("clears a stale naming error when the user reopens the editor (AC2)", async () => {
    const user = userEvent.setup();
    const onDismissTitleError = vi.fn();
    const titleError: AppError = {
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre trop long.",
      userAction: "Raccourcis le titre.",
      details: null,
    };
    render(
      <DeviceStoryInspector
        story={baseStory}
        onSetTitle={vi.fn().mockResolvedValue(false)}
        titleState={{ kind: "failed", error: titleError }}
        onDismissTitleError={onDismissTitleError}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: /nommer cette histoire/i }),
    );
    expect(onDismissTitleError).toHaveBeenCalled();
  });

  it("surfaces a naming failure in-context as an alert (e.g. a rejected title)", async () => {
    const user = userEvent.setup();
    const titleError: AppError = {
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre trop long (120 caractères maximum).",
      userAction: "Raccourcis le titre à 120 caractères maximum.",
      details: null,
    };
    render(
      <DeviceStoryInspector
        story={baseStory}
        onSetTitle={vi.fn().mockResolvedValue(false)}
        titleState={{ kind: "failed", error: titleError }}
      />,
    );
    // Open the editor so the in-context error region renders.
    await user.click(
      screen.getByRole("button", { name: /nommer cette histoire/i }),
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/titre trop long/i);
  });
});
