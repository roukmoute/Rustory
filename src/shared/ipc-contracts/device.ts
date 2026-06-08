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

export type SupportedFamilyDto = "lunii";

export type FirmwareCohortDto = "origineV1" | "midGenV2" | "v3";

export interface SupportedOperationsDto {
  readLibrary: boolean;
  inspectStory: boolean;
  importStory: boolean;
  writeStory: boolean;
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
      metadataFormatVersion: number;
      deviceIdentifier: string;
      supportedOperations: SupportedOperationsDto;
    }
  | {
      kind: "unsupported";
      reason: UnsupportedReasonDto;
      firmwareHint: string | null;
    }
  | { kind: "ambiguous"; candidateCount: number };

const SUPPORTED_FAMILIES: ReadonlySet<string> = new Set(["lunii"]);
const FIRMWARE_COHORTS: ReadonlySet<string> = new Set([
  "origineV1",
  "midGenV2",
  "v3",
]);
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
    typeof c.writeStory === "boolean"
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

function hasOnlyAllowedKeys(
  value: Record<string, unknown>,
  kind: string,
): boolean {
  const allowed = ALLOWED_KEYS[kind];
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
    case "supported":
      if (typeof c.family !== "string" || !SUPPORTED_FAMILIES.has(c.family))
        return false;
      if (
        typeof c.firmwareCohort !== "string" ||
        !FIRMWARE_COHORTS.has(c.firmwareCohort)
      )
        return false;
      if (
        typeof c.metadataFormatVersion !== "number" ||
        !Number.isInteger(c.metadataFormatVersion) ||
        c.metadataFormatVersion < 0 ||
        c.metadataFormatVersion > 127
      )
        return false;
      if (
        typeof c.deviceIdentifier !== "string" ||
        !DEVICE_IDENTIFIER_PATTERN.test(c.deviceIdentifier)
      )
        return false;
      if (!isSupportedOperationsDto(c.supportedOperations)) return false;
      return true;
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
