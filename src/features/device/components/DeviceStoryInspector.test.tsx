import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DeviceStoryInspector } from "./DeviceStoryInspector";

const baseStory = {
  uuid: "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
  shortId: "A4B5C6D7",
  hidden: false,
  contentPresent: true,
};

const importableOps = {
  readLibrary: true,
  inspectStory: true,
  importStory: true,
  writeStory: false,
};

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

  it("offers the import affordance disabled with a 'not yet wired' reason when import is allowed", () => {
    render(
      <DeviceStoryInspector story={baseStory} supportedOperations={importableOps} />,
    );
    const button = screen.getByRole("button", {
      name: /copier dans ma bibliothèque/i,
    });
    expect(button).toHaveAttribute("aria-disabled", "true");
    const reason = document.getElementById(
      button.getAttribute("aria-describedby") as string,
    );
    expect(reason).toHaveTextContent(/pas encore activée/i);
  });

  it("phrases the disabled import reason as 'profil non supporté' when import is gated off (V3)", () => {
    render(
      <DeviceStoryInspector
        story={baseStory}
        supportedOperations={{ ...importableOps, importStory: false }}
      />,
    );
    const button = screen.getByRole("button", {
      name: /copier dans ma bibliothèque/i,
    });
    const reason = document.getElementById(
      button.getAttribute("aria-describedby") as string,
    );
    expect(reason).toHaveTextContent(/profil non supporté/i);
  });

  it("defaults to 'profil non supporté' (fail-closed) when the operations matrix is absent", () => {
    render(<DeviceStoryInspector story={baseStory} />);
    const button = screen.getByRole("button", {
      name: /copier dans ma bibliothèque/i,
    });
    const reason = document.getElementById(
      button.getAttribute("aria-describedby") as string,
    );
    // Without a known matrix we must NOT imply the profile supports the copy.
    expect(reason).toHaveTextContent(/profil non supporté/i);
    expect(reason).not.toHaveTextContent(/pas encore activée/i);
  });
});
