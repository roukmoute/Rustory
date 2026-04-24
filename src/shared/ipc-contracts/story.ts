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
 *  mandates the `Z` suffix; any other representation (including the
 *  semantically-equivalent `+00:00` offset) is refused so Rust stays
 *  the single source for the canonical wire shape — a drift in the
 *  Rust serializer must fail loudly here, not be quietly accommodated. */
const ISO8601_UTC_PATTERN =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/;

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

export type { StoryCardDto };
