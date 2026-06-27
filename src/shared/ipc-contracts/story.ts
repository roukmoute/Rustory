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
 * A resolved node media slot. Mirror of `NodeMediaSlotDto`. `state` is
 * `ready` (the bytes are present) or `attention` (a dangling reference —
 * repairable; the rest of the node stays editable). `format` / `byteSize`
 * are present only when `ready`.
 */
export interface NodeMediaSlot {
  assetId: string;
  mediaType: "image" | "audio";
  state: "ready" | "attention";
  format?: string;
  byteSize?: number;
}

/**
 * The current node PROJECTED BY RUST. Mirror of `NodeContentDto`. The UI
 * consumes this and never recomposes a node from `structureJson`.
 */
export interface NodeContentDto {
  id: string;
  text: string;
  label: string;
  image: NodeMediaSlot | null;
  audio: NodeMediaSlot | null;
}

/**
 * Full wire projection of a single story for the edit surface. Mirror of
 * `src-tauri/src/ipc/dto/story.rs::StoryDetailDto`. `structureJson` is the
 * exact byte sequence covered by `contentChecksum` — never reserialize or
 * reformat it on the frontend. `editable` is `false` for an imported story
 * (its node is read-only); `node` is the Rust-projected current node, or
 * `null` when the structure could not be projected (degraded).
 */
export interface StoryDetailDto {
  id: string;
  title: string;
  schemaVersion: number;
  structureJson: string;
  contentChecksum: string;
  createdAt: string;
  updatedAt: string;
  editable: boolean;
  node: NodeContentDto | null;
}

/** Mirror of `NodeWriteOutputDto` — the outcome of every node write. */
export interface NodeWriteOutput {
  id: string;
  updatedAt: string;
  contentChecksum: string;
  node: NodeContentDto;
}

/** Mirror of `UpdateNodeContentInputDto`. */
export interface UpdateNodeContentInput {
  storyId: string;
  nodeId: string;
  text: string;
  label: string;
}

/** Which media slot an attach / remove targets. */
export type NodeMediaSlotKind = "image" | "audio";

/** Mirror of `NodeMediaSlotInputDto` (attach / remove). */
export interface NodeMediaSlotInput {
  storyId: string;
  nodeId: string;
  slot: NodeMediaSlotKind;
}

/** Tagged outcome of `attach_node_media`. `cancelled` covers a dismissed
 *  file picker (never an error). */
export type AttachNodeMediaOutcome =
  | { kind: "cancelled" }
  | { kind: "attached"; output: NodeWriteOutput };

/** Mirror of `NodeMediaPreviewDto` — a self-contained `data:` URL. */
export interface NodeMediaPreview {
  dataUrl: string;
}

/** Mirror of `RecordNodeDraftInputDto` (NFR8 recovery buffer). */
export interface RecordNodeDraftInput {
  storyId: string;
  nodeId: string;
  draftText: string;
  draftLabel: string;
}

/** Tagged union returned by `read_recoverable_node_draft`. */
export type RecoverableNodeDraft =
  | { kind: "none" }
  | {
      kind: "recoverable";
      storyId: string;
      nodeId: string;
      draftText: string;
      draftLabel: string;
      draftAt: string;
      persistedText: string;
      persistedLabel: string;
    };

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
  if (typeof candidate.editable !== "boolean") return false;
  if (candidate.node !== null && !isNodeContentDto(candidate.node)) return false;
  return true;
}

/**
 * Runtime guard for a `NodeMediaSlot`. STRICT on the `state`↔fields coupling so
 * a Rust/TS drift raises a `NodeContractDriftError` instead of being masked by a
 * `média · 0 o` fallback in the UI: a `ready` slot MUST carry a known format and
 * a safe non-negative size; an `attention` slot MUST carry neither.
 */
export function isNodeMediaSlot(value: unknown): value is NodeMediaSlot {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.assetId !== "string" || c.assetId.length === 0) return false;
  if (c.mediaType !== "image" && c.mediaType !== "audio") return false;
  if (c.state === "ready") {
    if (typeof c.format !== "string" || c.format.length === 0) return false;
    return (
      typeof c.byteSize === "number" &&
      Number.isSafeInteger(c.byteSize) &&
      c.byteSize >= 0
    );
  }
  if (c.state === "attention") {
    return c.format === undefined && c.byteSize === undefined;
  }
  return false;
}

/** Runtime guard for a `NodeContentDto`. */
export function isNodeContentDto(value: unknown): value is NodeContentDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.id !== "string" || c.id.length === 0) return false;
  if (typeof c.text !== "string") return false;
  if (typeof c.label !== "string") return false;
  if (c.image !== null && !isNodeMediaSlot(c.image)) return false;
  if (c.audio !== null && !isNodeMediaSlot(c.audio)) return false;
  return true;
}

/** Runtime guard for a `NodeWriteOutput`. */
export function isNodeWriteOutput(value: unknown): value is NodeWriteOutput {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.id !== "string" || c.id.length === 0) return false;
  if (typeof c.updatedAt !== "string" || !ISO8601_UTC_PATTERN.test(c.updatedAt)) {
    return false;
  }
  if (
    typeof c.contentChecksum !== "string" ||
    !SHA256_HEX_PATTERN.test(c.contentChecksum)
  ) {
    return false;
  }
  return isNodeContentDto(c.node);
}

/** Runtime guard for an `AttachNodeMediaOutcome`. */
export function isAttachNodeMediaOutcome(
  value: unknown,
): value is AttachNodeMediaOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "cancelled") return Object.keys(c).length === 1;
  if (c.kind === "attached") return isNodeWriteOutput(c.output);
  return false;
}

/** Runtime guard for a `NodeMediaPreview`. */
export function isNodeMediaPreview(value: unknown): value is NodeMediaPreview {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  return typeof c.dataUrl === "string" && c.dataUrl.startsWith("data:");
}

/** Runtime guard for a `RecoverableNodeDraft`. */
export function isRecoverableNodeDraft(
  value: unknown,
): value is RecoverableNodeDraft {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "none") return Object.keys(c).length === 1;
  if (c.kind !== "recoverable") return false;
  if (typeof c.storyId !== "string" || c.storyId.length === 0) return false;
  if (typeof c.nodeId !== "string" || c.nodeId.length === 0) return false;
  if (typeof c.draftText !== "string") return false;
  if (typeof c.draftLabel !== "string") return false;
  if (typeof c.persistedText !== "string") return false;
  if (typeof c.persistedLabel !== "string") return false;
  if (typeof c.draftAt !== "string" || !ISO8601_UTC_PATTERN.test(c.draftAt)) {
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
