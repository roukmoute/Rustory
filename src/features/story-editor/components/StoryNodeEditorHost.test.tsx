import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StoryNodeEditorHost } from "./StoryNodeEditorHost";

describe("<StoryNodeEditorHost />", () => {
  it("renders the named current-node zone", () => {
    render(<StoryNodeEditorHost />);
    expect(
      screen.getByRole("region", { name: "Nœud courant" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Nœud courant" }),
    ).toBeInTheDocument();
  });

  it("names the v1 empty state instead of hiding it (UX-DR38)", () => {
    render(<StoryNodeEditorHost />);
    expect(
      screen.getByText("Aucun nœud à éditer pour l'instant."),
    ).toBeInTheDocument();
  });

  it("exposes the zone as a keyboard focus stop", () => {
    render(<StoryNodeEditorHost />);
    const empty = screen.getByText("Aucun nœud à éditer pour l'instant.");
    expect(empty).toHaveAttribute("tabindex", "0");
    empty.focus();
    expect(empty).toHaveFocus();
  });
});
