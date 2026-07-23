/**
 * Wire contract for the `send_pack_to_device` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/device_send.rs`.
 *
 * Input: EXACTLY the two identifiers the frontend legitimately holds — the
 * opaque hashed `deviceIdentifier` and the local `storyId` selected in the
 * library. Rust resolves the story's RETAINED source archive itself (by
 * convention from the story id), so no path ever crosses the IPC boundary; it
 * refuses unknown fields. This is the V3 branch of the SINGLE "Envoyer vers la
 * Lunii" gesture — there is no file picker.
 *
 * Outcome: the pack facts the UI echoes. Family-neutral (no family/cohort on
 * the wire). There is no `cancelled` variant — the gesture is a single CTA
 * click, like the delete flow.
 */

export interface SendPackToDeviceInput {
  /** 32 lowercase hex chars — mirrors `compute_device_identifier`. */
  deviceIdentifier: string;
  /** The selected local story id (canonical lowercase UUID). */
  storyId: string;
}

export interface SendPackToDeviceOutcome {
  /** Canonical lowercase hyphenated pack UUID (8-4-4-4-12). */
  packUuid: string;
  /** Distinct image assets written with the pack. */
  imageCount: number;
  /** Distinct audio assets written with the pack. */
  audioCount: number;
}

const ALLOWED_INPUT_KEYS: ReadonlySet<string> = new Set([
  "deviceIdentifier",
  "storyId",
]);

const ALLOWED_OUTCOME_KEYS: ReadonlySet<string> = new Set([
  "packUuid",
  "imageCount",
  "audioCount",
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

/** Runtime guard for a [`SendPackToDeviceInput`] — refused client-side
 *  BEFORE the round-trip, mirroring the strict Rust-side validation. */
export function isSendPackToDeviceInput(
  value: unknown,
): value is SendPackToDeviceInput {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_INPUT_KEYS)) return false;
  if (
    typeof c.deviceIdentifier !== "string" ||
    !DEVICE_IDENTIFIER_PATTERN.test(c.deviceIdentifier)
  ) {
    return false;
  }
  return typeof c.storyId === "string" && CANONICAL_UUID_PATTERN.test(c.storyId);
}

/** Runtime guard for a [`SendPackToDeviceOutcome`] — closed keys, canonical
 *  UUID and non-negative integer counts. */
export function isSendPackToDeviceOutcome(
  value: unknown,
): value is SendPackToDeviceOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, ALLOWED_OUTCOME_KEYS)) return false;
  if (typeof c.packUuid !== "string" || !CANONICAL_UUID_PATTERN.test(c.packUuid)) {
    return false;
  }
  return (
    typeof c.imageCount === "number" &&
    Number.isInteger(c.imageCount) &&
    c.imageCount >= 0 &&
    typeof c.audioCount === "number" &&
    Number.isInteger(c.audioCount) &&
    c.audioCount >= 0
  );
}
