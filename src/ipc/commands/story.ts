import { invoke } from "@tauri-apps/api/core";

import type {
  CreateStoryInput,
  StoryCardDto,
  StoryDetailDto,
  UpdateStoryInput,
  UpdateStoryOutput,
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
