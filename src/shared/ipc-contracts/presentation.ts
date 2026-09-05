/**
 * Wire contracts of a story's PRESENTATION on the device (its layout and
 * its spoken announcements) and of the announcement VOICES.
 *
 * Frontend mirror of `src-tauri/src/ipc/dto/presentation.rs` — contract
 * tests on both sides validate the shapes.
 */

/** `sequential` = episodes chained in order; `menu` = spoken question then
 *  the episodes on the wheel. */
export type StoryLayout = "sequential" | "menu";

export type AnnouncementStatus = "ready" | "stale" | "missing";

/** `voice` = synthesized; `recorded` = the user's microphone. */
export type AnnouncementSource = "voice" | "recorded";

export interface AnnouncementDto {
  spokenText: string;
  status: AnnouncementStatus;
  assetId?: string;
  source?: AnnouncementSource;
}

export interface ChapterAnnouncementDto {
  nodeId: string;
  label: string;
  spokenText: string;
  status: AnnouncementStatus;
  assetId?: string;
  source?: AnnouncementSource;
}

/** Which announcement an attach / remove targets (tagged on `kind`). */
export type AnnouncementTarget =
  | { kind: "title" }
  | { kind: "question" }
  | { kind: "chapter"; nodeId: string };

export interface AttachRecordedAnnouncementInput {
  storyId: string;
  target: AnnouncementTarget;
  /** A mono 16-bit WAV, base64. */
  audioBase64: string;
}

export interface RemoveAnnouncementInput {
  storyId: string;
  target: AnnouncementTarget;
}

/** Why the structure does not lay out as episodes, naming the node to fix. */
export interface LinearBlockerDto {
  reason: "empty" | "malformed" | "branching" | "missingAudio";
  nodeId?: string;
  label?: string;
}

export interface StoryPresentationDto {
  layout: StoryLayout;
  /** The voice the stored announcements were generated with. */
  voiceId?: string;
  /** `true` iff the story is sent from its retained source archive — the
   *  layout then does not apply to the send. */
  archiveRetained: boolean;
  /** `true` iff the structure lays out as episodes (announcements make
   *  sense); `false` for a story with choices or without audio. */
  linear: boolean;
  /** When `linear` is false: the reason and the first node to fix. */
  linearBlocker?: LinearBlockerDto;
  title: AnnouncementDto;
  question: AnnouncementDto;
  chapters: ChapterAnnouncementDto[];
}

export interface SetStoryLayoutInput {
  storyId: string;
  layout: StoryLayout;
}

export interface GenerateAnnouncementsInput {
  storyId: string;
  force?: boolean;
}

export interface GenerateAnnouncementsOutcome {
  generated: number;
  planned: number;
  voiceId: string;
  presentation: StoryPresentationDto;
}

export type VoiceEngine = "system" | "embedded";

export interface AnnouncementVoiceDto {
  id: string;
  name: string;
  language: string;
  engine: VoiceEngine;
}

export type EmbeddedVoiceState =
  | "unsupported"
  | "notInstalled"
  | "installing"
  | "installed";

export interface EmbeddedVoiceStatusDto {
  state: EmbeddedVoiceState;
  version?: string;
  /** Bytes to download for an install (0 when unsupported). */
  downloadBytes: number;
  voiceId: string;
  voiceName: string;
}

export interface AnnouncementVoicesDto {
  /** The French voices available now, system voices first. */
  voices: AnnouncementVoiceDto[];
  /** The voice announcements are generated with (stored choice when still
   *  available, else the first available). Absent when no voice exists. */
  selectedVoiceId?: string;
  /** `true` iff `selectedVoiceId` comes from the stored setting. */
  selectedIsStored: boolean;
  embedded: EmbeddedVoiceStatusDto;
}

export interface VoicePreviewDto {
  dataUrl: string;
  durationMs: number;
  spokenText: string;
}

