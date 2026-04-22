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

export type { StoryCardDto };
