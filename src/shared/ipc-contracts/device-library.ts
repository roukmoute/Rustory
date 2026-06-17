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

/** Provenance of a recognized device-story title. Mirrors the Rust
 *  `PackTitleSource`; the UI uses it to show "officiel / non-officiel /
 *  saisi" and to NEVER present a user/community title as official. */
export type PackTitleSource = "user" | "official" | "unofficial";

export interface DeviceStoryDto {
  /** Canonical lowercase pack UUID (public content identifier). */
  uuid: string;
  /** Uppercase last 8 hex characters — the `.content` folder name and the
   *  fallback label shown when the pack is not recognized. */
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
  /** Recognized title, or `null` when no index covers this pack ("non
   *  reconnue"). Composed by RUST from the local UUID→title index. */
  title: string | null;
  /** Provenance of `title`. `null` exactly when `title` is `null`. */
  titleSource: PackTitleSource | null;
  /** Presence flag for a cached cover: an OPAQUE local cache reference (a
   *  file name), or `null` when there is none. NEVER a remote URL and NEVER
   *  rendered directly — the UI loads the image via the `read_pack_cover`
   *  command (a local read returning a `data:` URL), keeping offline-first.
   *  `null` for user / local-library titles and for unrecognized packs. */
  thumbnail: string | null;
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
  "title",
  "titleSource",
  "thumbnail",
]);

const TITLE_SOURCES: ReadonlySet<string> = new Set([
  "user",
  "official",
  "unofficial",
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

  // Recognition fields. `title` is either null (unrecognized) or a
  // non-empty string; `titleSource` must be null exactly when `title` is
  // null (the coupling Rust guarantees), and otherwise a known token. A
  // cover may only ride along a recognized title.
  const hasTitle = typeof s.title === "string" && s.title.length > 0;
  if (!hasTitle && s.title !== null) return false;
  if (hasTitle) {
    if (typeof s.titleSource !== "string" || !TITLE_SOURCES.has(s.titleSource)) {
      return false;
    }
    if (s.thumbnail !== null && (typeof s.thumbnail !== "string" || s.thumbnail.length === 0)) {
      return false;
    }
  } else {
    if (s.titleSource !== null) return false;
    if (s.thumbnail !== null) return false;
  }
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
