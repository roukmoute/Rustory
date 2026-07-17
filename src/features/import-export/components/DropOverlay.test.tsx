import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { useDropShell } from "../../../shell/state/drop-shell-store";
import { DropOverlay } from "./DropOverlay";

describe("<DropOverlay />", () => {
  beforeEach(() => {
    useDropShell.setState({ hoverActive: false, pendingSignal: false });
  });

  it("renders nothing while no drag hovers the window", () => {
    const { container } = render(<DropOverlay />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the frozen copy verbatim while a drag hovers", () => {
    useDropShell.setState({ hoverActive: true });
    const { container } = render(<DropOverlay />);
    expect(container.textContent).toBe(
      "Dépose ton fichier ou ton dossier pour l'analyser",
    );
  });

  it("closes when the hover ends (hover-ended or requested both clear the flag)", () => {
    useDropShell.setState({ hoverActive: true });
    const { container } = render(<DropOverlay />);
    expect(container.textContent).not.toBe("");
    // Both the `drop:hover-ended` and `drop:requested` bootstrap paths
    // funnel into clearHover — the overlay closes either way (idempotent).
    act(() => {
      useDropShell.getState().clearHover();
    });
    expect(container).toBeEmptyDOMElement();
  });

  it("is decorative: aria-hidden, no focusable element, no live region", () => {
    useDropShell.setState({ hoverActive: true });
    const { container } = render(<DropOverlay />);
    const overlay = container.firstElementChild as HTMLElement;
    expect(overlay).toHaveAttribute("aria-hidden", "true");
    // A drag gesture in progress has nothing to announce and nothing to
    // focus — the verdicts speak through the library's live regions.
    expect(
      overlay.querySelectorAll("button, a, input, [tabindex]"),
    ).toHaveLength(0);
    expect(overlay.querySelectorAll("[role], [aria-live]")).toHaveLength(0);
  });

  it("steals no focus from the active element", () => {
    const button = document.createElement("button");
    document.body.appendChild(button);
    button.focus();
    useDropShell.setState({ hoverActive: true });
    render(<DropOverlay />);
    expect(document.activeElement).toBe(button);
    button.remove();
  });
});
