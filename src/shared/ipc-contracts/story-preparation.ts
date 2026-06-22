/**
 * Wire contract for the story-preparation flow. Mirror of
 * `src-tauri/src/ipc/dto/story_preparation.rs` (state DTO + acceptance) and
 * `src-tauri/src/ipc/events.rs` (the three `job:*` event payloads).
 *
 * The preparation state is COMPOSED by Rust; the frontend only PRESENTS it. The
 * `preflight` / `preparing` variants describe the in-flight phases the hook
 * derives from `job:progress` events; `read_preparation_state` itself only ever
 * returns `idle` / `prepared` / `retryable`. Cross-stack contract tests keep the
 * shapes symmetric.
 */

import { isValidationBlocker, type ValidationBlocker } from "./story-validation";

/** Closed set of functional preparation-failure causes (AC3). */
export type PreparationCause =
  | "preflightNotPassing"
  | "artifactMissing"
  | "artifactCorrupt"
  | "deviceChanged"
  | "interrupted";

/** The selected local story identified in the preparation state. */
export interface PreparationStory {
  /** Local story id (canonical lowercase UUID). */
  id: string;
  /** The local `stories.title`. */
  title: string;
}

export type PreparationStateDto =
  | { kind: "idle" }
  | { kind: "preflight"; deviceIdentifier: string; story: PreparationStory }
  | {
      kind: "preparing";
      deviceIdentifier: string;
      story: PreparationStory;
      progress: number | null;
    }
  | {
      kind: "prepared";
      deviceIdentifier: string;
      story: PreparationStory;
      targetCohort: string;
      /** Whether the prepared descriptor carries a device-format pack (an
       *  imported story). `false` for a native story with no pack — the send
       *  gate disables `Envoyer` before any write attempt. */
      transferable: boolean;
    }
  | {
      kind: "retryable";
      story: PreparationStory;
      cause: PreparationCause;
      message: string;
      userAction: string;
      blockers: ValidationBlocker[];
    };

/** Acceptance returned by `start_prepare_story`. */
export interface StartPreparationAcceptedDto {
  jobId: string;
  storyId: string;
}

/** Phase carried by a `job:progress` event (the in-flight phases). Shared by
 *  the preparation flow (`preflight` / `prepare`) and the transfer flow
 *  (`transfer`, on the same job channel). `verify` stays out of scope. */
export type JobPhase = "preflight" | "prepare" | "transfer";

export interface JobProgressEvent {
  jobId: string;
  jobType: string;
  targetStoryId: string;
  phase: JobPhase;
  progress: number | null;
  sequence: number;
  message: string | null;
}

export interface JobCompletedEvent {
  jobId: string;
  jobType: string;
  targetStoryId: string;
  sequence: number;
}

export interface JobFailedEvent {
  jobId: string;
  jobType: string;
  targetStoryId: string;
  sequence: number;
  errorCode: string;
  errorMessage: string;
  userAction: string;
  /** Transfer-only (AC2): whether the device was already mutated when the write
   *  failed — `"failed"` (device left untouched → `échec récupérable`) vs
   *  `"incomplete"` (the write had started → `transfert incomplet`). Absent for
   *  preparation jobs, which have no device-mutation notion. */
  completeness?: "failed" | "incomplete";
  /** Transfer-only (AC3): the structured failure cause (camelCase) so the UI keeps
   *  "cause + issue + next action" in context, not only the message. Absent for
   *  preparation and the non-classifiable defensive terminal. */
  cause?: string;
}

const CAUSES: ReadonlySet<string> = new Set([
  "preflightNotPassing",
  "artifactMissing",
  "artifactCorrupt",
  "deviceChanged",
  "interrupted",
]);

// The generic job channel accepts every live phase Rustory emits: `preflight` /
// `prepare` (preparation) and `transfer` (the write flow). `verify` is reserved
// and stays rejected until its flow exists.
const PHASES: ReadonlySet<string> = new Set(["preflight", "prepare", "transfer"]);

const DEVICE_IDENTIFIER_PATTERN = /^[0-9a-f]{32}$/;
const STORY_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

// A Map (not a plain object): `.get(kind)` returns `undefined` for an unknown OR
// inherited key, so `hasOnlyAllowedKeys` reports drift instead of throwing.
const ALLOWED_KEYS: ReadonlyMap<string, ReadonlySet<string>> = new Map([
  ["idle", new Set(["kind"])],
  ["preflight", new Set(["kind", "deviceIdentifier", "story"])],
  ["preparing", new Set(["kind", "deviceIdentifier", "story", "progress"])],
  [
    "prepared",
    new Set(["kind", "deviceIdentifier", "story", "targetCohort", "transferable"]),
  ],
  [
    "retryable",
    new Set(["kind", "story", "cause", "message", "userAction", "blockers"]),
  ],
]);

const ALLOWED_STORY_KEYS: ReadonlySet<string> = new Set(["id", "title"]);

const ALLOWED_PROGRESS_KEYS: ReadonlySet<string> = new Set([
  "jobId",
  "jobType",
  "targetStoryId",
  "phase",
  "progress",
  "sequence",
  "message",
]);
const ALLOWED_COMPLETED_KEYS: ReadonlySet<string> = new Set([
  "jobId",
  "jobType",
  "targetStoryId",
  "sequence",
]);
const ALLOWED_FAILED_KEYS: ReadonlySet<string> = new Set([
  "jobId",
  "jobType",
  "targetStoryId",
  "sequence",
  "errorCode",
  "errorMessage",
  "userAction",
  "completeness",
  "cause",
]);

