import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  isAnnouncementVoicesDto,
  isStoryPresentationDto,
  isVoicePreviewDto,
} from "../../shared/ipc-contracts/presentation";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage: ((msg: number) => void) | null = null;
  },
}));

type ChannelLike = { onmessage?: ((msg: number) => void) | null };

// Symmetric twins of the Rust `tests/contracts/presentation.rs` shapes.
const PRESENTATION = {
  layout: "menu",
  voiceId: "system:say:Thomas",
  archiveRetained: false,
  linear: true,
  title: {
    spokenText: "Tina et le serpent à plumes.",
    status: "ready",
    assetId: "a-title",
  },
  question: {
    spokenText: "Quelle histoire veux-tu écouter ?",
    status: "missing",
  },
  chapters: [
    {
      nodeId: "n1",
      label: "Le trésor : épisode 1/10",
      spokenText: "Épisode 1. Le trésor.",
      status: "stale",
      assetId: "a-1",
    },
  ],
};

const VOICES = {
  voices: [
    { id: "system:say:Thomas", name: "Thomas", language: "fr-FR", engine: "system" },
    {
      id: "embedded:fr_FR-siwis-medium",
      name: "Voix neuronale française (Siwis)",
      language: "fr-FR",
      engine: "embedded",
    },
  ],
  selectedVoiceId: "system:say:Thomas",
  selectedIsStored: false,
  embedded: {
    state: "installed",
    version: "2023.11.14-2",
    downloadBytes: 89667631,
    voiceId: "embedded:fr_FR-siwis-medium",
    voiceName: "Voix neuronale française (Siwis)",
  },
};

describe("presentation guards", () => {
  it("accept the Rust wire shapes and refuse drifted ones", () => {
    expect(isStoryPresentationDto(PRESENTATION)).toBe(true);
    expect(isStoryPresentationDto({ ...PRESENTATION, layout: "grid" })).toBe(false);
    expect(
      isStoryPresentationDto({
        ...PRESENTATION,
        chapters: [{ ...PRESENTATION.chapters[0], status: "done" }],
      }),
    ).toBe(false);
    expect(isStoryPresentationDto({ ...PRESENTATION, linear: "yes" })).toBe(false);

    expect(isAnnouncementVoicesDto(VOICES)).toBe(true);
    for (const state of ["unsupported", "notInstalled", "installing", "installed"]) {
      expect(
        isAnnouncementVoicesDto({ ...VOICES, embedded: { ...VOICES.embedded, state } }),
      ).toBe(true);
    }
    expect(
      isAnnouncementVoicesDto({ ...VOICES, embedded: { ...VOICES.embedded, state: "ready" } }),
    ).toBe(false);
    expect(
      isAnnouncementVoicesDto({
        ...VOICES,
        voices: [{ ...VOICES.voices[0], engine: "cloud" }],
      }),
    ).toBe(false);

    expect(
      isVoicePreviewDto({ dataUrl: "data:audio/wav;base64,UklGRg==", durationMs: 2954, spokenText: "x" }),
    ).toBe(true);
    expect(isVoicePreviewDto({ dataUrl: "https://x", durationMs: 1, spokenText: "x" })).toBe(false);
  });
});

describe("presentation IPC facade", () => {
  beforeEach(async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockReset();
  });

  it("reads and sets the presentation through the validated commands", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockResolvedValueOnce(PRESENTATION);
    const facade = await import("../commands/presentation");
    const read = await facade.readStoryPresentation({ storyId: "s1" });
    expect(read.layout).toBe("menu");
    expect(vi.mocked(core.invoke)).toHaveBeenCalledWith("read_story_presentation", { storyId: "s1" });

    vi.mocked(core.invoke).mockResolvedValueOnce({ ...PRESENTATION, layout: "sequential" });
    const set = await facade.setStoryLayout({ storyId: "s1", layout: "sequential" });
    expect(set.layout).toBe("sequential");
    expect(vi.mocked(core.invoke)).toHaveBeenCalledWith("set_story_layout", {
      input: { storyId: "s1", layout: "sequential" },
    });
  });

  it("streams generation progress over a channel and resolves the outcome", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockImplementationOnce(async (_cmd, args) => {
      const channel = (args as { onProgress: ChannelLike }).onProgress;
      channel.onmessage?.(50);
      channel.onmessage?.(100);
      channel.onmessage?.(Number.NaN);
      return { generated: 4, planned: 4, voiceId: "system:say:Thomas", presentation: PRESENTATION };
    });
    const facade = await import("../commands/presentation");
    const ticks: number[] = [];
    const outcome = await facade.generateStoryAnnouncements({ storyId: "s1" }, (p) => ticks.push(p));
    expect(ticks).toEqual([50, 100]);
    expect(outcome.generated).toBe(4);
  });

  it("throws a contract drift error on an off-contract answer and normalizes AppError rejections", async () => {
    const core = await import("@tauri-apps/api/core");
    const facade = await import("../commands/presentation");
    vi.mocked(core.invoke).mockResolvedValueOnce({ voices: "none" });
    await expect(facade.readAnnouncementVoices()).rejects.toBeInstanceOf(
      facade.PresentationContractDriftError,
    );
    vi.mocked(core.invoke).mockRejectedValueOnce({
      code: "MEDIA_PROCESSING_FAILED",
      message: "La voix n'a pas pu produire l'annonce.",
      userAction: "Réessaie.",
      details: { source: "speech", cause: "engine_failed" },
    });
    await expect(facade.previewAnnouncementVoice({ voiceId: "system:say:Thomas" })).rejects.toMatchObject({
      code: "MEDIA_PROCESSING_FAILED",
    });
  });

  it("installs the embedded voice with byte progress and returns the voices", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockImplementationOnce(async (_cmd, args) => {
      const channel = (args as { onProgress: ChannelLike }).onProgress;
      channel.onmessage?.(10);
      channel.onmessage?.(100);
      return VOICES;
    });
    const facade = await import("../commands/presentation");
    const ticks: number[] = [];
    const voices = await facade.installEmbeddedVoice((p) => ticks.push(p));
    expect(ticks).toEqual([10, 100]);
    expect(voices.embedded.state).toBe("installed");
    expect(vi.mocked(core.invoke)).toHaveBeenCalledWith(
      "install_embedded_voice",
      expect.objectContaining({ onProgress: expect.anything() }),
    );
  });
});
