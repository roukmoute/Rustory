import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { StoryDetailDto } from "../../../shared/ipc-contracts/story";
import type { UseStoryExport } from "../../import-export/hooks/use-story-export";
import type { UseNodeEditor } from "../hooks/use-node-editor";
import type { UseStoryRecovery } from "../hooks/use-story-recovery";
import type { UseStructureEditor } from "../hooks/use-structure-editor";

import { StoryEditorShell } from "./StoryEditorShell";

vi.mock("../../../ipc/commands/story", () => ({
  readNodeMedia: vi.fn().mockResolvedValue({ dataUrl: "data:image/png;base64,AA" }),
}));

function buildDetail(overrides: Partial<StoryDetailDto> = {}): StoryDetailDto {
  return {
    id: "abc",
    title: "Le soleil couchant",
    schemaVersion: 3,
    structureJson: '{"schemaVersion":3,"startNodeId":"n1","nodes":[]}',
    contentChecksum: "a".repeat(64),
    createdAt: "2026-04-23T09:00:00.000Z",
    updatedAt: "2026-04-23T09:00:00.000Z",
    editable: true,
    structure: {
      startNodeId: "n1",
      nodes: [
        { id: "n1", label: "", isStart: true, hasIssue: false, options: [] },
      ],
    },
    node: { id: "n1", text: "", label: "", image: null, audio: null },
    ...overrides,
  };
}

function stubNodeEditor(overrides: Partial<UseNodeEditor> = {}): UseNodeEditor {
  return {
    nodeId: "n1",
    editable: true,
    text: "",
    label: "",
    saveStatus: { kind: "idle" },
    image: null,
    audio: null,
    imageError: null,
    audioError: null,
    imageBusy: false,
    audioBusy: false,
    recovery: { kind: "none" },
    recoveryApplyError: null,
    setText: vi.fn(),
    setLabel: vi.fn(),
    flushNodeAutoSave: vi.fn(),
    attachMedia: vi.fn(),
    removeMedia: vi.fn(),
    applyRecovery: vi.fn(),
    discardRecovery: vi.fn(),
    ...overrides,
  };
}

function stubRecovery(): UseStoryRecovery {
  return {
    state: { kind: "none" },
    apply: vi.fn(),
    discard: vi.fn(),
    retry: vi.fn(),
    dismissReadError: vi.fn(),
  };
}

function stubExporter(): UseStoryExport {
  return {
    status: { kind: "idle" },
    triggerExport: vi.fn().mockResolvedValue(undefined),
    retryExport: vi.fn().mockResolvedValue(undefined),
    dismissStatus: vi.fn(),
  };
}

function stubStructureEditor(
  overrides: Partial<UseStructureEditor> = {},
): UseStructureEditor {
  return {
    selectedNodeId: null,
    busy: false,
    lastError: null,
    clearError: vi.fn(),
    selectNode: vi.fn(),
    refreshDetail: vi.fn(),
    addNode: vi.fn(),
    addNodeAndLink: vi.fn(),
    deleteNode: vi.fn(),
    moveNode: vi.fn(),
    addOption: vi.fn(),
    setOptionLink: vi.fn(),
    removeOption: vi.fn(),
    ...overrides,
  };
}

function renderShell(props: Partial<Parameters<typeof StoryEditorShell>[0]> = {}) {
  const onBack = vi.fn();
  const onFlushAutoSave = vi.fn();
  const { container } = render(
    <StoryEditorShell
      detail={props.detail ?? buildDetail()}
      draftTitle={props.draftTitle ?? "Le soleil couchant"}
      saveStatus={props.saveStatus ?? { kind: "idle" }}
      recovery={props.recovery ?? stubRecovery()}
      exporter={props.exporter ?? stubExporter()}
      nodeEditor={props.nodeEditor ?? stubNodeEditor()}
      structureEditor={props.structureEditor ?? stubStructureEditor()}
      onSetDraftTitle={props.onSetDraftTitle ?? vi.fn()}
      onRetrySave={props.onRetrySave ?? vi.fn()}
      onFlushAutoSave={props.onFlushAutoSave ?? onFlushAutoSave}
      onBack={props.onBack ?? onBack}
    />,
  );
  return { onBack, onFlushAutoSave, container };
}

