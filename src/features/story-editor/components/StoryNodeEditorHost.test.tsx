import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/story", () => ({
  readNodeMedia: vi.fn().mockResolvedValue({ dataUrl: "data:image/png;base64,AA" }),
}));

import type { AppError } from "../../../shared/errors/app-error";
import type { NodeMediaSlot } from "../../../shared/ipc-contracts/story";
import type { UseNodeEditor } from "../hooks/use-node-editor";

import { StoryNodeEditorHost } from "./StoryNodeEditorHost";

function stubEditor(overrides: Partial<UseNodeEditor> = {}): UseNodeEditor {
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

const READY_IMAGE: NodeMediaSlot = {
  assetId: "a1",
  mediaType: "image",
  state: "ready",
  format: "png",
  byteSize: 42,
};

describe("<StoryNodeEditorHost />", () => {
  it("renders the named current-node zone with the text + metadata fields", () => {
    render(<StoryNodeEditorHost storyId="s1" editor={stubEditor()} />);
    expect(
      screen.getByRole("region", { name: "Nœud courant" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("textbox", { name: "Texte du nœud" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("textbox", { name: /libellé du nœud/i }),
    ).toBeInTheDocument();
  });

  it("names the empty media slots without hiding them", () => {
    render(<StoryNodeEditorHost storyId="s1" editor={stubEditor()} />);
    expect(screen.getByText("Aucune image")).toBeInTheDocument();
    expect(screen.getByText("Aucun audio")).toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "Ajouter" }).length,
    ).toBe(2);
  });

  it("calls setText / setLabel when the user edits", async () => {
    const user = userEvent.setup();
    const editor = stubEditor();
    render(<StoryNodeEditorHost storyId="s1" editor={editor} />);
    await user.type(
      screen.getByRole("textbox", { name: "Texte du nœud" }),
      "x",
    );
    expect(editor.setText).toHaveBeenCalled();
  });

  it("falls back to the named empty state when no node is projected", () => {
    render(
      <StoryNodeEditorHost storyId="s1" editor={stubEditor({ nodeId: null })} />,
    );
    const empty = screen.getByText("Aucun nœud à éditer pour l'instant.");
    expect(empty).toBeInTheDocument();
    expect(empty).toHaveAttribute("tabindex", "0");
  });

  it("renders the named pack state INSTEAD of the controls for a device pack", () => {
    render(
      <StoryNodeEditorHost
        storyId="s1"
        editor={stubEditor({ editable: false })}
        packScoped
      >
        <div data-testid="hosted-option-editor" />
      </StoryNodeEditorHost>,
    );
    // The named state + its explanation (AC2): the content lives in the
    // copied pack; only the title (a local metadata) stays editable.
    const state = screen.getByText("Contenu porté par le pack de l'appareil");
    expect(state).toBeInTheDocument();
    expect(
      screen.getByText(/tu peux modifier le titre depuis l'éditeur/i),
    ).toBeInTheDocument();
    // The controls are ABSENT, not disabled — never a control that cannot
    // be saved, and the hosted option editor is not mounted either.
    expect(
      screen.queryByRole("textbox", { name: "Texte du nœud" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Ajouter" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("hosted-option-editor")).not.toBeInTheDocument();
  });

  it("keeps the defensive no-op projection when editable is false (defense in depth)", () => {
    // A device pack never mounts these controls at all (the pack state
    // replaces the zone) — the disabled projection only covers transient
    // states such as a pending recovery decision.
    render(
      <StoryNodeEditorHost
        storyId="s1"
        editor={stubEditor({ editable: false })}
      />,
    );
    expect(
      screen.queryByRole("button", { name: "Ajouter" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Texte du nœud" })).toBeDisabled();
  });

  it("surfaces a blocking media error inline at the slot (role=alert, never a toast)", () => {
    const error: AppError = {
      code: "MEDIA_INVALID",
      message: "Ce média utilise un format non pris en charge.",
      userAction: "Choisis une image PNG, JPEG ou BMP.",
      details: null,
    };
    render(
      <StoryNodeEditorHost storyId="s1" editor={stubEditor({ imageError: error })} />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Média bloqué");
    expect(alert).toHaveTextContent("format non pris en charge");
  });

  it("distinguishes a 'needs attention' media from a block (tone + label)", () => {
    render(
      <StoryNodeEditorHost
        storyId="s1"
        editor={stubEditor({
          image: { ...READY_IMAGE, state: "attention", format: undefined, byteSize: undefined },
        })}
      />,
    );
    expect(screen.getByText("Média à corriger")).toBeInTheDocument();
    // It is NOT a block: no "Média bloqué" alert.
    expect(screen.queryByText("Média bloqué")).not.toBeInTheDocument();
  });

  it("announces a media action in progress (NFR3, F9)", () => {
    render(
      <StoryNodeEditorHost
        storyId="s1"
        editor={stubEditor({ imageBusy: true })}
      />,
    );
    expect(
      screen.getByText("Ajout du média en cours…"),
    ).toBeInTheDocument();
  });

  it("announces a media UPDATE in progress when a media is already present (F9)", () => {
    render(
      <StoryNodeEditorHost
        storyId="s1"
        editor={stubEditor({ image: READY_IMAGE, imageBusy: true })}
      />,
    );
    expect(
      screen.getByText("Mise à jour du média en cours…"),
    ).toBeInTheDocument();
  });

  it("clears the preview when the slot's asset changes (F10)", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <StoryNodeEditorHost
        storyId="s1"
        editor={stubEditor({ image: READY_IMAGE })}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Aperçu" }));
    expect(await screen.findByRole("img")).toBeInTheDocument();
    // Replace the media with a different asset — the old preview must clear.
    rerender(
      <StoryNodeEditorHost
        storyId="s1"
        editor={stubEditor({ image: { ...READY_IMAGE, assetId: "a2" } })}
      />,
    );
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("offers the node recovery draft when present", () => {
    render(
      <StoryNodeEditorHost
        storyId="s1"
        editor={stubEditor({
          recovery: {
            kind: "recoverable",
            nodeId: "n1",
            draftText: "en cours",
            draftLabel: "",
            draftAt: "2026-06-27T12:00:00.000Z",
            persistedText: "",
            persistedLabel: "",
          },
        })}
      />,
    );
    expect(screen.getByText("Brouillon récupéré")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /reprendre ce brouillon/i }),
    ).toBeInTheDocument();
  });

  it("gates the node (locks fields, holds its recovery banner) while a title recovery is pending (P7)", () => {
    render(
      <StoryNodeEditorHost
        storyId="s1"
        gated
        editor={stubEditor({
          recovery: {
            kind: "recoverable",
            nodeId: "n1",
            draftText: "en cours",
            draftLabel: "",
            draftAt: "2026-06-27T12:00:00.000Z",
            persistedText: "",
            persistedLabel: "",
          },
        })}
      />,
    );
    // The node's OWN recovery banner is held back so the two recovery surfaces
    // never compete for the same decision.
    expect(screen.queryByText("Brouillon récupéré")).not.toBeInTheDocument();
    // And the fields are locked until the title recovery decision settles.
    expect(screen.getByRole("textbox", { name: "Texte du nœud" })).toBeDisabled();
    expect(
      screen.getByRole("textbox", { name: /libellé du nœud/i }),
    ).toBeDisabled();
    // Media actions are gated too (no add buttons).
    expect(
      screen.queryByRole("button", { name: "Ajouter" }),
    ).not.toBeInTheDocument();
  });
});
