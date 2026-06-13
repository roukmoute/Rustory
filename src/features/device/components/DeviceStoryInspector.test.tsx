import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AppError } from "../../../shared/errors/app-error";
import { DeviceStoryInspector } from "./DeviceStoryInspector";

const baseStory = {
  uuid: "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
  shortId: "A4B5C6D7",
  hidden: false,
  contentPresent: true,
  alreadyImported: false,
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
});
