/**
 * Wire contract for the `read_connected_lunii` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/device.rs::ConnectedDeviceDto`.
 *
 * Shape: tagged enum on `kind` ∈ `"none" | "supported" | "unsupported"
 * | "ambiguous"`. Every payload field is camelCase; `firmwareCohort`
 * and `reason` use camelCase string enums (Rust serde
 * `rename_all = "camelCase"`). Cross-stack contract tests keep the wire
 * shape symmetric.
 */

export type SupportedFamilyDto = "lunii" | "flam";

export type FirmwareCohortDto = "origineV1" | "midGenV2" | "v3" | "flamGen1";

export interface SupportedOperationsDto {
  readLibrary: boolean;
  inspectStory: boolean;
  importStory: boolean;
  writeStory: boolean;
  /** Delete a story already on the device. Gated separately from
   *  `writeStory`: deletion removes opaque bytes and needs no
   *  pack-format ciphering, so Lunii V3 may delete even while it may
   *  not (yet) be written to. */
  deleteStory: boolean;
  /** Send a STUdio-format pack archive (`.zip`) to the device. Gated
   *  separately from `writeStory` (the round-trip of an imported pack):
   *  the archive-send owns its whole V3 pipeline, so Lunii V3 may
   *  receive archives while the round-trip stays closed. */
  sendArchive: boolean;
}

export type UnsupportedReasonDto =
  | "firmwareUnsupported"
  | "metadataUnsupported"
  | "metadataCorrupt"
  | "familyUnknown"
  | "operationNotAuthorized"
  | "multipleCandidates";

export type ConnectedDeviceDto =
  | { kind: "none" }
  | {
      kind: "supported";
      family: SupportedFamilyDto;
      firmwareCohort: FirmwareCohortDto;
      /** Present for families whose primary marker carries a version
       *  byte (Lunii). ABSENT — the key itself, never `null` — for
       *  families without one (FLAM): the Rust serializer omits it. */
      metadataFormatVersion?: number;
      deviceIdentifier: string;
      supportedOperations: SupportedOperationsDto;
    }
  | {
      kind: "unsupported";
      reason: UnsupportedReasonDto;
      firmwareHint: string | null;
    }
  | { kind: "ambiguous"; candidateCount: number };

/** Closed per-family contract: which cohorts are legal for the family
 *  and whether `metadataFormatVersion` must be PRESENT (an integer in
 *  0..127) or ABSENT (the key itself — JSON has no `undefined`, and
 *  `null` is refused). Replaces the former independent family/cohort
 *  sets: an illegal combination (`lunii`+`flamGen1`, `flam` carrying a
 *  version, …) is unrepresentable at the boundary. */
const FAMILY_CONTRACTS: Record<
  string,
  { cohorts: ReadonlySet<string>; metadataFormatVersion: "required" | "absent" }
> = {
  lunii: {
    cohorts: new Set(["origineV1", "midGenV2", "v3"]),
    metadataFormatVersion: "required",
  },
  flam: {
    cohorts: new Set(["flamGen1"]),
    metadataFormatVersion: "absent",
  },
};
const UNSUPPORTED_REASONS: ReadonlySet<string> = new Set([
  "firmwareUnsupported",
  "metadataUnsupported",
  "metadataCorrupt",
  "familyUnknown",
  "operationNotAuthorized",
  "multipleCandidates",
]);

function isSupportedOperationsDto(
  value: unknown,
): value is SupportedOperationsDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  return (
    typeof c.readLibrary === "boolean" &&
    typeof c.inspectStory === "boolean" &&
    typeof c.importStory === "boolean" &&
    typeof c.writeStory === "boolean" &&
    typeof c.deleteStory === "boolean" &&
    typeof c.sendArchive === "boolean"
  );
}

/** 32 lowercase hex chars — must mirror exactly what the Rust core
 *  emits via `compute_device_identifier`. A non-matching string is a
 *  protocol drift, not a transient quirk. */
const DEVICE_IDENTIFIER_PATTERN = /^[0-9a-f]{32}$/;

/** Closed set of permitted property names per `kind`. Used to refuse
 *  payloads that carry extra fields — a drifted Rust serializer
 *  must fail the guard loudly, not be quietly tolerated. */
const ALLOWED_KEYS: Record<string, ReadonlySet<string>> = {
  none: new Set(["kind"]),
  supported: new Set([
    "kind",
    "family",
    "firmwareCohort",
    "metadataFormatVersion",
    "deviceIdentifier",
    "supportedOperations",
  ]),
  unsupported: new Set(["kind", "reason", "firmwareHint"]),
  ambiguous: new Set(["kind", "candidateCount"]),
};

/** Prototype-safe own-property lookup. A plain indexation
 *  (`record[key]`) walks the prototype chain: a hostile discriminant
 *  such as `"constructor"` or `"__proto__"` would resolve to a truthy
 *  `Object.prototype` member and crash the guard with a `TypeError`
 *  instead of a boolean rejection. */
function ownEntry<T>(record: Record<string, T>, key: string): T | undefined {
  return Object.prototype.hasOwnProperty.call(record, key)
    ? record[key]
    : undefined;
}

function hasOnlyAllowedKeys(
  value: Record<string, unknown>,
  kind: string,
): boolean {
  const allowed = ownEntry(ALLOWED_KEYS, kind);
  if (!allowed) return false;
  for (const k of Object.keys(value)) {
    if (!allowed.has(k)) return false;
  }
  return true;
}

/**
 * Runtime guard for `ConnectedDeviceDto`. Rejects every drift: unknown
 * `kind`, missing fields, extra fields, wrong types, unrecognized enum
 * strings, and malformed `deviceIdentifier` payloads. The UI must
 * never render against an arbitrary object — a drift is a fail-loud
 * bug, not a silent fallback.
 */
export function isConnectedDeviceDto(
  value: unknown,
): value is ConnectedDeviceDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.kind !== "string") return false;
  if (!hasOnlyAllowedKeys(c, c.kind)) return false;
  switch (c.kind) {
    case "none":
      return true;
    case "supported": {
      if (typeof c.family !== "string") return false;
      const contract = ownEntry(FAMILY_CONTRACTS, c.family);
      if (!contract) return false;
      if (
        typeof c.firmwareCohort !== "string" ||
        !contract.cohorts.has(c.firmwareCohort)
      )
        return false;
      if (contract.metadataFormatVersion === "required") {
        if (
          typeof c.metadataFormatVersion !== "number" ||
          !Number.isInteger(c.metadataFormatVersion) ||
          c.metadataFormatVersion < 0 ||
          c.metadataFormatVersion > 127
        )
          return false;
      } else {
        // ABSENT means the KEY is absent: a present key — even
        // `null`/`undefined`-valued — is a producer drift, refused.
        if ("metadataFormatVersion" in c) return false;
      }
      if (
        typeof c.deviceIdentifier !== "string" ||
        !DEVICE_IDENTIFIER_PATTERN.test(c.deviceIdentifier)
      )
        return false;
      if (!isSupportedOperationsDto(c.supportedOperations)) return false;
      return true;
    }
    case "unsupported":
      if (
        typeof c.reason !== "string" ||
        !UNSUPPORTED_REASONS.has(c.reason)
      )
        return false;
      if (c.firmwareHint !== null && typeof c.firmwareHint !== "string")
        return false;
      return true;
    case "ambiguous":
      return (
        typeof c.candidateCount === "number" &&
        Number.isInteger(c.candidateCount) &&
        c.candidateCount >= 2
      );
    default:
      return false;
  }
}
