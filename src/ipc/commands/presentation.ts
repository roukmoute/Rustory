import { Channel, invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  AnnouncementVoicesDto,
  GenerateAnnouncementsInput,
  GenerateAnnouncementsOutcome,
  SetStoryLayoutInput,
  StoryPresentationDto,
  VoicePreviewDto,
} from "../../shared/ipc-contracts/presentation";
import {
  isAnnouncementVoicesDto,
  isGenerateAnnouncementsOutcome,
  isStoryPresentationDto,
  isVoicePreviewDto,
} from "../../shared/ipc-contracts/presentation";

/** Thrown when a presentation/voice command answers off-contract. */
export class PresentationContractDriftError extends Error {
  readonly raw: unknown;
  constructor(command: string, raw: unknown) {
    super(`${command} returned a payload that does not match the contract`);
    this.name = "PresentationContractDriftError";
    this.raw = raw;
  }
}

function progressChannel(
  onProgress?: (percent: number) => void,
): Channel<number> {
  const channel = new Channel<number>();
  if (onProgress) {
    channel.onmessage = (percent) => {
      if (typeof percent === "number" && Number.isFinite(percent)) {
        onProgress(Math.max(0, Math.min(100, Math.round(percent))));
      }
    };
  }
  return channel;
}

async function call<T>(
  command: string,
  args: Record<string, unknown>,
  guard: (value: unknown) => value is T,
): Promise<T> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>(command, args);
  } catch (err) {
    throw toAppError(err);
  }
  if (!guard(raw)) {
    throw new PresentationContractDriftError(command, raw);
  }
  return raw;
}

/** The layout and announcements of a story. */
export function readStoryPresentation(input: {
  storyId: string;
}): Promise<StoryPresentationDto> {
  return call(
    "read_story_presentation",
    { storyId: input.storyId },
    isStoryPresentationDto,
  );
}

/** Change a story's layout; the announcements are kept. */
export function setStoryLayout(
  input: SetStoryLayoutInput,
): Promise<StoryPresentationDto> {
  return call("set_story_layout", { input }, isStoryPresentationDto);
}

/**
 * Generate the missing / stale announcements with the selected voice.
 * `onProgress` receives an integer percent of the clips (a signal only —
 * the settled outcome is the resolved value).
 */
export function generateStoryAnnouncements(
  input: GenerateAnnouncementsInput,
  onProgress?: (percent: number) => void,
): Promise<GenerateAnnouncementsOutcome> {
  return call(
    "generate_story_announcements",
    { input, onProgress: progressChannel(onProgress) },
    isGenerateAnnouncementsOutcome,
  );
}

/** The available announcement voices, the selection, the embedded voice. */
export function readAnnouncementVoices(): Promise<AnnouncementVoicesDto> {
  return call("read_announcement_voices", {}, isAnnouncementVoicesDto);
}

/** Store the announcement voice (must be one of the available voices). */
export function setAnnouncementVoice(input: {
  voiceId: string;
}): Promise<AnnouncementVoicesDto> {
  return call("set_announcement_voice", { input }, isAnnouncementVoicesDto);
}

/** A spoken sample of a voice, as a playable data URL. */
export function previewAnnouncementVoice(input: {
  voiceId: string;
}): Promise<VoicePreviewDto> {
  return call("preview_announcement_voice", { input }, isVoicePreviewDto);
}

/**
 * Download and install the embedded neural voice (an explicit gesture,
 * ~90 MB), then select it. `onProgress` receives an integer percent of the
 * bytes.
 */
export function installEmbeddedVoice(
  onProgress?: (percent: number) => void,
): Promise<AnnouncementVoicesDto> {
  return call(
    "install_embedded_voice",
    { onProgress: progressChannel(onProgress) },
    isAnnouncementVoicesDto,
  );
}
