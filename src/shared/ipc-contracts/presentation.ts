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

export interface AnnouncementDto {
  spokenText: string;
  status: AnnouncementStatus;
  assetId?: string;
}

export interface ChapterAnnouncementDto {
  nodeId: string;
  label: string;
  spokenText: string;
  status: AnnouncementStatus;
  assetId?: string;
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
const ENGINES: ReadonlySet<string> = new Set(["system", "embedded"]);
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
    isOptionalString(value.assetId)
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
    })
  );
}

export function isStoryLayout(value: unknown): value is StoryLayout {
  return typeof value === "string" && LAYOUTS.has(value);
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
