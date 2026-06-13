/**
 * Wire contract for the `read_device_library` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/device_library.rs::DeviceLibraryDto`.
 *
 * Shape: tagged enum on `kind` ∈ `"none" | "unsupported" | "readable"`.
 * Every payload field is camelCase. A `DeviceStoryDto` carries only the
 * opaque on-device identity — there is no title, because the device
 * stores none for official packs and Rustory does not consult an external
 * catalog in the MVP. Cross-stack contract tests keep the wire shape
 * symmetric.
 */

import type { UnsupportedReasonDto } from "./device";

export interface DeviceStoryDto {
  /** Canonical lowercase pack UUID (public content identifier). */
  uuid: string;
  /** Uppercase last 8 hex characters — the opaque label shown to the
   *  user and the `.content` folder name. */
  shortId: string;
  /** Listed in `.pi.hidden` rather than `.pi`. */
  hidden: boolean;
  /** A `.content/<shortId>` payload folder exists; `false` flags an
   *  orphan/ambiguous entry. */
  contentPresent: boolean;
  /** A local copy of this pack already exists (provenance link present).
   *  Stamped by RUST — local truth and device truth are composed at the
   *  boundary, never recomposed by the frontend. */
  alreadyImported: boolean;
}

export type DeviceLibraryDto =
  | { kind: "none" }
  | {
      kind: "unsupported";
      reason: UnsupportedReasonDto;
      firmwareHint: string | null;
    }
  | { kind: "readable"; deviceIdentifier: string; stories: DeviceStoryDto[] };

const UNSUPPORTED_REASONS: ReadonlySet<string> = new Set([
  "firmwareUnsupported",
  "metadataUnsupported",
  "metadataCorrupt",
  "familyUnknown",
  "operationNotAuthorized",
  "multipleCandidates",
]);

/** 32 lowercase hex chars — mirrors `compute_device_identifier`. */
const DEVICE_IDENTIFIER_PATTERN = /^[0-9a-f]{32}$/;

const ALLOWED_KEYS: Record<string, ReadonlySet<string>> = {
  none: new Set(["kind"]),
  unsupported: new Set(["kind", "reason", "firmwareHint"]),
  readable: new Set(["kind", "deviceIdentifier", "stories"]),
};

const ALLOWED_STORY_KEYS: ReadonlySet<string> = new Set([
  "uuid",
  "shortId",
  "hidden",
  "contentPresent",
  "alreadyImported",
]);

function hasOnlyAllowedKeys(
  value: Record<string, unknown>,
  allowed: ReadonlySet<string> | undefined,
): boolean {
  if (!allowed) return false;
  for (const k of Object.keys(value)) {
    if (!allowed.has(k)) return false;
  }
  return true;
}

function isDeviceStoryDto(value: unknown): value is DeviceStoryDto {
  if (typeof value !== "object" || value === null) return false;
  const s = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(s, ALLOWED_STORY_KEYS)) return false;
  // Non-empty strings: a blank uuid / shortId is a serializer drift, not
  // a tolerable quirk — the UI keys and labels device entries on them.
  if (typeof s.uuid !== "string" || s.uuid.length === 0) return false;
  if (typeof s.shortId !== "string" || s.shortId.length === 0) return false;
  if (typeof s.hidden !== "boolean") return false;
  if (typeof s.contentPresent !== "boolean") return false;
  if (typeof s.alreadyImported !== "boolean") return false;
  return true;
}

/**
 * Runtime guard for `DeviceLibraryDto`. Rejects every drift: unknown
 * `kind`, missing/extra fields, wrong types, unrecognized enum strings,
 * malformed `deviceIdentifier`, malformed story entries. The UI must
 * never render against an arbitrary object — a drift is a fail-loud bug.
 */
export function isDeviceLibraryDto(value: unknown): value is DeviceLibraryDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.kind !== "string") return false;
  if (!hasOnlyAllowedKeys(c, ALLOWED_KEYS[c.kind])) return false;
  switch (c.kind) {
    case "none":
      return true;
    case "unsupported":
      if (typeof c.reason !== "string" || !UNSUPPORTED_REASONS.has(c.reason))
        return false;
      if (c.firmwareHint !== null && typeof c.firmwareHint !== "string")
        return false;
      return true;
    case "readable":
      if (
        typeof c.deviceIdentifier !== "string" ||
        !DEVICE_IDENTIFIER_PATTERN.test(c.deviceIdentifier)
      )
        return false;
      if (!Array.isArray(c.stories)) return false;
      return c.stories.every(isDeviceStoryDto);
    default:
      return false;
  }
}
