import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { StoryStructure } from "../../../shared/ipc-contracts/story";

import { StoryStructureNavigator } from "./StoryStructureNavigator";

const TWO_NODE_STRUCTURE: StoryStructure = {
  startNodeId: "n1",
  nodes: [
    {
      id: "n1",
      label: "Le départ",
      isStart: true,
      hasIssue: false,
      options: [{ label: "Continuer", target: "n2", state: "linked" }],
    },
    {
      id: "n2",
      label: "",
      isStart: false,
      hasIssue: false,
      options: [],
    },
  ],
};

function renderNavigator(
  overrides: Partial<
    React.ComponentProps<typeof StoryStructureNavigator>
  > = {},
) {
  const props: React.ComponentProps<typeof StoryStructureNavigator> = {
    title: "Le soleil couchant",
    structure: TWO_NODE_STRUCTURE,
    currentNodeId: "n1",
    editable: true,
    busy: false,
    nodeError: null,
    globalError: null,
    onSelectNode: vi.fn(),
    onAddNode: vi.fn(),
    onMoveNode: vi.fn(),
    onDeleteNode: vi.fn(),
    ...overrides,
  };
  return { ...render(<StoryStructureNavigator {...props} />), props };
}

describe("<StoryStructureNavigator />", () => {
  it("shows the story root and the ORDERED node list with a textual Départ mark", () => {
    const { container } = renderNavigator();
    expect(
      screen.getByRole("region", { name: "Structure de l'histoire" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Le soleil couchant", {
        selector: ".story-structure-navigator__root-label",
      }),
    ).toBeInTheDocument();
    const entries = container.querySelectorAll(
      ".story-structure-navigator__node",
    );
    // Order = canonical nodes[] order; the label falls back to the stable id.
    expect(entries).toHaveLength(2);
    expect(entries[0]).toHaveTextContent("Le départ");
    expect(entries[0]).toHaveTextContent("— Départ");
    expect(entries[1]).toHaveTextContent("n2");
    expect(entries[1]).not.toHaveTextContent("— Départ");
    // Options summary is a named state, never a blank.
    expect(entries[0]).toHaveTextContent("1 option");
    expect(entries[1]).toHaveTextContent("Aucune option");
  });

  it("clearly marks the current node (AC3)", () => {
    const { container } = renderNavigator();
    const current = container.querySelector(
      ".story-structure-navigator__node--current",
    );
    expect(current).not.toBeNull();
    expect(current).toHaveAttribute("aria-current", "true");
    expect(current).toHaveTextContent("en cours d'édition");
  });

  it("selects a node on click and moves the roving focus with arrow keys", () => {
    const { container, props } = renderNavigator();
    const entries = container.querySelectorAll<HTMLButtonElement>(
      ".story-structure-navigator__node",
    );
    const [first, second] = [entries[0], entries[1]];
    // Roving tabindex: exactly one entry is tabbable.
    expect(first).toHaveAttribute("tabindex", "0");
    expect(second).toHaveAttribute("tabindex", "-1");

    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(second).toHaveFocus();
    expect(second).toHaveAttribute("tabindex", "0");
    expect(first).toHaveAttribute("tabindex", "-1");

    fireEvent.keyDown(second, { key: "ArrowUp" });
    expect(first).toHaveFocus();

    fireEvent.click(second);
    expect(props.onSelectNode).toHaveBeenCalledWith("n2");
  });

  it("flags a localized issue with glyph + text without hiding the rest (AC1)", () => {
    renderNavigator({
      structure: {
        startNodeId: "n1",
        nodes: [
          {
            id: "n1",
            label: "Départ",
            isStart: true,
            hasIssue: true,
            options: [{ label: "Perdu", target: "ghost", state: "broken" }],
          },
          {
            id: "n2",
            label: "Sain",
            isStart: false,
            hasIssue: false,
            options: [],
          },
        ],
      },
    });
    // Localized textual mark on the flagged node…
    expect(screen.getByText("à corriger")).toBeInTheDocument();
    // …and the rest of the list stays visible and active.
    expect(
      screen.getByText("Sain", {
        selector: ".story-structure-navigator__node-label",
      }),
    ).toBeInTheDocument();
  });

  it("requires TWO gestures to delete a node, with the impact named inline", () => {
    const { props } = renderNavigator();
    fireEvent.click(
      screen.getByRole("button", { name: "Supprimer le nœud — n2" }),
    );
    // First gesture: no deletion yet, an inline confirmation appears.
    expect(props.onDeleteNode).not.toHaveBeenCalled();
    expect(
      screen.getByText(/Le nœud et ses médias seront supprimés/),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Confirmer la suppression" }),
    );
    expect(props.onDeleteNode).toHaveBeenCalledWith("n2");
  });

  it("cancels the delete confirmation with Annuler", () => {
    const { props } = renderNavigator();
    fireEvent.click(
      screen.getByRole("button", { name: "Supprimer le nœud — n2" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Annuler" }));
    expect(props.onDeleteNode).not.toHaveBeenCalled();
    expect(
      screen.queryByText(/Le nœud et ses médias seront supprimés/),
    ).not.toBeInTheDocument();
  });

  it("hands the keyboard focus through the two-step delete (never body)", () => {
    renderNavigator();
    // First gesture: the trigger unmounts, the focus lands on the FIRST
    // button of the incoming confirmation block.
    fireEvent.click(
      screen.getByRole("button", { name: "Supprimer le nœud — n2" }),
    );
    expect(
      screen.getByRole("button", { name: "Confirmer la suppression" }),
    ).toHaveFocus();

    // Cancel: the focus returns to the logical trigger.
    fireEvent.click(screen.getByRole("button", { name: "Annuler" }));
    expect(
      screen.getByRole("button", { name: "Supprimer le nœud — n2" }),
    ).toHaveFocus();
  });

  it("focuses the re-clamped entry after a confirmed deletion", () => {
    const { container, rerender, props } = renderNavigator();
    fireEvent.click(
      screen.getByRole("button", { name: "Supprimer le nœud — n2" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Confirmer la suppression" }),
    );
    expect(props.onDeleteNode).toHaveBeenCalledWith("n2");

    // The ACK re-projects a list without n2: the focus must land on the
    // entry at the re-clamped index, keeping the keyboard user in the list.
    rerender(
      <StoryStructureNavigator
        {...props}
        structure={{
          startNodeId: "n1",
          nodes: [
            {
              id: "n1",
              label: "Le départ",
              isStart: true,
              hasIssue: false,
              options: [
                { label: "Continuer", target: "n2", state: "broken" },
              ],
            },
          ],
        }}
      />,
    );
    const entries = container.querySelectorAll<HTMLButtonElement>(
      ".story-structure-navigator__node",
    );
    expect(entries).toHaveLength(1);
    expect(entries[0]).toHaveFocus();
  });

  it("never offers deleting the start node and bounds the move actions", () => {
    renderNavigator();
    // The start node's delete stays disabled (the entry point must exist).
    expect(
      screen.getByRole("button", { name: "Supprimer le nœud — Le départ" }),
    ).toBeDisabled();
    // First node cannot move up, last node cannot move down.
    expect(
      screen.getByRole("button", { name: "Monter — Le départ" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Descendre — n2" }),
    ).toBeDisabled();
  });

  it("moves a node with the explicit actions", () => {
    const { props } = renderNavigator();
    fireEvent.click(screen.getByRole("button", { name: "Descendre — Le départ" }));
    expect(props.onMoveNode).toHaveBeenCalledWith("n1", "down");
    fireEvent.click(screen.getByRole("button", { name: "Monter — n2" }));
    expect(props.onMoveNode).toHaveBeenCalledWith("n2", "up");
  });

  it("adds a node from the dedicated action", () => {
    const { props } = renderNavigator();
    fireEvent.click(screen.getByRole("button", { name: "Ajouter un nœud" }));
    expect(props.onAddNode).toHaveBeenCalled();
  });

  it("renders ZERO structural action for an imported story", () => {
    renderNavigator({ editable: false });
    expect(
      screen.queryByRole("button", { name: "Ajouter un nœud" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Supprimer le nœud/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Monter/ }),
    ).not.toBeInTheDocument();
    // Selection stays available (read-only navigation is legitimate).
    expect(
      screen.getByRole("button", { name: /Le départ/ }),
    ).toBeInTheDocument();
  });

  it("surfaces a refused node mutation INLINE at the acted-on entry", () => {
    renderNavigator({
      nodeError: {
        nodeId: "n2",
        error: {
          code: "LIBRARY_INCONSISTENT",
          message: "Le nœud à modifier est introuvable dans l'histoire.",
          userAction: "Recharge l'éditeur puis réessaie.",
          details: null,
        },
      },
    });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      "Le nœud à modifier est introuvable dans l'histoire.",
    );
    expect(alert).toHaveTextContent("Recharge l'éditeur puis réessaie.");
  });

  it("degrades to the NAMED state when the graph is not projected (never a crash)", () => {
    renderNavigator({ structure: null, currentNodeId: null });
    const degraded = screen.getByText("Structure illisible.");
    expect(degraded).toBeInTheDocument();
    // The degraded state stays a keyboard focus stop.
    expect(degraded).toHaveAttribute("tabindex", "0");
    // No structural action over an unprojected graph.
    expect(
      screen.queryByRole("button", { name: "Ajouter un nœud" }),
    ).not.toBeInTheDocument();
  });
});
