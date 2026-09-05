import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/presentation", () => ({
  readStoryPresentation: vi.fn(),
  setStoryLayout: vi.fn(),
  generateStoryAnnouncements: vi.fn(),
  attachRecordedAnnouncement: vi.fn(),
  removeStoryAnnouncement: vi.fn(),
}));
vi.mock("../../../ipc/commands/story", () => ({
  readNodeMedia: vi.fn(),
}));

import {
  attachRecordedAnnouncement,
  generateStoryAnnouncements,
  readStoryPresentation,
  removeStoryAnnouncement,
  setStoryLayout,
} from "../../../ipc/commands/presentation";
import { readNodeMedia } from "../../../ipc/commands/story";
import type { StoryPresentationDto } from "../../../shared/ipc-contracts/presentation";
import { bytesToBase64 } from "../lib/wav";

import { StoryPresentationPanel } from "./StoryPresentationPanel";

const SEQUENTIAL: StoryPresentationDto = {
  layout: "sequential",
  archiveRetained: false,
  linear: true,
  title: { spokenText: "Tina.", status: "missing" },
  question: { spokenText: "Quelle histoire veux-tu écouter ?", status: "missing" },
  chapters: [
    { nodeId: "n1", label: "Un", spokenText: "Épisode 1. Un.", status: "missing" },
    { nodeId: "n2", label: "Deux", spokenText: "Deux.", status: "missing" },
  ],
};
const MENU_READY: StoryPresentationDto = {
  ...SEQUENTIAL,
  layout: "menu",
  voiceId: "system:say:Thomas",
  title: { spokenText: "Tina.", status: "ready", assetId: "a-t" },
  question: { ...SEQUENTIAL.question, status: "ready", assetId: "a-q" },
  chapters: [
    { nodeId: "n1", label: "Un", spokenText: "Épisode 1. Un.", status: "ready", assetId: "a-1" },
    { nodeId: "n2", label: "Deux", spokenText: "Deux.", status: "stale", assetId: "a-2" },
  ],
};

// A take long enough not to count as empty (a real WAV is 44 bytes of
// header plus samples).
const FAKE_WAV = new Uint8Array(64).map((_, i) => (i < 4 ? [82, 73, 70, 70][i] : i));
const FAKE_RECORDER = {
  start: vi.fn(async () => ({
    stop: vi.fn(async () => ({ wav: FAKE_WAV, durationMs: 1200, sampleRate: 22050 })),
    cancel: vi.fn(),
  })),
};

function renderPanel(): void {
  render(<StoryPresentationPanel storyId="s1" editable structureKey="k" recorder={FAKE_RECORDER} />);
}

