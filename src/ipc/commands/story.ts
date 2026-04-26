import { invoke } from "@tauri-apps/api/core";

import {
  isRecoverableDraft,
  isUpdateStoryOutput,
  type ApplyRecoveryInput,
  type CreateStoryInput,
  type RecordDraftInput,
  type RecoverableDraft,
  type StoryCardDto,
  type StoryDetailDto,
  type UpdateStoryInput,
  type UpdateStoryOutput,
} from "../../shared/ipc-contracts/story";

/**
 * Create a new story draft through the Rust core.
 *
 * The command is synchronous on the Rust side (a validated INSERT into the
 * local SQLite store), so no timeout guard is necessary here — the call
 * either resolves with the canonical card or rejects with a normalized
 * `AppError`. Callers that want to display pending state must track it
 * locally.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so
 * the wire contract stays owned by `src/ipc/`.
 */
export function createStory(input: CreateStoryInput): Promise<StoryCardDto> {
  return invoke<StoryCardDto>("create_story", { input });
}

/**
 * Persist a story's metadata (title only in the current MVP) through the
 * Rust core. Synchronous bounded mutation — no timeout wrapper. Rejects
 * with a normalized `AppError` on validation (`INVALID_STORY_TITLE`),
 * storage (`LOCAL_STORAGE_UNAVAILABLE`) or consistency
 * (`LIBRARY_INCONSISTENT`) failures. Callers (specifically the autosave
 * hook) own the retry lifecycle.
 */
export function saveStory(input: UpdateStoryInput): Promise<UpdateStoryOutput> {
  return invoke<UpdateStoryOutput>("update_story", { input });
}

/**
 * Read a single story detail for the edit surface. Returns `null` when
 * the row is absent — the route renders that as "Histoire introuvable"
 * without treating it as an error.
 */
export function getStoryDetail(input: {
  storyId: string;
}): Promise<StoryDetailDto | null> {
  return invoke<StoryDetailDto | null>("get_story_detail", {
    storyId: input.storyId,
  });
}

/**
 * Thrown when `read_recoverable_draft` returns a payload that does not
 * match the canonical wire shape. The captured `raw` value is kept on
 * the error instance for support / debugging, never surfaced verbatim
 * to the user.
 */
export class ReadRecoverableDraftContractDriftError extends Error {
  public readonly raw: unknown;
  constructor(message: string, options: { raw: unknown }) {
    super(message);
    this.name = "ReadRecoverableDraftContractDriftError";
    this.raw = options.raw;
  }
}

/**
 * Thrown when `apply_recovery` returns a payload that does not match
 * the `UpdateStoryOutput` wire shape. Same discipline as the read path:
 * the raw value lives on the error for support, not for display.
 */
export class ApplyRecoveryContractDriftError extends Error {
  public readonly raw: unknown;
  constructor(message: string, options: { raw: unknown }) {
    super(message);
    this.name = "ApplyRecoveryContractDriftError";
    this.raw = options.raw;
  }
}

/**
 * Buffer a keystroke value into the recovery store. Best-effort on the
 * frontend: callers should `.catch(() => undefined)` and proceed —
 * autosave is the durability mechanism, not this. Used by `useStoryEditor`
 * with a 150 ms debounce so the buffer is fresher than the autosave window.
 */
export function recordDraft(input: RecordDraftInput): Promise<void> {
  return invoke<void>("record_draft", { input });
}

/**
 * Read the recoverable-draft state for a story. Resolves with a tagged
 * union — `kind: "none"` is informational, never an error. Throws
 * `ReadRecoverableDraftContractDriftError` on wire-shape drift so the
 * UI can fall back to a safe state instead of rendering an arbitrary
 * object.
 */
export async function readRecoverableDraft(input: {
  storyId: string;
}): Promise<RecoverableDraft> {
  const raw = await invoke<unknown>("read_recoverable_draft", {
    storyId: input.storyId,
  });
  if (!isRecoverableDraft(raw)) {
    throw new ReadRecoverableDraftContractDriftError(
      "read_recoverable_draft a renvoyé une forme inattendue.",
      { raw },
    );
  }
  return raw;
}

/**
 * Apply the recoverable draft authoritatively. The Rust core re-validates
 * the buffered title, UPDATEs `stories`, and consumes the draft row in a
 * single transaction. The resolved `UpdateStoryOutput` carries the
 * freshly committed values so the caller can reconcile its in-memory
 * `detail` without a follow-up `get_story_detail` round-trip.
 */
export async function applyRecovery(
  input: ApplyRecoveryInput,
): Promise<UpdateStoryOutput> {
  const raw = await invoke<unknown>("apply_recovery", { input });
  if (!isUpdateStoryOutput(raw)) {
    throw new ApplyRecoveryContractDriftError(
      "apply_recovery a renvoyé une forme inattendue.",
      { raw },
    );
  }
  return raw;
}

/**
 * Drop the buffered draft without modifying canonical state. Idempotent
 * — a second call on an already-empty row resolves silently.
 *
 * The optional `expectedDraftAt` is forwarded to the Rust core as a
 * compare-and-swap guard: when the UI passes the timestamp it
 * observed, a concurrent `record_draft` that refreshed the row
 * between observation and click is preserved. When absent, the
 * DELETE runs unconditionally — that path is reserved for callers
 * that explicitly accept dropping whatever is buffered (e.g. the
 * autosave's auto-discard when the user types back to the
 * persisted value).
 */
export function discardDraft(input: {
  storyId: string;
  expectedDraftAt?: string;
}): Promise<void> {
  return invoke<void>("discard_draft", {
    input: {
      storyId: input.storyId,
      expectedDraftAt: input.expectedDraftAt ?? null,
    },
  });
}
