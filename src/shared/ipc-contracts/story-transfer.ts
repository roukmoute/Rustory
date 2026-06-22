/**
 * Wire contract for the story-transfer flow (the FIRST real device write).
 * Mirror of the Rust transfer DTO (state + acceptance). The transfer reuses the
 * shared `job:*` event channel (see `story-preparation.ts`) with
 * `jobType = "transfer_story"` and the `transfer` phase.
 *
 * The transfer state is COMPOSED by Rust and only PRESENTED here. The `transferring`
 * variant describes the in-flight phase the hook derives from `job:progress`;
 * `read_transfer_state` itself returns `idle` / `transferring` / `transferred` /
 * `retryable`. The terminal `transferred` is deliberately NON-SUCCESS ("écriture
 * effectuée — vérification à venir") — verification is a later flow. Cross-stack
 * contract tests keep the shapes symmetric.
 */

import type { PreparationStory } from "./story-preparation";

/** The selected local story identified in the transfer state. Same shape as the
 *  preparation story (canonical lowercase UUID + the local title). */
export type TransferStory = PreparationStory;

/**
 * Closed set of functional transfer-failure causes (AC2/AC3). Mirror of the Rust
 * `TransferFailureCause`. A functional failure is a recoverable job state, never
 * an `AppError` (only transport failures are `AppError`s).
 */
export type TransferCause =
  | "writeNotAuthorized"
  | "notPrepared"
  | "notTransferable"
  | "deviceChanged"
  | "writeRejected"
  | "interrupted";

export type TransferStateDto =
  | { kind: "idle" }
  | {
      kind: "transferring";
      deviceIdentifier: string;
      story: TransferStory;
      progress: number | null;
    }
  | {
      // Terminal NON-SUCCESS: the bytes were written, but nothing is verified
      // yet. NEVER carries success vocabulary (`transférée et vérifiée` is a
      // later flow).
      kind: "transferred";
      deviceIdentifier: string;
      story: TransferStory;
    }
  | {
      kind: "retryable";
      story: TransferStory;
      cause: TransferCause;
      message: string;
      userAction: string;
    };

/** Acceptance returned by `start_transfer_story`. */
export interface StartTransferAcceptedDto {
  jobId: string;
  storyId: string;
}

const CAUSES: ReadonlySet<string> = new Set([
  "writeNotAuthorized",
  "notPrepared",
  "notTransferable",
  "deviceChanged",
  "writeRejected",
  "interrupted",
]);

const DEVICE_IDENTIFIER_PATTERN = /^[0-9a-f]{32}$/;
const STORY_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

// A Map (not a plain object): `.get(kind)` returns `undefined` for an unknown OR
// inherited key, so `hasOnlyAllowedKeys` reports drift instead of throwing.
const ALLOWED_KEYS: ReadonlyMap<string, ReadonlySet<string>> = new Map([
  ["idle", new Set(["kind"])],
  [
    "transferring",
    new Set(["kind", "deviceIdentifier", "story", "progress"]),
  ],
  ["transferred", new Set(["kind", "deviceIdentifier", "story"])],
  ["retryable", new Set(["kind", "story", "cause", "message", "userAction"])],
]);

const ALLOWED_STORY_KEYS: ReadonlySet<string> = new Set(["id", "title"]);

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

function isTransferStory(value: unknown): value is TransferStory {
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
    (typeof value === "number" &&
      Number.isFinite(value) &&
      value >= 0 &&
      value <= 1)
  );
}

/**
 * Runtime guard for `TransferStateDto`. Rejects every drift: unknown `kind`,
 * missing/extra fields, wrong types, malformed `deviceIdentifier` / `story.id`,
 * unrecognized `cause`, empty `message` / `userAction`.
 */
export function isTransferStateDto(value: unknown): value is TransferStateDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.kind !== "string") return false;
  if (!hasOnlyAllowedKeys(c, ALLOWED_KEYS.get(c.kind))) return false;
  switch (c.kind) {
    case "idle":
      return true;
    case "transferring":
      return (
        isDeviceIdentifier(c.deviceIdentifier) &&
        isTransferStory(c.story) &&
        isProgress(c.progress)
      );
    case "transferred":
      return isDeviceIdentifier(c.deviceIdentifier) && isTransferStory(c.story);
    case "retryable":
      if (!isTransferStory(c.story)) return false;
      if (typeof c.cause !== "string" || !CAUSES.has(c.cause)) return false;
      if (!isNonEmptyString(c.message)) return false;
      if (!isNonEmptyString(c.userAction)) return false;
      return true;
    default:
      return false;
  }
}

/** Runtime guard for the acceptance returned by `start_transfer_story`. Both ids
 *  are canonical lowercase UUIDs (the `jobId` is a generated UUID, the `storyId`
 *  the selected story) — an empty / malformed id would have the hook subscribe to
 *  an impossible correlation, so it is rejected as drift. */
export function isStartTransferAcceptedDto(
  value: unknown,
): value is StartTransferAcceptedDto {
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
