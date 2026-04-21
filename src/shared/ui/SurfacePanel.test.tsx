import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SurfacePanel } from "./SurfacePanel";

describe("<SurfacePanel />", () => {
  it("renders as a <section> by default", () => {
    render(<SurfacePanel>contenu</SurfacePanel>);
    const el = screen.getByText("contenu");
    expect(el.tagName).toBe("SECTION");
  });

  it("honors the `as` prop for semantic tags", () => {
    render(<SurfacePanel as="aside">panneau</SurfacePanel>);
    const el = screen.getByText("panneau");
    expect(el.tagName).toBe("ASIDE");
  });

  it("applies the elevation class", () => {
    render(<SurfacePanel elevation={2}>elevation</SurfacePanel>);
    expect(screen.getByText("elevation")).toHaveClass(
      "ds-surface--elevation-2",
    );
  });

  it("forwards aria-labelledby", () => {
    render(
      <SurfacePanel ariaLabelledBy="titre-id">
        <h2 id="titre-id">Titre</h2>
      </SurfacePanel>,
    );
    const region = screen.getByText("Titre").parentElement;
    expect(region).toHaveAttribute("aria-labelledby", "titre-id");
  });
});
