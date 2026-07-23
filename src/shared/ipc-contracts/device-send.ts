/**
 * Wire contract for the `send_pack_to_device` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/device_send.rs`.
 *
 * Input: EXACTLY the one identifier the frontend legitimately holds (opaque
 * hashed `deviceIdentifier`). The source archive is picked in a NATIVE dialog
 * owned by Rust — no path ever crosses the IPC boundary in either direction,
 * and Rust refuses unknown fields.
 *
 * Outcome: tagged on `kind`. A dismissed native dialog is `cancelled` (a
 * non-event, never an error — the catalog-import pattern); a completed write
 * is `sent` with the pack facts the UI echoes. Family-neutral: no
 * family/cohort on the wire.
 */

export interface SendPackToDeviceInput {
  /** 32 lowercase hex chars — mirrors `compute_device_identifier`. */
  deviceIdentifier: string;
}

export type SendPackToDeviceOutcome =
  | { kind: "cancelled" }
  | {
      kind: "sent";
      /** Canonical lowercase hyphenated pack UUID (8-4-4-4-12). */
      packUuid: string;
      /** Distinct image assets written with the pack. */
      imageCount: number;
      /** Distinct audio assets written with the pack. */
      audioCount: number;
    };

const ALLOWED_INPUT_KEYS: ReadonlySet<string> = new Set(["deviceIdentifier"]);

const ALLOWED_OUTCOME_KEYS: Record<string, ReadonlySet<string>> = {
  cancelled: new Set(["kind"]),
  sent: new Set(["kind", "packUuid", "imageCount", "audioCount"]),
};

const DEVICE_IDENTIFIER_PATTERN = /^[0-9a-f]{32}$/;
const CANONICAL_UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/** Prototype-safe own-property lookup (a hostile `kind` such as
 *  `"constructor"` must resolve to a boolean rejection, never a crash). */
function ownEntry<T>(record: Record<string, T>, key: string): T | undefined {
  return Object.prototype.hasOwnProperty.call(record, key)
    ? record[key]
    : undefined;
}

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
  return (
    typeof c.deviceIdentifier === "string" &&
    DEVICE_IDENTIFIER_PATTERN.test(c.deviceIdentifier)
  );
}

/** Runtime guard for a [`SendPackToDeviceOutcome`] — closed kinds, closed
 *  keys per kind, canonical UUID and non-negative integer counts. */
export function isSendPackToDeviceOutcome(
  value: unknown,
): value is SendPackToDeviceOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.kind !== "string") return false;
  const allowed = ownEntry(ALLOWED_OUTCOME_KEYS, c.kind);
  if (!allowed || !hasOnlyAllowedKeys(c, allowed)) return false;
  switch (c.kind) {
    case "cancelled":
      return true;
    case "sent":
      if (
        typeof c.packUuid !== "string" ||
        !CANONICAL_UUID_PATTERN.test(c.packUuid)
      ) {
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
    default:
      return false;
  }
}
