/**
 * Wire contract for the story-transfer flow (the FIRST real device write).
 * Mirror of the Rust transfer DTO (state + acceptance). The transfer reuses the
 * shared `job:*` event channel (see `story-preparation.ts`) with
 * `jobType = "transfer_story"` and the `transfer` phase.
 *
 * The transfer state is COMPOSED by Rust and only PRESENTED here. The `transferring`
 * variant describes the in-flight phase the hook derives from `job:progress` (incl.
 * the final `verify` phase); `read_transfer_state` itself returns `idle` or
 * `verified`. The success terminal `verified` (`transférée et vérifiée`) only ever
 * appears AFTER the `verify` phase proved the write (indexed + content present +
 * byte-faithful); it carries the AC2 summary. Cross-stack contract tests keep the
 * shapes symmetric.
 */

import type { PreparationStory } from "./story-preparation";

/** The selected local story identified in the transfer state. Same shape as the
 *  preparation story (canonical lowercase UUID + the local title). */
export type TransferStory = PreparationStory;

/** The `verified` confirmation summary (AC2/FR15), composed in Rust and rendered
 *  VERBATIM — the user-facing lines arrive ready-made so React never reinterprets
 *  them. `changed` = what changed (+ final state), `unchanged` = what stayed. */
export interface TransferVerifiedSummary {
  /** "« <Titre> » est maintenant sur la Lunii." */
  changed: string;
  /** "N autres histoires de l'appareil restent inchangées." */
  unchanged: string;
}

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
      // Terminal SUCCESS: the write landed AND the verify phase confirmed it
      // (indexed + content present + byte-faithful). The ONLY state that carries
      // `transférée et vérifiée` — proven, never claimed. `summary` is the AC2
      // confirmation (what stayed unchanged), composed in Rust.
      kind: "verified";
      deviceIdentifier: string;
      story: TransferStory;
      summary: TransferVerifiedSummary;
    }
  | {
      kind: "retryable";
      story: TransferStory;
      cause: TransferCause;
      message: string;
      userAction: string;
      /** Whether the device was already mutated (AC2): `"failed"` = untouched
       *  (→ `échec récupérable`), `"incomplete"` = write had started (→ `transfert
       *  incomplet`). Optional: a passive `read_transfer_state` only returns
       *  `idle` / `transferred`, so the issue is normally carried by the
       *  `job:failed` event, not this DTO. */
      completeness?: "failed" | "incomplete";
    };

/** Acceptance returned by `start_transfer_story`. */
export interface StartTransferAcceptedDto {
  jobId: string;
  storyId: string;
}

/**
 * Closed terminal-kind discriminant of a durable transfer outcome (the Transfer
 * Resume Contract). Mirror of the Rust `TransferTerminalKindDto`. Drives the hook's
 * re-hydrated sticky state on mount.
 */
export type TransferTerminalKind =
  | "verified"
  | "partial"
  | "retryable"
  | "incomplete";

/**
 * Wire shape of a durable transfer outcome re-hydrated from `transfer_jobs`
 * (`read_transfer_outcome`). Mirror of the Rust `TransferOutcomeDto`. `terminalKind`
 * drives the rendered state; `cause` is the AC3 structured cause of a write-phase
 * terminal (absent on a verify terminal / `verified`); `summary` carries the
 * `verified` confirmation lines (present iff `terminalKind === "verified"`).
 * `message` / `userAction` are the canonical FR copy rendered verbatim; `recordedAt`
 * is the ISO-8601 UTC instant the terminal was remembered.
 */
export interface TransferOutcomeDto {
  storyId: string;
  terminalKind: TransferTerminalKind;
  /** AC3 structured cause — only on a write-phase `retryable` / `incomplete`. */
  cause?: TransferCause;
  message: string;
  userAction: string;
  /** The `verified` confirmation lines — only on `terminalKind === "verified"`. */
  summary?: TransferVerifiedSummary;
  recordedAt: string;
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
  ["verified", new Set(["kind", "deviceIdentifier", "story", "summary"])],
  [
    "retryable",
    new Set(["kind", "story", "cause", "message", "userAction", "completeness"]),
  ],
]);