const LAYOUTS: ReadonlySet<string> = new Set(["sequential", "menu"]);
const STATUSES: ReadonlySet<string> = new Set(["ready", "stale", "missing"]);
const SOURCES: ReadonlySet<string> = new Set(["voice", "recorded"]);
const ENGINES: ReadonlySet<string> = new Set(["system", "embedded"]);
const LINEAR_REASONS: ReadonlySet<string> = new Set([
  "empty",
  "malformed",
  "branching",
  "missingAudio",
]);
const EMBEDDED_STATES: ReadonlySet<string> = new Set([
  "unsupported",
  "notInstalled",
  "installing",
  "installed",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isOptionalString(value: unknown): boolean {
  return value === undefined || typeof value === "string";
}

function isAnnouncement(value: unknown): value is AnnouncementDto {
  if (!isRecord(value)) return false;
  return (
    typeof value.spokenText === "string" &&
    typeof value.status === "string" &&
    STATUSES.has(value.status) &&
    isOptionalString(value.assetId) &&
    (value.source === undefined ||
      (typeof value.source === "string" && SOURCES.has(value.source)))
  );
}

function isChapterAnnouncement(value: unknown): value is ChapterAnnouncementDto {
  if (!isRecord(value)) return false;
  return (
    typeof value.nodeId === "string" &&
    value.nodeId.length > 0 &&
    typeof value.label === "string" &&
    isAnnouncement({
      spokenText: value.spokenText,
      status: value.status,
      assetId: value.assetId,
      source: value.source,
    })
  );
}

export function isStoryLayout(value: unknown): value is StoryLayout {
  return typeof value === "string" && LAYOUTS.has(value);
}

function isLinearBlocker(value: unknown): value is LinearBlockerDto {
  if (!isRecord(value)) return false;
  return (
    typeof value.reason === "string" &&
    LINEAR_REASONS.has(value.reason) &&
    isOptionalString(value.nodeId) &&
    isOptionalString(value.label)
  );
}

export function isStoryPresentationDto(
  value: unknown,
): value is StoryPresentationDto {
  if (!isRecord(value)) return false;
  return (
    isStoryLayout(value.layout) &&
    isOptionalString(value.voiceId) &&
    typeof value.archiveRetained === "boolean" &&
    typeof value.linear === "boolean" &&
    (value.linearBlocker === undefined || isLinearBlocker(value.linearBlocker)) &&
    isAnnouncement(value.title) &&
    isAnnouncement(value.question) &&
    Array.isArray(value.chapters) &&
    value.chapters.every(isChapterAnnouncement)
  );
}

export function isGenerateAnnouncementsOutcome(
  value: unknown,
): value is GenerateAnnouncementsOutcome {
  if (!isRecord(value)) return false;
  return (
    typeof value.generated === "number" &&
    typeof value.planned === "number" &&
    typeof value.voiceId === "string" &&
    isStoryPresentationDto(value.presentation)
  );
}

function isVoice(value: unknown): value is AnnouncementVoiceDto {
  if (!isRecord(value)) return false;
  return (
    typeof value.id === "string" &&
    value.id.length > 0 &&
    typeof value.name === "string" &&
    typeof value.language === "string" &&
    typeof value.engine === "string" &&
    ENGINES.has(value.engine)
  );
}

function isEmbeddedStatus(value: unknown): value is EmbeddedVoiceStatusDto {
  if (!isRecord(value)) return false;
  return (
    typeof value.state === "string" &&
    EMBEDDED_STATES.has(value.state) &&
    isOptionalString(value.version) &&
    typeof value.downloadBytes === "number" &&
    typeof value.voiceId === "string" &&
    typeof value.voiceName === "string"
  );
}

export function isAnnouncementVoicesDto(
  value: unknown,
): value is AnnouncementVoicesDto {
  if (!isRecord(value)) return false;
  return (
    Array.isArray(value.voices) &&
    value.voices.every(isVoice) &&
    isOptionalString(value.selectedVoiceId) &&
    typeof value.selectedIsStored === "boolean" &&
    isEmbeddedStatus(value.embedded)
  );
}

export function isVoicePreviewDto(value: unknown): value is VoicePreviewDto {
  if (!isRecord(value)) return false;
  return (
    typeof value.dataUrl === "string" &&
    value.dataUrl.startsWith("data:audio/") &&
    typeof value.durationMs === "number" &&
    typeof value.spokenText === "string"
  );
}
