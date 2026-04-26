import type { StoryCardDto } from "./library";

/**
 * Wire contract for the `create_story` Tauri command input. Mirror of
 * `src-tauri/src/ipc/dto/story.rs::CreateStoryInputDto`. The Rust side
 * enforces `deny_unknown_fields`, so extending this interface without a
 * matching Rust change will fail deserialization at runtime.
 */
export interface CreateStoryInput {
  title: string;
}

/**
 * Wire contract for the `update_story` Tauri command input. Mirror of
 * `src-tauri/src/ipc/dto/story.rs::UpdateStoryInputDto`.
 */
export interface UpdateStoryInput {
  id: string;
  title: string;
}

/**
 * Wire contract returned by `update_story`. Mirror of
 * `UpdateStoryOutputDto`. `updatedAt` is the ISO-8601 UTC millisecond
 * timestamp the Rust core committed.
 */
export interface UpdateStoryOutput {
  id: string;
  title: string;
  updatedAt: string;
}

/**
 * Full wire projection of a single story for the edit surface. Mirror of
 * `src-tauri/src/ipc/dto/story.rs::StoryDetailDto`. `structureJson` is the
 * exact byte sequence covered by `contentChecksum` — never reserialize or
 * reformat it on the frontend.
 */
export interface StoryDetailDto {
  id: string;
  title: string;
  schemaVersion: number;
  structureJson: string;
  contentChecksum: string;
  createdAt: string;
  updatedAt: string;
}

const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/;

/** ISO-8601 UTC timestamp with millisecond precision. The contract
 *  mandates the `Z` suffix AND exactly three fractional digits; any
 *  other representation (including the semantically-equivalent
 *  `+00:00` offset, fractional nanoseconds, or zero-fractional) is
 *  refused so Rust stays the single source for the canonical wire
 *  shape — a drift in the Rust serializer must fail loudly here, not
 *  be quietly accommodated. The previous greedy `\.\d+` accepted
 *  fractional nanoseconds (10 digits) which would have masked a
 *  formatter regression on the Rust side. */
const ISO8601_UTC_PATTERN =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

/**
 * Runtime guard for a `StoryDetailDto` payload. Rust is authoritative, but
 * the frontend still refuses to trust a wire shape that drifts from the
 * contract — the edit surface must never render against an arbitrary
 * object.
 */
export function isStoryDetailDto(value: unknown): value is StoryDetailDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.id !== "string" || candidate.id.length === 0)
    return false;
  if (typeof candidate.title !== "string" || candidate.title.trim().length === 0)
    return false;
  if (
    typeof candidate.schemaVersion !== "number" ||
    !Number.isInteger(candidate.schemaVersion) ||
    candidate.schemaVersion < 1
  ) {
    return false;
  }
  if (typeof candidate.structureJson !== "string") return false;
  if (
    typeof candidate.contentChecksum !== "string" ||
    !SHA256_HEX_PATTERN.test(candidate.contentChecksum)
  ) {
    return false;
  }
  if (
    typeof candidate.createdAt !== "string" ||
    !ISO8601_UTC_PATTERN.test(candidate.createdAt)
  ) {
    return false;
  }
  if (
    typeof candidate.updatedAt !== "string" ||
    !ISO8601_UTC_PATTERN.test(candidate.updatedAt)
  ) {
    return false;
  }
  return true;
}

/**
 * Runtime guard for `UpdateStoryOutput`. The shape is small enough that
 * the wire contract is unlikely to drift, but `applyRecovery` resolves
 * with this exact payload — locking the shape here keeps the apply path
 * symmetric with the rest of the recovery contract.
 */
export function isUpdateStoryOutput(value: unknown): value is UpdateStoryOutput {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.id !== "string" || candidate.id.length === 0)
    return false;
  if (typeof candidate.title !== "string" || candidate.title.trim().length === 0)
    return false;
  if (
    typeof candidate.updatedAt !== "string" ||
    !ISO8601_UTC_PATTERN.test(candidate.updatedAt)
  ) {
    return false;
  }
  return true;
}

/**
 * Wire input for the `record_draft` Tauri command. Mirror of
 * `RecordDraftInputDto`. `draftTitle` may be empty (the user erased
 * everything) and may carry control characters — re-validation only
 * kicks in at apply time, never at record time.
 */
export interface RecordDraftInput {
  storyId: string;
  draftTitle: string;
}

/** Wire input for the `apply_recovery` Tauri command. */
export interface ApplyRecoveryInput {
  storyId: string;
}

/**
 * Tagged union returned by `read_recoverable_draft`. `kind: "none"` is
 * the explicit "no draft to recover" state and is NOT an error — the UI
 * simply hides the recovery banner.
 */
export type RecoverableDraft =
  | { kind: "none" }
  | {
      kind: "recoverable";
      storyId: string;
      draftTitle: string;
      draftAt: string;
      persistedTitle: string;
    };

/** Hard cap mirrored from the Rust-side `MAX_DRAFT_TITLE_CHARS`. */
const MAX_DRAFT_TITLE_LENGTH = 4096;

/**
 * Runtime guard for `RecoverableDraft`. A drifted wire shape (missing
 * `kind`, unknown `kind` value, malformed timestamp) is rejected so the
 * recovery banner never renders against an arbitrary object.
 */
export function isRecoverableDraft(value: unknown): value is RecoverableDraft {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.kind === "none") {
    // The `none` variant must not carry any extra fields — anything else
    // is drift.
    return Object.keys(candidate).length === 1;
  }
  if (candidate.kind !== "recoverable") return false;
  if (typeof candidate.storyId !== "string" || candidate.storyId.length === 0)
    return false;
  // `draftTitle` MAY be empty (user erased everything before crash).
  if (typeof candidate.draftTitle !== "string") return false;
  // Cap is anti-DoS, mirrored from Rust's `MAX_DRAFT_TITLE_CHARS`. Rust
  // counts Unicode scalars via `chars().count()`; JS `.length` would
  // count UTF-16 code units, which double-counts surrogate pairs (e.g.
  // an emoji = 2). Use the iterator form to match Rust exactly.
  if ([...candidate.draftTitle].length > MAX_DRAFT_TITLE_LENGTH) return false;
  if (
    typeof candidate.draftAt !== "string" ||
    !ISO8601_UTC_PATTERN.test(candidate.draftAt)
  ) {
    return false;
  }
  // `persistedTitle` mirrors `stories.title`, which the CHECK constraint
  // forbids from being blank — a blank value here means drift.
  if (
    typeof candidate.persistedTitle !== "string" ||
    candidate.persistedTitle.trim().length === 0
  ) {
    return false;
  }
  return true;
}

export type { StoryCardDto };