describe("StoryPresentationPanel", () => {
  beforeEach(() => {
    vi.mocked(readStoryPresentation).mockReset();
    vi.mocked(setStoryLayout).mockReset();
    vi.mocked(generateStoryAnnouncements).mockReset();
    vi.mocked(readNodeMedia).mockReset();
    vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
  });

  it("shows the sequential layout selected and no announcements block", async () => {
    vi.mocked(readStoryPresentation).mockResolvedValueOnce(SEQUENTIAL);
    renderPanel();
    const panel = await screen.findByRole("region", { name: /présentation sur la lunii/i });
    expect(within(panel).getByRole("radio", { name: /à la suite/i })).toBeChecked();
    expect(within(panel).queryByRole("list", { name: /annonces/i })).not.toBeInTheDocument();
    expect(readStoryPresentation).toHaveBeenCalledWith({ storyId: "s1" });
  });

  it("switches to the menu layout through Rust and lists the announcements with their states", async () => {
    const user = userEvent.setup();
    vi.mocked(readStoryPresentation).mockResolvedValueOnce(SEQUENTIAL);
    vi.mocked(setStoryLayout).mockResolvedValueOnce({ ...SEQUENTIAL, layout: "menu" });
    renderPanel();
    await user.click(await screen.findByRole("radio", { name: /au choix/i }));
    expect(setStoryLayout).toHaveBeenCalledWith({ storyId: "s1", layout: "menu" });
    const list = await screen.findByRole("list", { name: /annonces/i });
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(4);
    expect(items[0]).toHaveTextContent("Titre de la série");
    expect(items[0]).toHaveTextContent("« Tina. »");
    expect(items[0]).toHaveTextContent("manquante");
    expect(items[2]).toHaveTextContent("Épisode 1");
    expect(items[2]).toHaveTextContent("« Épisode 1. Un. »");
    expect(within(list).queryByRole("button", { name: /écouter/i })).not.toBeInTheDocument();
    expect(within(list).getAllByRole("button", { name: /^enregistrer — /i })).toHaveLength(4);
    expect(screen.getByRole("button", { name: "Générer les annonces" })).toBeInTheDocument();
  });

  it("records an announcement with the microphone and attaches it through Rust", async () => {
    const user = userEvent.setup();
    vi.mocked(readStoryPresentation).mockResolvedValueOnce({ ...SEQUENTIAL, layout: "menu" });
    vi.mocked(attachRecordedAnnouncement).mockResolvedValueOnce({
      ...MENU_READY,
      question: { ...MENU_READY.question, source: "recorded" },
    });
    renderPanel();
    const list = await screen.findByRole("list", { name: /annonces/i });
    await user.click(within(list).getByRole("button", { name: /enregistrer — question/i }));
    await user.click(await screen.findByRole("button", { name: "Arrêter" }));
    await user.click(await screen.findByRole("button", { name: "Utiliser" }));
    await waitFor(() =>
      expect(attachRecordedAnnouncement).toHaveBeenCalledWith({
        storyId: "s1",
        target: { kind: "question" },
        audioBase64: bytesToBase64(FAKE_WAV),
      }),
    );
    expect(await within(list).findByText("ta voix")).toBeInTheDocument();
    expect(within(list).getByRole("button", { name: /réenregistrer — question/i })).toBeInTheDocument();
  });

  it("removes a clip through Rust", async () => {
    const user = userEvent.setup();
    vi.mocked(readStoryPresentation).mockResolvedValueOnce(MENU_READY);
    vi.mocked(removeStoryAnnouncement).mockResolvedValueOnce({ ...SEQUENTIAL, layout: "menu" });
    renderPanel();
    const list = await screen.findByRole("list", { name: /annonces/i });
    await user.click(within(list).getByRole("button", { name: /retirer — épisode 1/i }));
    expect(removeStoryAnnouncement).toHaveBeenCalledWith({
      storyId: "s1",
      target: { kind: "chapter", nodeId: "n1" },
    });
    await waitFor(() => expect(within(list).getAllByText("manquante")).toHaveLength(4));
  });

  it("generates the announcements with progress and re-renders the outcome", async () => {
    const user = userEvent.setup();
    vi.mocked(readStoryPresentation).mockResolvedValueOnce({ ...SEQUENTIAL, layout: "menu" });
    let resolveGeneration: (v: unknown) => void = () => undefined;
    vi.mocked(generateStoryAnnouncements).mockImplementationOnce((_input, onProgress) => {
      onProgress?.(50);
      return new Promise((resolve) => {
        resolveGeneration = resolve;
      }) as never;
    });
    renderPanel();
    await user.click(await screen.findByRole("button", { name: "Générer les annonces" }));
    expect(generateStoryAnnouncements).toHaveBeenCalledWith(
      { storyId: "s1", force: false },
      expect.any(Function),
    );
    const bar = await screen.findByRole("progressbar", { name: /génération des annonces/i });
    expect(bar).toHaveAttribute("aria-valuenow", "50");
    resolveGeneration({ generated: 4, planned: 4, voiceId: "system:say:Thomas", presentation: MENU_READY });
    const list = await screen.findByRole("list", { name: /annonces/i });
    await waitFor(() => expect(within(list).getAllByText("prête")).toHaveLength(3));
    expect(within(list).getByText("à régénérer")).toBeInTheDocument();
    expect(within(list).getAllByRole("button", { name: /écouter/i })).toHaveLength(4);
  });

  it("plays a generated clip through the node-media preview", async () => {
    const user = userEvent.setup();
    vi.mocked(readStoryPresentation).mockResolvedValueOnce(MENU_READY);
    vi.mocked(readNodeMedia).mockResolvedValueOnce({ dataUrl: "data:audio/wav;base64,UklGRg==" });
    renderPanel();
    await user.click(await screen.findByRole("button", { name: /écouter — question/i }));
    expect(readNodeMedia).toHaveBeenCalledWith({ storyId: "s1", assetId: "a-q" });
    await waitFor(() => expect(HTMLMediaElement.prototype.play).toHaveBeenCalled());
  });

  it("surfaces a generation failure with the Rust message and its next gesture", async () => {
    const user = userEvent.setup();
    vi.mocked(readStoryPresentation).mockResolvedValueOnce({ ...SEQUENTIAL, layout: "menu" });
    vi.mocked(generateStoryAnnouncements).mockRejectedValueOnce({
      code: "MEDIA_PROCESSING_FAILED",
      message: "Aucune voix n'est disponible sur cet ordinateur.",
      userAction: "Télécharge la voix neuronale depuis les réglages.",
      details: { source: "speech", cause: "no_engine" },
    });
    renderPanel();
    await user.click(await screen.findByRole("button", { name: "Générer les annonces" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Aucune voix n'est disponible sur cet ordinateur. Télécharge la voix neuronale depuis les réglages.",
    );
  });

  it("explains that an archive-sent story ignores the presentation", async () => {
    vi.mocked(readStoryPresentation).mockResolvedValueOnce({ ...SEQUENTIAL, archiveRetained: true });
    renderPanel();
    expect(await screen.findByText(/archive d'origine/i)).toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: /au choix/i })).not.toBeInTheDocument();
  });

  it("keeps the menu choice disabled for a story that does not lay out as episodes, naming the node to fix", async () => {
    vi.mocked(readStoryPresentation).mockResolvedValueOnce({
      ...SEQUENTIAL,
      linear: false,
      chapters: [],
      linearBlocker: { reason: "missingAudio", nodeId: "n3", label: "L'île des femmes" },
    });
    renderPanel();
    expect(await screen.findByRole("radio", { name: /au choix/i })).toBeDisabled();
    expect(
      screen.getByText(/l'épisode « L'île des femmes » n'a pas d'audio/i),
    ).toBeInTheDocument();
  });

  it("names a branching node and falls back to the generic note without a blocker", async () => {
    vi.mocked(readStoryPresentation).mockResolvedValueOnce({
      ...SEQUENTIAL,
      linear: false,
      chapters: [],
      linearBlocker: { reason: "branching", nodeId: "n1", label: "Un" },
    });
    renderPanel();
    expect(await screen.findByText(/l'épisode « Un » propose des choix/i)).toBeInTheDocument();
    vi.mocked(readStoryPresentation).mockResolvedValueOnce({ ...SEQUENTIAL, linear: false, chapters: [] });
    render(<StoryPresentationPanel storyId="s2" editable structureKey="k" />);
    expect(await screen.findByText(/demande un audio sur chaque épisode/i)).toBeInTheDocument();
  });

  it("summarizes the announcements in one line and explains a locked editor", async () => {
    vi.mocked(readStoryPresentation).mockResolvedValueOnce(MENU_READY);
    render(<StoryPresentationPanel storyId="s1" editable={false} structureKey="k" />);
    expect(await screen.findByText("4 annonces : 3 prêtes, 1 à régénérer")).toBeInTheDocument();
    expect(screen.getByText(/présentation verrouillée/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Générer les annonces" })).toBeDisabled();
  });
});
