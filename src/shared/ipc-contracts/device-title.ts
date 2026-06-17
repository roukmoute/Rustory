/**
 * Wire contract for the `set_device_story_title` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/device_title.rs`.
 *
 * Input: the canonical `packUuid` the frontend holds from the inventory and
 * the raw title text. Rust normalizes (NFC + trim), validates (denylist +
 * ≤120) and persists it; provenance is Rust-owned (`user`), so it is NOT a
 * field of the input.
 *
 * Outcome: the stored title and its provenance (always `user`).
 */

import type { PackTitleSource } from "./device-library";

export interface SetDeviceStoryTitleInput {
  /** Canonical lowercase hyphenated pack UUID (8-4-4-4-12). */
  packUuid: string;
  /** Raw title text; Rust normalizes and validates it authoritatively. */
  title: string;
}

export interface DeviceStoryTitleDto {
  title: string;
  source: PackTitleSource;
}

const ALLOWED_INPUT_KEYS: ReadonlySet<string> = new Set(["packUuid", "title"]);
const ALLOWED_OUTCOME_KEYS: ReadonlySet<string> = new Set(["title", "source"]);
const TITLE_SOURCES: ReadonlySet<string> = new Set([
  "user",
  "official",
  "unofficial",
]);

/** Canonical lowercase hyphenated UUID (8-4-4-4-12). */
const CANONICAL_UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

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
 * Client-side guard for the input. The `packUuid` is a Rust-issued
 * identifier, so a malformed value is a frontend bug — refused before the
 * round-trip. The `title` is only checked for being a non-blank string
 * here; Rust owns the authoritative normalization + validation (denylist,
 * length) and returns the canonical refusal on violation.
 */
export function isSetDeviceStoryTitleInput(
  value: unknown,
): value is SetDeviceStoryTitleInput {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_INPUT_KEYS)) return false;
  if (typeof c.packUuid !== "string" || !CANONICAL_UUID_PATTERN.test(c.packUuid)) {
    return false;
  }
  if (typeof c.title !== "string" || c.title.trim().length === 0) return false;
  return true;
}

/** Runtime guard for the `set_device_story_title` outcome. */
export function isDeviceStoryTitleDto(
  value: unknown,
): value is DeviceStoryTitleDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_OUTCOME_KEYS)) return false;
  if (typeof c.title !== "string" || c.title.length === 0) return false;
  if (typeof c.source !== "string" || !TITLE_SOURCES.has(c.source)) return false;
  return true;
}
