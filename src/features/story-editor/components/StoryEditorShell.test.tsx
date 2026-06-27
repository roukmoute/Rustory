import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { StoryDetailDto } from "../../../shared/ipc-contracts/story";
import type { UseStoryExport } from "../../import-export/hooks/use-story-export";
import type { UseStoryRecovery } from "../hooks/use-story-recovery";

import { StoryEditorShell } from "./StoryEditorShell";

function buildDetail(overrides: Partial<StoryDetailDto> = {}): StoryDetailDto {
  return {
    id: "abc",
    title: "Le soleil couchant",
    schemaVersion: 1,
    structureJson: '{"schemaVersion":1,"nodes":[]}',
    contentChecksum: "a".repeat(64),
    createdAt: "2026-04-23T09:00:00.000Z",
    updatedAt: "2026-04-23T09:00:00.000Z",
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

function renderShell(props: Partial<Parameters<typeof StoryEditorShell>[0]> = {}) {
  const onBack = vi.fn();
  const onFlushAutoSave = vi.fn();
  render(
    <StoryEditorShell
      detail={props.detail ?? buildDetail()}
      draftTitle={props.draftTitle ?? "Le soleil couchant"}
      saveStatus={props.saveStatus ?? { kind: "idle" }}
      recovery={props.recovery ?? stubRecovery()}
      exporter={props.exporter ?? stubExporter()}
      onSetDraftTitle={props.onSetDraftTitle ?? vi.fn()}
      onRetrySave={props.onRetrySave ?? vi.fn()}
      onFlushAutoSave={props.onFlushAutoSave ?? onFlushAutoSave}
      onBack={props.onBack ?? onBack}
    />,
  );
  return { onBack, onFlushAutoSave };
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

  it("renders the honest v1 empty states, named and not hidden (UX-DR38)", () => {
    renderShell();
    expect(
      screen.getByText("Aucune saison ni nœud pour l'instant."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Aucun nœud à éditer pour l'instant."),
    ).toBeInTheDocument();
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

  it("makes each content zone a keyboard focus stop (AC3)", () => {
    renderShell();
    const structureRoot = screen.getByText("Le soleil couchant", {
      selector: ".story-structure-navigator__root-label",
    }).parentElement;
    const nodeEmpty = screen.getByText("Aucun nœud à éditer pour l'instant.");
    expect(structureRoot).toHaveAttribute("tabindex", "0");
    expect(nodeEmpty).toHaveAttribute("tabindex", "0");
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
