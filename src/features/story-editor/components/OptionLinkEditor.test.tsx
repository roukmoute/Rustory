import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { NodeGraph } from "../../../shared/ipc-contracts/story";

import { OptionLinkEditor } from "./OptionLinkEditor";

const NODES: NodeGraph[] = [
  {
    id: "n1",
    label: "Départ",
    isStart: true,
    hasIssue: true,
    options: [
      { label: "Continuer", target: "n2", state: "linked" },
      { label: "Attendre", target: null, state: "unlinked" },
      { label: "Perdu", target: "ghost", state: "broken" },
    ],
  },
  { id: "n2", label: "La suite", isStart: false, hasIssue: false, options: [] },
];

function renderEditor(
  overrides: Partial<React.ComponentProps<typeof OptionLinkEditor>> = {},
) {
  const props: React.ComponentProps<typeof OptionLinkEditor> = {
    node: NODES[0],
    nodes: NODES,
    editable: true,
    busy: false,
    optionError: null,
    onAddOption: vi.fn(),
    onLink: vi.fn(),
    onCreateAndLink: vi.fn(),
    onUnlink: vi.fn(),
    onRemoveOption: vi.fn(),
    ...overrides,
  };
  return { ...render(<OptionLinkEditor {...props} />), props };
}

describe("<OptionLinkEditor />", () => {
  it("renders each option with its Rust-derived state in product language", () => {
    renderEditor();
    expect(screen.getByText("Continuer")).toBeInTheDocument();
    // linked → « liée » + the destination's display name.
    expect(screen.getByText(/liée → La suite/)).toBeInTheDocument();
    // unlinked → « non liée », a normal authoring state.
    expect(screen.getByText("non liée")).toBeInTheDocument();
    // broken → « destination à corriger » — the words « broken » / « lien
    // cassé » NEVER reach the screen.
    expect(screen.getByText(/destination à corriger/)).toBeInTheDocument();
    expect(screen.queryByText(/broken/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/lien cassé/i)).not.toBeInTheDocument();
  });

  it("explains a broken destination inline with cause, impact and next gesture", () => {
    renderEditor();
    // A persistent STATE note carries role="status" (an action error keeps
    // role="alert") — announcing a standing state as an alert on every
    // mount would spam AT users.
    const note = screen
      .getAllByRole("status")
      .find((el) => el.textContent?.includes("n'existe plus"));
    expect(note).toBeDefined();
    expect(note).toHaveTextContent("ne mènera nulle part");
    expect(note).toHaveTextContent("Relie l'option vers un nœud existant");
  });

  it("closes an open link form when the option LIST changes (stale index)", () => {
    // Form open on « Attendre » (index 1); the owner then removes « Continuer »
    // (index 0) and the ACK re-projects a slid list: keeping the form alive
    // would submit against the WRONG option — it must be closed.
    const { rerender, props } = renderEditor();
    fireEvent.click(screen.getByRole("button", { name: "Lier — Attendre" }));
    expect(
      screen.getByRole("combobox", { name: "Destination" }),
    ).toBeInTheDocument();

    const slidNode: NodeGraph = {
      ...NODES[0],
      options: NODES[0].options.slice(1),
    };
    rerender(
      <OptionLinkEditor
        {...props}
        node={slidNode}
        nodes={[slidNode, NODES[1]]}
      />,
    );
    expect(
      screen.queryByRole("combobox", { name: "Destination" }),
    ).not.toBeInTheDocument();
    expect(props.onLink).not.toHaveBeenCalled();
  });

  it("moves the focus into the link form on open and back to the trigger on cancel", () => {
    renderEditor();
    const trigger = screen.getByRole("button", { name: "Lier — Attendre" });
    fireEvent.click(trigger);
    expect(screen.getByRole("combobox", { name: "Destination" })).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Annuler" }));
    expect(
      screen.getByRole("button", { name: "Lier — Attendre" }),
    ).toHaveFocus();
  });

  it("links toward an existing node through the FLAT selector", () => {
    const { props } = renderEditor();
    fireEvent.click(screen.getByRole("button", { name: "Lier — Attendre" }));
    const select = screen.getByRole("combobox", { name: "Destination" });
    // Flat list of the graph's nodes — self-reference included (a loop is a
    // legitimate narrative shape).
    expect(
      screen.getByRole("option", { name: "Départ — Départ" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: "La suite" }),
    ).toBeInTheDocument();
    fireEvent.change(select, { target: { value: "n2" } });
    fireEvent.click(screen.getByRole("button", { name: "Lier" }));
    expect(props.onLink).toHaveBeenCalledWith(1, "n2");
  });

  it("never re-submits a ghost destination from the repair form", () => {
    const { props } = renderEditor();
    // Open the selector on the BROKEN option (target "ghost" is absent from
    // the graph): the form must NOT pre-select the ghost — the <select> has
    // no such option — and `Lier` stays disabled until a REAL node is
    // chosen, so the same phantom target can never be resubmitted.
    fireEvent.click(screen.getByRole("button", { name: "Lier — Perdu" }));
    const select = screen.getByRole("combobox", {
      name: "Destination",
    }) as HTMLSelectElement;
    expect(select.value).toBe("");
    const confirm = screen.getByRole("button", { name: "Lier" });
    expect(confirm).toBeDisabled();

    fireEvent.change(select, { target: { value: "n2" } });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    expect(props.onLink).toHaveBeenCalledWith(2, "n2");
  });

  it("creates and links a new node atomically from the link form", () => {
    const { props } = renderEditor();
    fireEvent.click(screen.getByRole("button", { name: "Lier — Attendre" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Créer et lier un nouveau nœud" }),
    );
    expect(props.onCreateAndLink).toHaveBeenCalledWith(1);
  });

  it("unlinks and removes options with the explicit gestures", () => {
    const { props } = renderEditor();
    fireEvent.click(screen.getByRole("button", { name: "Délier — Continuer" }));
    expect(props.onUnlink).toHaveBeenCalledWith(0);
    // An unlinked option offers no Délier (nothing to unlink).
    expect(
      screen.queryByRole("button", { name: "Délier — Attendre" }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Retirer l'option — Perdu" }),
    );
    expect(props.onRemoveOption).toHaveBeenCalledWith(2);
  });

  it("adds an option with its label typed at creation", () => {
    const { props } = renderEditor();
    const addButton = screen.getByRole("button", { name: "Ajouter une option" });
    // No label yet → the add action stays disabled (the label is typed at
    // creation, never an empty afterthought).
    expect(addButton).toBeDisabled();
    fireEvent.change(
      screen.getByLabelText("Libellé de la nouvelle option"),
      { target: { value: "  Aller au château  " } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Ajouter une option" }));
    expect(props.onAddOption).toHaveBeenCalledWith("Aller au château");
  });

  it("renders the options read-only with ZERO action for an imported story", () => {
    renderEditor({ editable: false });
    // States stay visible…
    expect(screen.getByText(/liée → La suite/)).toBeInTheDocument();
    expect(screen.getByText(/destination à corriger/)).toBeInTheDocument();
    // …but no link gesture is offered.
    expect(screen.queryByRole("button", { name: /Lier/ })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Retirer l'option/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Ajouter une option" }),
    ).not.toBeInTheDocument();
  });

  it("surfaces a refused link inline at the acted-on option", () => {
    renderEditor({
      optionError: {
        nodeId: "n1",
        optionIndex: 1,
        error: {
          code: "LIBRARY_INCONSISTENT",
          message: "La destination choisie n'existe plus dans l'histoire.",
          userAction: "Recharge l'éditeur puis choisis un nœud existant.",
          details: null,
        },
      },
    });
    const alert = screen
      .getAllByRole("alert")
      .find((el) =>
        el.textContent?.includes("La destination choisie n'existe plus"),
      );
    expect(alert).toBeDefined();
    expect(alert).toHaveTextContent(
      "Recharge l'éditeur puis choisis un nœud existant.",
    );
  });

  it("renders a NAMED empty state when the node has no option yet", () => {
    renderEditor({ node: NODES[1] });
    expect(
      screen.getByText("Aucune option pour l'instant."),
    ).toBeInTheDocument();
  });

  it("renders nothing when no node is selected", () => {
    const { container } = renderEditor({ node: null });
    expect(container).toBeEmptyDOMElement();
  });
});
