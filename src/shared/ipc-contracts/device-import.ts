/**
 * Wire contract for the `import_device_story` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/device_import.rs`.
 *
 * Input: exactly the two identifiers the frontend legitimately holds —
 * the opaque hashed `deviceIdentifier` from detection and the canonical
 * `packUuid` from the inventory. No path, no short id: Rust re-resolves
 * everything else itself, and the Rust DTO refuses unknown fields.
 *
 * Outcome: the created local story card + the opaque short id + the
 * import timestamp. Cross-stack contract tests keep the shape symmetric.
 */

import type { StoryCardDto } from "./library";

export interface ImportDeviceStoryInput {
  /** 32 lowercase hex chars — mirrors `compute_device_identifier`. */
  deviceIdentifier: string;
  /** Canonical lowercase hyphenated pack UUID (8-4-4-4-12). */
  packUuid: string;
}

export interface ImportDeviceStoryOutcome {
  /** The freshly created local story ("Histoire de ma Lunii (XXXXXXXX)"). */
  story: StoryCardDto;
  /** Uppercase last 8 hex characters of the pack UUID. */
  packShortId: string;
  /** ISO-8601 UTC millisecond timestamp of the import. */
  importedAt: string;
}

const ALLOWED_INPUT_KEYS: ReadonlySet<string> = new Set([
  "deviceIdentifier",
  "packUuid",
]);

const ALLOWED_OUTCOME_KEYS: ReadonlySet<string> = new Set([
  "story",
  "packShortId",
  "importedAt",
]);

const ALLOWED_STORY_KEYS: ReadonlySet<string> = new Set(["id", "title"]);

/** 32 lowercase hex chars — mirrors `compute_device_identifier`. */
const DEVICE_IDENTIFIER_PATTERN = /^[0-9a-f]{32}$/;

/** Canonical lowercase hyphenated UUID (8-4-4-4-12) — the exact shape
 *  `format_pack_uuid` emits for packs and `Uuid::now_v7().to_string()`
 *  emits for local story ids. */
const CANONICAL_UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/** Uppercase 8-hex — the `.content` folder name shape. */
const SHORT_ID_PATTERN = /^[0-9A-F]{8}$/;

/** ISO-8601 UTC at millisecond precision (`YYYY-MM-DDTHH:MM:SS.sssZ`). */
const ISO_UTC_MS_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

function hasOnlyAllowedKeys(
  value: Record<string, unknown>,
  allowed: ReadonlySet<string>,
): boolean {
  for (const k of Object.keys(value)) {
    if (!allowed.has(k)) return false;
  }
  return true;
}

/**
 * Runtime guard for an [`ImportDeviceStoryInput`] payload. Both values
 * originate from Rust itself (detection + inventory DTOs), so a
 * malformed input is a frontend bug — refused client-side BEFORE the
 * IPC round-trip, mirroring the strict Rust-side boundary validation.
 */
export function isImportDeviceStoryInput(
  value: unknown,
): value is ImportDeviceStoryInput {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_INPUT_KEYS)) return false;
  if (
    typeof c.deviceIdentifier !== "string" ||
    !DEVICE_IDENTIFIER_PATTERN.test(c.deviceIdentifier)
  ) {
    return false;
  }
  if (typeof c.packUuid !== "string" || !CANONICAL_UUID_PATTERN.test(c.packUuid)) {
    return false;
  }
  return true;
}

/**
 * Runtime guard for an [`ImportDeviceStoryOutcome`] payload. Rust is
 * authoritative, but the success surface must never render against an
 * arbitrary object — closed keys, canonical story id, exact shortId
 * and timestamp shapes.
 */
export function isImportDeviceStoryOutcome(
  value: unknown,
): value is ImportDeviceStoryOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_OUTCOME_KEYS)) return false;

  if (typeof c.story !== "object" || c.story === null) return false;
  const story = c.story as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(story, ALLOWED_STORY_KEYS)) return false;
  // The created id is always a Rust-generated canonical UUID — the route
  // feeds it to `/story/:id/edit`, so a malformed id is a drift, not a
  // tolerable quirk.
  if (typeof story.id !== "string" || !CANONICAL_UUID_PATTERN.test(story.id)) {
    return false;
  }
  if (typeof story.title !== "string" || story.title.trim().length === 0) {
    return false;
  }

  if (typeof c.packShortId !== "string" || !SHORT_ID_PATTERN.test(c.packShortId)) {
    return false;
  }
  if (typeof c.importedAt !== "string" || !ISO_UTC_MS_PATTERN.test(c.importedAt)) {
    return false;
  }
  return true;
}
