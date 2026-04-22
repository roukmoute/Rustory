import { invoke } from "@tauri-apps/api/core";

import type {
  CreateStoryInput,
  StoryCardDto,
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
