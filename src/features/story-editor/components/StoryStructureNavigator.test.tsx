import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { NodeContentDto } from "../../../shared/ipc-contracts/story";

import { StoryStructureNavigator } from "./StoryStructureNavigator";

const NODE: NodeContentDto = {
  id: "n1",
  text: "",
  label: "",
  image: null,
  audio: null,
};

describe("<StoryStructureNavigator />", () => {
  it("shows the story root and the projected current node", () => {
    render(
      <StoryStructureNavigator
        title="Le soleil couchant"
        node={NODE}
        currentNodeId="n1"
      />,
    );
    expect(
      screen.getByRole("region", { name: "Structure de l'histoire" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Le soleil couchant", {
        selector: ".story-structure-navigator__root-label",
      }),
    ).toBeInTheDocument();
    // The node falls back to "Nœud courant" when its label is empty.
    expect(screen.getByText("Nœud courant")).toBeInTheDocument();
  });

  it("clearly marks the current node (AC3)", () => {
    const { container } = render(
      <StoryStructureNavigator title="Histoire" node={NODE} currentNodeId="n1" />,
    );
    const current = container.querySelector(
      ".story-structure-navigator__node--current",
    );
    expect(current).not.toBeNull();
    expect(current).toHaveAttribute("aria-current", "true");
    expect(current).toHaveTextContent("en cours d'édition");
  });

  it("uses the node's own label when it has one", () => {
    render(
      <StoryStructureNavigator
        title="Histoire"
        node={{ ...NODE, label: "Le départ" }}
        currentNodeId="n1"
      />,
    );
    expect(screen.getByText("Le départ")).toBeInTheDocument();
  });

  it("degrades to a NAMED state when no node is projected (never a crash)", () => {
    render(
      <StoryStructureNavigator title="Histoire" node={null} currentNodeId={null} />,
    );
    const degraded = screen.getByText("Structure illisible.");
    expect(degraded).toBeInTheDocument();
    // The degraded state stays a keyboard focus stop.
    expect(degraded).toHaveAttribute("tabindex", "0");
  });

  it("keeps the structure root focusable (AC3)", () => {
    render(
      <StoryStructureNavigator title="Histoire" node={NODE} currentNodeId="n1" />,
    );
    const root = screen
      .getByText("Histoire", {
        selector: ".story-structure-navigator__root-label",
      })
      .closest(".story-structure-navigator__root");
    expect(root).toHaveAttribute("tabindex", "0");
  });
});
