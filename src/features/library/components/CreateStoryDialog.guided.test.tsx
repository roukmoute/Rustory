import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/story", () => ({ createStory: vi.fn() }));
import { createStory } from "../../../ipc/commands/story";
import type { ContentSourcePolicy } from "../../../shared/ipc-contracts/import-export";
import { CreateStoryDialog, type CreateStoryDialogProps } from "./CreateStoryDialog";

const POLICY: ContentSourcePolicy = { sources: [
  { kind: "rss", label: "Flux RSS", activation: "enabled", activationMarker: "Activée par la distribution officielle" },
  { kind: "web", label: "Page web", activation: "enabled", activationMarker: "Activée par la distribution officielle" },
  { kind: "atom", label: "Flux Atom", activation: "notActivated", reason: "Source indisponible: non activée dans la distribution officielle" },
] };

const showModal = vi.spyOn(HTMLDialogElement.prototype, "showModal");
const close = vi.spyOn(HTMLDialogElement.prototype, "close");
beforeAll(() => {
  showModal.mockImplementation(function (this: HTMLDialogElement) { this.setAttribute("open", ""); });
  close.mockImplementation(function (this: HTMLDialogElement) { this.removeAttribute("open"); });
});
afterAll(() => { showModal.mockRestore(); close.mockRestore(); });
beforeEach(() => { vi.mocked(createStory).mockReset(); });

function props(overrides: Partial<CreateStoryDialogProps> = {}): CreateStoryDialogProps {
  return { flow: "guided", open: true, onClose: vi.fn(), onCreated: vi.fn(), ...overrides };
}

describe("CreateStoryDialog guided flow", () => {
  it("asks for the intent first and keeps the title through a return to the choice", async () => {
    const user = userEvent.setup();
    const initial = props();
    vi.mocked(createStory).mockResolvedValue({ id: "new-story", title: "Mon aventure" });
    render(<CreateStoryDialog {...initial} />);
    expect(screen.getByRole("dialog", { name: "Que veux-tu faire ?" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Titre" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Écrire une histoire" }));
    const title = screen.getByRole("textbox", { name: "Titre" });
    expect(title).toHaveFocus();
    await user.type(title, "Mon aventure");
    await user.click(screen.getByRole("button", { name: "Retour" }));
    expect(screen.getByRole("button", { name: "Écrire une histoire" })).toHaveFocus();
    expect(createStory).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Écrire une histoire" }));
    expect(screen.getByRole("textbox", { name: "Titre" })).toHaveValue("Mon aventure");
    await user.click(screen.getByRole("button", { name: "Créer" }));
    expect(createStory).toHaveBeenCalledWith({ title: "Mon aventure" });
    await waitFor(() => expect(initial.onCreated).toHaveBeenCalledWith({ id: "new-story", title: "Mon aventure" }));
    expect(initial.onClose).toHaveBeenCalledOnce();
  });

  it("opens a requested direct entry and resets it on the next opening", async () => {
    const initial = props({ initialStep: "title" });
    const { rerender } = render(<CreateStoryDialog {...initial} />);
    expect(screen.getByRole("dialog", { name: "Comment s'appelle ton histoire ?" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Titre" }), { target: { value: "Ancien titre" } });
    rerender(<CreateStoryDialog {...initial} open={false} />);
    rerender(<CreateStoryDialog {...initial} initialStep="import" onImportArtifactRequest={vi.fn()} />);
    expect(screen.getByRole("dialog", { name: "Comment veux-tu importer ?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retour" }));
    fireEvent.click(screen.getByRole("button", { name: "Écrire une histoire" }));
    expect(screen.getByRole("textbox", { name: "Titre" })).toHaveValue("");
  });

  it("allows inspecting sources during another operation but guards their actions", () => {
    const initial = props({
      onImportArtifactRequest: vi.fn(), isImportArtifactUnavailable: true,
      onCreateFromFolderRequest: vi.fn(), isCreateFromFolderUnavailable: true,
      onCreateFromArchiveRequest: vi.fn(), isCreateFromArchiveUnavailable: true,
      onCreateFromRssRequest: vi.fn(), isCreateFromRssUnavailable: true,
      onCreateFromWebRequest: vi.fn(), isCreateFromWebUnavailable: true,
      contentSourcePolicy: POLICY,
    });
    render(<CreateStoryDialog {...initial} />);
    fireEvent.click(screen.getByRole("button", { name: "Importer" }));
    expect(screen.getByRole("dialog", { name: "Comment veux-tu importer ?" })).toBeInTheDocument();
    for (const label of ["Choisir un fichier .rustory…", "Choisir un dossier…", "Choisir une archive de pack (.zip)…", "Démarrer depuis une source externe (RSS)", "Démarrer depuis une page web", "Flux Atom"]) {
      const button = screen.getByRole("button", { name: label });
      expect(button).toHaveAttribute("aria-disabled", "true");
      fireEvent.click(button);
    }
    expect(initial.onClose).not.toHaveBeenCalled();
    expect(initial.onImportArtifactRequest).not.toHaveBeenCalled();
    expect(initial.onCreateFromFolderRequest).not.toHaveBeenCalled();
    expect(initial.onCreateFromArchiveRequest).not.toHaveBeenCalled();
    expect(initial.onCreateFromRssRequest).not.toHaveBeenCalled();
    expect(initial.onCreateFromWebRequest).not.toHaveBeenCalled();
  });

  it("closes the modal before handing over a real artifact import", () => {
    const initial = props({ initialStep: "import", onImportArtifactRequest: vi.fn() });
    render(<CreateStoryDialog {...initial} />);
    fireEvent.click(screen.getByRole("button", { name: "Choisir un fichier .rustory…" }));
    expect(initial.onClose).toHaveBeenCalledOnce();
    expect(initial.onImportArtifactRequest).toHaveBeenCalledOnce();
    expect(vi.mocked(initial.onClose).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(initial.onImportArtifactRequest!).mock.invocationCallOrder[0]);
  });

  it("keeps web creation fail-closed until the real policy enables it", () => {
    const initial = props({ onCreateFromWebRequest: vi.fn() });
    const { rerender } = render(<CreateStoryDialog {...initial} />);
    const web = screen.getByRole("button", { name: "Depuis une page web" });
    expect(web).toHaveAccessibleDescription("Sources externes indisponibles pour l'instant.");
    fireEvent.click(web);
    expect(initial.onCreateFromWebRequest).not.toHaveBeenCalled();
    rerender(<CreateStoryDialog {...initial} contentSourcePolicy={POLICY} />);
    fireEvent.click(screen.getByRole("button", { name: "Depuis une page web" }));
    expect(initial.onCreateFromWebRequest).toHaveBeenCalledOnce();
    expect(initial.onClose).toHaveBeenCalledOnce();
  });

  it("keeps title validation and cross-flow submission guards in the direct title step", () => {
    const initial = props({ initialStep: "title", isSubmitUnavailable: true });
    render(<CreateStoryDialog {...initial} />);
    const input = screen.getByRole("textbox", { name: "Titre" });
    expect(screen.getByRole("button", { name: "Créer" })).toHaveAccessibleDescription("Création impossible: titre requis");
    fireEvent.change(input, { target: { value: "Une histoire" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "Créer" }));
    expect(createStory).not.toHaveBeenCalled();
    fireEvent.change(input, { target: { value: "a".repeat(121) } });
    expect(screen.getByText(/titre trop long/)).toBeInTheDocument();
  });
});
