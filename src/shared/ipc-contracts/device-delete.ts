/**
 * Wire contract for the `delete_device_story` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/device_delete.rs`.
 *
 * Input: exactly the two identifiers the frontend legitimately holds (opaque
 * hashed `deviceIdentifier`, canonical `packUuid`). No path, no short id —
 * Rust re-resolves everything itself and refuses unknown fields.
 *
 * Outcome: the deleted pack UUID + whether it was actually present.
 * `wasPresent = false` is an idempotent no-op (a re-issued delete, or a stale
 * selection), NOT an error. Family-neutral: no family/cohort on the wire.
 */

export interface DeleteDeviceStoryInput {
  /** 32 lowercase hex chars — mirrors `compute_device_identifier`. */
  deviceIdentifier: string;
  /** Canonical lowercase hyphenated pack UUID (8-4-4-4-12). */
  packUuid: string;
}

export interface DeleteDeviceStoryOutcome {
  /** The canonical pack UUID the delete targeted. */
  packUuid: string;
  /** `true` when the pack was listed and has been delisted; `false` when it
   *  was already absent (idempotent no-op). */
  wasPresent: boolean;
}

const ALLOWED_INPUT_KEYS: ReadonlySet<string> = new Set([
  "deviceIdentifier",
  "packUuid",
]);

const ALLOWED_OUTCOME_KEYS: ReadonlySet<string> = new Set([
  "packUuid",
  "wasPresent",
]);

const DEVICE_IDENTIFIER_PATTERN = /^[0-9a-f]{32}$/;
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

/** Runtime guard for a [`DeleteDeviceStoryInput`] — refused client-side
 *  BEFORE the round-trip, mirroring the strict Rust-side validation. */
export function isDeleteDeviceStoryInput(
  value: unknown,
): value is DeleteDeviceStoryInput {
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

/** Runtime guard for a [`DeleteDeviceStoryOutcome`] — closed keys, canonical
 *  UUID, boolean flag. */
export function isDeleteDeviceStoryOutcome(
  value: unknown,
): value is DeleteDeviceStoryOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_OUTCOME_KEYS)) return false;
  if (typeof c.packUuid !== "string" || !CANONICAL_UUID_PATTERN.test(c.packUuid)) {
    return false;
  }
  if (typeof c.wasPresent !== "boolean") return false;
  return true;
}