const COMPLETENESS: ReadonlySet<string> = new Set(["failed", "incomplete"]);

const ALLOWED_STORY_KEYS: ReadonlySet<string> = new Set(["id", "title"]);
const ALLOWED_SUMMARY_KEYS: ReadonlySet<string> = new Set([
  "changed",
  "unchanged",
]);

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

function isVerifiedSummary(value: unknown): value is TransferVerifiedSummary {
  if (typeof value !== "object" || value === null) return false;
  const s = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(s, ALLOWED_SUMMARY_KEYS)) return false;
  return isNonEmptyString(s.changed) && isNonEmptyString(s.unchanged);
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
    case "verified":
      return (
        isDeviceIdentifier(c.deviceIdentifier) &&
        isTransferStory(c.story) &&
        isVerifiedSummary(c.summary)
      );
    case "retryable":
      if (!isTransferStory(c.story)) return false;
      if (typeof c.cause !== "string" || !CAUSES.has(c.cause)) return false;
      if (!isNonEmptyString(c.message)) return false;
      if (!isNonEmptyString(c.userAction)) return false;
      if (
        c.completeness !== undefined &&
        !COMPLETENESS.has(c.completeness as string)
      ) {
        return false;
      }
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

const TERMINAL_KINDS: ReadonlySet<string> = new Set([
  "verified",
  "partial",
  "retryable",
  "incomplete",
]);

const OUTCOME_ALLOWED_KEYS: ReadonlySet<string> = new Set([
  "storyId",
  "terminalKind",
  "cause",
  "message",
  "userAction",
  "summary",
  "recordedAt",
]);

// ISO-8601 UTC with millisecond precision and a literal `Z` — the exact shape Rust
// composes (`now_iso_ms`). A bare-second or offset timestamp is drift.
const ISO_UTC_MS_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

/**
 * Runtime guard for `TransferOutcomeDto`. Rejects every drift: unknown `kind`,
 * missing/extra fields, wrong types, malformed `storyId` / `recordedAt`,
 * unrecognized `cause`, empty `message` / `userAction`, AND the coherence rules
 * mirroring the Rust model: `summary` is present iff `terminalKind === "verified"`;
 * a `cause` is allowed only on a write-phase `retryable` / `incomplete` and is
 * REQUIRED on `incomplete`.
 */
export function isTransferOutcomeDto(value: unknown): value is TransferOutcomeDto {
  if (typeof value !== "object" || value === null) return false;
  const o = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(o, OUTCOME_ALLOWED_KEYS)) return false;
  if (typeof o.storyId !== "string" || !STORY_ID_PATTERN.test(o.storyId)) {
    return false;
  }
  if (typeof o.terminalKind !== "string" || !TERMINAL_KINDS.has(o.terminalKind)) {
    return false;
  }
  if (!isNonEmptyString(o.message)) return false;
  if (!isNonEmptyString(o.userAction)) return false;
  if (typeof o.recordedAt !== "string" || !ISO_UTC_MS_PATTERN.test(o.recordedAt)) {
    return false;
  }
  // `cause`: only on a write-phase terminal, required on `incomplete`.
  if (o.cause !== undefined) {
    if (typeof o.cause !== "string" || !CAUSES.has(o.cause)) return false;
    if (o.terminalKind === "verified" || o.terminalKind === "partial") {
      return false;
    }
  }
  if (o.terminalKind === "incomplete" && o.cause === undefined) return false;
  // `summary`: present iff `verified`.
  if (o.terminalKind === "verified") {
    if (!isVerifiedSummary(o.summary)) return false;
  } else if (o.summary !== undefined) {
    return false;
  }
  return true;
}
