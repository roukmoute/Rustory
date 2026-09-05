import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/presentation", () => ({
  readAnnouncementVoices: vi.fn(),
  setAnnouncementVoice: vi.fn(),
  previewAnnouncementVoice: vi.fn(),
  installEmbeddedVoice: vi.fn(),
}));

import {
  installEmbeddedVoice,
  previewAnnouncementVoice,
  readAnnouncementVoices,
  setAnnouncementVoice,
} from "../../../ipc/commands/presentation";

import { AnnouncementVoiceSettings } from "./AnnouncementVoiceSettings";

const THOMAS = { id: "system:say:Thomas", name: "Thomas", language: "fr-FR", engine: "system" as const };
const SIWIS = {
  id: "embedded:fr_FR-siwis-medium",
  name: "Voix neuronale française (Siwis)",
  language: "fr-FR",
  engine: "embedded" as const,
};
const EMBEDDED_ABSENT = {
  state: "notInstalled" as const,
  downloadBytes: 89_667_631,
  voiceId: SIWIS.id,
  voiceName: SIWIS.name,
};

describe("AnnouncementVoiceSettings", () => {
  beforeEach(() => {
    vi.mocked(readAnnouncementVoices).mockReset();
    vi.mocked(setAnnouncementVoice).mockReset();
    vi.mocked(previewAnnouncementVoice).mockReset();
    vi.mocked(installEmbeddedVoice).mockReset();
    // The preview plays through an <audio>; happy-dom has no media engine.
    vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
  });

  it("lists the French voices with the selection and offers the embedded download", async () => {
    vi.mocked(readAnnouncementVoices).mockResolvedValueOnce({
      voices: [THOMAS],
      selectedVoiceId: THOMAS.id,
      selectedIsStored: false,
      embedded: EMBEDDED_ABSENT,
    });
    render(<AnnouncementVoiceSettings />);
    const section = await screen.findByRole("region", { name: /voix des annonces/i });
    const radio = within(section).getByRole("radio", { name: /thomas/i });
    expect(radio).toBeChecked();
    expect(
      within(section).getByRole("button", { name: /télécharger la voix neuronale \(90 Mo\)/i }),
    ).toBeInTheDocument();
  });

  it("stores a new selection through Rust and re-renders its answer", async () => {
    const user = userEvent.setup();
    vi.mocked(readAnnouncementVoices).mockResolvedValueOnce({
      voices: [THOMAS, SIWIS],
      selectedVoiceId: THOMAS.id,
      selectedIsStored: false,
      embedded: { ...EMBEDDED_ABSENT, state: "installed", version: "2023.11.14-2" },
    });
    vi.mocked(setAnnouncementVoice).mockResolvedValueOnce({
      voices: [THOMAS, SIWIS],
      selectedVoiceId: SIWIS.id,
      selectedIsStored: true,
      embedded: { ...EMBEDDED_ABSENT, state: "installed", version: "2023.11.14-2" },
    });
    render(<AnnouncementVoiceSettings />);
    const siwis = await screen.findByRole("radio", { name: /siwis/i });
    await user.click(siwis);
    expect(setAnnouncementVoice).toHaveBeenCalledWith({ voiceId: SIWIS.id });
    await waitFor(() => expect(screen.getByRole("radio", { name: /siwis/i })).toBeChecked());
    expect(screen.getByText("Voix neuronale installée.")).toBeInTheDocument();
  });

  it("previews a voice by playing the sample Rust returns", async () => {
    const user = userEvent.setup();
    vi.mocked(readAnnouncementVoices).mockResolvedValueOnce({
      voices: [THOMAS],
      selectedVoiceId: THOMAS.id,
      selectedIsStored: false,
      embedded: EMBEDDED_ABSENT,
    });
    vi.mocked(previewAnnouncementVoice).mockResolvedValueOnce({
      dataUrl: "data:audio/wav;base64,UklGRg==",
      durationMs: 1200,
      spokenText: "Quelle histoire veux-tu écouter ?",
    });
    render(<AnnouncementVoiceSettings />);
    await user.click(await screen.findByRole("button", { name: /écouter — thomas/i }));
    expect(previewAnnouncementVoice).toHaveBeenCalledWith({ voiceId: THOMAS.id });
    await waitFor(() => expect(HTMLMediaElement.prototype.play).toHaveBeenCalled());
  });

  it("downloads the embedded voice with progress and shows the installed state", async () => {
    const user = userEvent.setup();
    vi.mocked(readAnnouncementVoices).mockResolvedValueOnce({
      voices: [THOMAS],
      selectedVoiceId: THOMAS.id,
      selectedIsStored: false,
      embedded: EMBEDDED_ABSENT,
    });
    let resolveInstall: (v: unknown) => void = () => undefined;
    vi.mocked(installEmbeddedVoice).mockImplementationOnce((onProgress) => {
      onProgress?.(42);
      return new Promise((resolve) => {
        resolveInstall = resolve;
      }) as never;
    });
    render(<AnnouncementVoiceSettings />);
    await user.click(await screen.findByRole("button", { name: /télécharger la voix neuronale/i }));
    const bar = await screen.findByRole("progressbar", { name: /téléchargement de la voix neuronale/i });
    expect(bar).toHaveAttribute("aria-valuenow", "42");
    resolveInstall({
      voices: [THOMAS, SIWIS],
      selectedVoiceId: SIWIS.id,
      selectedIsStored: true,
      embedded: { ...EMBEDDED_ABSENT, state: "installed", version: "2023.11.14-2" },
    });
    await waitFor(() => expect(screen.getByText("Voix neuronale installée.")).toBeInTheDocument());
    expect(screen.getByRole("radio", { name: /siwis/i })).toBeChecked();
  });

  it("surfaces a failed download as the Rust message plus its next gesture", async () => {
    const user = userEvent.setup();
    vi.mocked(readAnnouncementVoices).mockResolvedValueOnce({
      voices: [],
      selectedIsStored: false,
      embedded: EMBEDDED_ABSENT,
    });
    vi.mocked(installEmbeddedVoice).mockRejectedValueOnce({
      code: "MEDIA_PROCESSING_FAILED",
      message: "Le téléchargement de la voix neuronale a échoué.",
      userAction: "Vérifie ta connexion internet puis réessaie.",
      details: { source: "embedded_voice", cause: "download" },
    });
    render(<AnnouncementVoiceSettings />);
    expect(await screen.findByText(/aucune voix française n'est disponible/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /télécharger la voix neuronale/i }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Le téléchargement de la voix neuronale a échoué. Vérifie ta connexion internet puis réessaie.",
    );
    expect(screen.getByRole("button", { name: /télécharger la voix neuronale/i })).toBeInTheDocument();
  });

  it("stays calm when the voices cannot be read", async () => {
    vi.mocked(readAnnouncementVoices).mockRejectedValueOnce(new Error("drift"));
    render(<AnnouncementVoiceSettings />);
    expect(await screen.findByText(/les voix n'ont pas pu être lues/i)).toBeInTheDocument();
  });
});
