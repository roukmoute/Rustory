/**
 * Wire contract for the `get_library_overview` Tauri command.
 *
 * This file is the frontend mirror of `src-tauri/src/ipc/dto/library.rs`.
 * Any change here MUST match the Rust side — contract tests on both sides
 * validate the shape.
 */
export interface StoryCardDto {
  id: string;
  title: string;
}

export interface LibraryOverviewDto {
  stories: StoryCardDto[];
}

function isStoryCardDto(value: unknown): value is StoryCardDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.id === "string" && typeof candidate.title === "string"
  );
}

/**
 * Runtime guard against a malformed IPC payload. Rust is authoritative, but
 * we still refuse to trust a wire shape that drifts from the contract — the
 * UI must never render against an arbitrary object.
 */
export function isLibraryOverviewDto(
  value: unknown,
): value is LibraryOverviewDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    Array.isArray(candidate.stories) && candidate.stories.every(isStoryCardDto)
  );
}