describe("<StoryEditorShell />", () => {
  it("renders the three coexisting zones at once (AC1)", () => {
    renderShell();

    // The editor is its own dominant context.
    expect(
      screen.getByRole("main", { name: "Éditeur d'histoire" }),
    ).toBeInTheDocument();
    // Zone 1 — global structure.
    expect(
      screen.getByRole("region", { name: "Structure de l'histoire" }),
    ).toBeInTheDocument();
    // Zone 2 — current node.
    expect(
      screen.getByRole("region", { name: "Nœud courant" }),
    ).toBeInTheDocument();
    // Zone 3 — story state + actions.
    expect(
      screen.getByRole("heading", { name: "Le soleil couchant" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("textbox", { name: /titre de l'histoire/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /retour à la bibliothèque/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /exporter l'histoire/i }),
    ).toBeInTheDocument();
  });

  it("renders the projected node and its named empty media states (UX-DR38)", () => {
    renderShell();
    // The node editor edits the current node's text + metadata.
    expect(
      screen.getByRole("textbox", { name: "Texte du nœud" }),
    ).toBeInTheDocument();
    // The optional media slots are named, never hidden.
    expect(screen.getByText("Aucune image")).toBeInTheDocument();
    expect(screen.getByText("Aucun audio")).toBeInTheDocument();
    // The navigator shows the current node (projected from Rust), not a
    // "no node yet" empty state.
    expect(screen.getByText(/en cours d'édition/i)).toBeInTheDocument();
  });

  it("keeps a stable focus order: structure → current node → global actions (AC3)", () => {
    renderShell();
    const structure = screen.getByRole("region", {
      name: "Structure de l'histoire",
    });
    const node = screen.getByRole("region", { name: "Nœud courant" });
    const back = screen.getByRole("button", {
      name: /retour à la bibliothèque/i,
    });

    const following = Node.DOCUMENT_POSITION_FOLLOWING;
    // structure precedes node …
    // eslint-disable-next-line no-bitwise
    expect(structure.compareDocumentPosition(node) & following).toBe(following);
    // … and node precedes the global actions.
    // eslint-disable-next-line no-bitwise
    expect(node.compareDocumentPosition(back) & following).toBe(following);
  });

  it("keeps a keyboard focus stop in the structure zone and editable node fields (AC3)", () => {
    const { container } = renderShell();
    // The roving tabindex lives on the node ENTRIES now: exactly one entry
    // of the structure list is tabbable.
    const tabbableEntries = container.querySelectorAll(
      '.story-structure-navigator__node[tabindex="0"]',
    );
    expect(tabbableEntries).toHaveLength(1);
    // The node fields are inherently focusable form controls.
    expect(
      screen.getByRole("textbox", { name: "Texte du nœud" }),
    ).toBeEnabled();
  });

  it("gates ALL structural actions while a recovery decision is pending", () => {
    // A pending title-recovery decision locks the content fields — the
    // structural mutations must be gated too: mutating the graph under an
    // undecided recovery would race the buffered content.
    renderShell({
      recovery: {
        state: {
          kind: "recoverable",
          draft: {
            storyId: "abc",
            draftTitle: "Tapé avant le crash",
            draftAt: "2026-07-04T12:00:00.000Z",
            persistedTitle: "Le soleil couchant",
          },
        },
        apply: vi.fn(),
        discard: vi.fn(),
        retry: vi.fn(),
        dismissReadError: vi.fn(),
      } as unknown as UseStoryRecovery,
    });
    // Navigator: zero structural action rendered.
    expect(
      screen.queryByRole("button", { name: "Ajouter un nœud" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Supprimer le nœud/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Monter|Descendre/ }),
    ).not.toBeInTheDocument();
    // Option link editor: zero link gesture rendered.
    expect(
      screen.queryByRole("button", { name: /^Lier — / }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Ajouter une option" }),
    ).not.toBeInTheDocument();
  });

  it("never mixes the library context into the editor (AC3)", () => {
    renderShell();
    // No library-only affordances leak into the separate edit context.
    expect(
      screen.queryByRole("button", { name: /créer une histoire/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /importer une histoire/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("searchbox"),
    ).not.toBeInTheDocument();
  });

  it("exposes no transfer/send action — sending stays anchored in the library (§6)", () => {
    renderShell();
    expect(
      screen.queryByRole("button", { name: /envoyer|transférer/i }),
    ).not.toBeInTheDocument();
  });

  it("calls onBack from Retour à la bibliothèque", async () => {
    const user = userEvent.setup();
    const { onBack } = renderShell();
    await user.click(
      screen.getByRole("button", { name: /retour à la bibliothèque/i }),
    );
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("describes the locked field by a REAL element during the recovery probe (F3)", () => {
    // While the recovery read is loading, the Field is disabled and its
    // `aria-describedby` points at "story-edit-recovery-banner" — that id must
    // resolve to the on-screen loading status, not dangle, so AT hears why the
    // Field is locked during the initial probe too.
    const recovery: UseStoryRecovery = {
      ...stubRecovery(),
      state: { kind: "loading" },
    };
    renderShell({ recovery });

    const field = screen.getByRole("textbox", { name: /titre de l'histoire/i });
    expect(field).toBeDisabled();
    expect(field).toHaveAttribute("aria-describedby", "story-edit-recovery-banner");

    const description = document.getElementById("story-edit-recovery-banner");
    expect(description).not.toBeNull();
    expect(description).toHaveTextContent(
      "Vérification d'un brouillon récupérable…",
    );
  });
});