const COMPLETENESS: ReadonlySet<string> = new Set(["failed", "incomplete"]);

function hasOnlyAllowedKeys(
  value: Record<string, unknown>,
  allowed: ReadonlySet<string> | undefined,
): boolean {
  if (!allowed) return false;
  for (const k of Object.keys(value)) {
    if (!allowed.has(k)) return false;
  }
  return true;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function isPreparationStory(value: unknown): value is PreparationStory {
  if (typeof value !== "object" || value === null) return false;
  const s = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(s, ALLOWED_STORY_KEYS)) return false;
  if (typeof s.id !== "string" || !STORY_ID_PATTERN.test(s.id)) return false;
  if (typeof s.title !== "string" || s.title.length === 0) return false;
  return true;
}

function isDeviceIdentifier(value: unknown): value is string {
  return typeof value === "string" && DEVICE_IDENTIFIER_PATTERN.test(value);
}

function isProgress(value: unknown): value is number | null {
  return (
    value === null ||
    (typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1)
  );
}

function isSequence(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}

/**
 * Runtime guard for `PreparationStateDto`. Rejects every drift: unknown `kind`,
 * missing/extra fields, wrong types, malformed `deviceIdentifier` / `story.id`,
 * unrecognized `cause`, empty `message` / `userAction`, malformed blockers.
 */
export function isPreparationStateDto(
  value: unknown,
): value is PreparationStateDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.kind !== "string") return false;
  if (!hasOnlyAllowedKeys(c, ALLOWED_KEYS.get(c.kind))) return false;
  switch (c.kind) {
    case "idle":
      return true;
    case "preflight":
      return isDeviceIdentifier(c.deviceIdentifier) && isPreparationStory(c.story);
    case "preparing":
      return (
        isDeviceIdentifier(c.deviceIdentifier) &&
        isPreparationStory(c.story) &&
        isProgress(c.progress)
      );
    case "prepared":
      return (
        isDeviceIdentifier(c.deviceIdentifier) &&
        isPreparationStory(c.story) &&
        isNonEmptyString(c.targetCohort) &&
        typeof c.transferable === "boolean"
      );
    case "retryable":
      if (!isPreparationStory(c.story)) return false;
      if (typeof c.cause !== "string" || !CAUSES.has(c.cause)) return false;
      if (!isNonEmptyString(c.message)) return false;
      if (!isNonEmptyString(c.userAction)) return false;
      if (!Array.isArray(c.blockers)) return false;
      // Reuse the 3.x blocker guard verbatim — never two wordings for one cause.
      return c.blockers.every(isValidationBlocker);
    default:
      return false;
  }
}

/** Runtime guard for the acceptance returned by `start_prepare_story`. Both ids
 *  are canonical lowercase UUIDs (the `jobId` is a generated UUID, the `storyId`
 *  the selected story) — an empty / malformed id would have the hook subscribe to
 *  an impossible correlation, so it is rejected as drift like the other guards. */
export function isStartPreparationAcceptedDto(
  value: unknown,
): value is StartPreparationAcceptedDto {
  if (typeof value !== "object" || value === null) return false;
  const a = value as Record<string, unknown>;
  if (Object.keys(a).length !== 2) return false;
  return (
    typeof a.jobId === "string" &&
    STORY_ID_PATTERN.test(a.jobId) &&
    typeof a.storyId === "string" &&
    STORY_ID_PATTERN.test(a.storyId)
  );
}

function isJobBaseShape(c: Record<string, unknown>): boolean {
  return (
    isNonEmptyString(c.jobId) &&
    isNonEmptyString(c.jobType) &&
    typeof c.targetStoryId === "string" &&
    isSequence(c.sequence)
  );
}

export function isJobProgressEvent(value: unknown): value is JobProgressEvent {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_PROGRESS_KEYS)) return false;
  if (!isJobBaseShape(c)) return false;
  if (typeof c.phase !== "string" || !PHASES.has(c.phase)) return false;
  if (!isProgress(c.progress)) return false;
  return c.message === null || typeof c.message === "string";
}

export function isJobCompletedEvent(value: unknown): value is JobCompletedEvent {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_COMPLETED_KEYS)) return false;
  return isJobBaseShape(c);
}

export function isJobFailedEvent(value: unknown): value is JobFailedEvent {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_FAILED_KEYS)) return false;
  if (!isJobBaseShape(c)) return false;
  // `completeness` is optional (transfer-only); when present it must be a known
  // variant.
  if (c.completeness !== undefined && !COMPLETENESS.has(c.completeness as string)) {
    return false;
  }
  // `cause` is optional (transfer-only); when present it must be a non-empty
  // string (the closed transfer cause set lives in the transfer contract).
  if (c.cause !== undefined && !isNonEmptyString(c.cause)) {
    return false;
  }
  return (
    isNonEmptyString(c.errorCode) &&
    isNonEmptyString(c.errorMessage) &&
    isNonEmptyString(c.userAction)
  );
}
