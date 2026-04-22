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
  if (typeof candidate.id !== "string" || typeof candidate.title !== "string") {
    return false;
  }
  // An empty id collapses under `key={id}` in React lists and cannot be
  // used as a lookup key in pruneSelection or /story/:storyId/edit routing.
  if (candidate.id.length === 0) return false;
  // A blank title produces a card with no accessible name (StoryCard uses
  // `aria-label={title}`); refuse the payload instead of rendering a
  // focusable button keyboard users cannot identify.
  if (candidate.title.trim().length === 0) return false;
  return true;
}

/**
 * Runtime guard against a malformed IPC payload. Rust is authoritative, but
 * we still refuse to trust a wire shape that drifts from the contract — the
 * UI must never render against an arbitrary object.
 *
 * In addition to shape validation, duplicate ids are rejected: they would
 * silently collide on `key={id}` in React lists and make selection /
 * routing ambiguous.
 */
export function isLibraryOverviewDto(
  value: unknown,
): value is LibraryOverviewDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (!Array.isArray(candidate.stories)) return false;
  const seen = new Set<string>();
  for (const entry of candidate.stories) {
    if (!isStoryCardDto(entry)) return false;
    if (seen.has(entry.id)) return false;
    seen.add(entry.id);
  }
  return true;
}
