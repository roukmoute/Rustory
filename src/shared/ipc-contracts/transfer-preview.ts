/**
 * Wire contract for the `read_transfer_preview` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/transfer_preview.rs::TransferPreviewDto`.
 *
 * Shape: tagged enum on `kind` ∈ `"noDevice" | "unsupported" | "ready"`.
 * Every payload field is camelCase. The comparison (`onDevice`,
 * `unchangedCount`) is composed by RUST from the live device inventory and
 * the `story_imports` join — the frontend only PRESENTS it. No size metric is
 * carried (no decisional volume before media preparation). Cross-stack
 * contract tests keep the wire shape symmetric.
 */

import type { UnsupportedReasonDto } from "./device";

/** The selected local story identified in the comparison. */
export interface TransferPreviewStory {
  /** Local story id (canonical lowercase UUID). */
  id: string;
  /** The local `stories.title` — the user owns this story, no recognition. */
  title: string;
}

export type TransferPreviewDto =
  | { kind: "noDevice" }
  | { kind: "unsupported"; reason: UnsupportedReasonDto }
  | {
      kind: "ready";
      deviceIdentifier: string;
      story: TransferPreviewStory;
      /** The selected story's pack already lives on the device — a send would
       *  REPLACE it. `false` ⇒ a send would ADD it. */
      onDevice: boolean;
      /** How many other device stories a send would leave untouched. */
      unchangedCount: number;
      /** Whether a transfer is allowed (`WriteStory`). Always `false` in MVP
       *  Phase 1 — the preview is read-only. */
      transferable: boolean;
    };

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

/** Canonical lowercase UUID (8-4-4-4-12) — the local story id shape. */
const STORY_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

// A Map (not a plain object): `.get(kind)` returns `undefined` for an unknown
// OR inherited key (`"constructor"`, `"toString"`, …). A plain-object lookup
// would resolve those to an Object.prototype value and make `hasOnlyAllowedKeys`
// throw on `allowed.has` instead of reporting contract drift.
const ALLOWED_KEYS: ReadonlyMap<string, ReadonlySet<string>> = new Map([
  ["noDevice", new Set(["kind"])],
  ["unsupported", new Set(["kind", "reason"])],
  [
    "ready",
    new Set([
      "kind",
      "deviceIdentifier",
      "story",
      "onDevice",
      "unchangedCount",
      "transferable",
    ]),
  ],
]);

const ALLOWED_STORY_KEYS: ReadonlySet<string> = new Set(["id", "title"]);

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

function isTransferPreviewStory(value: unknown): value is TransferPreviewStory {
  if (typeof value !== "object" || value === null) return false;
  const s = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(s, ALLOWED_STORY_KEYS)) return false;
  if (typeof s.id !== "string" || !STORY_ID_PATTERN.test(s.id)) return false;
  // A blank title is a serializer drift — the comparison labels the story.
  if (typeof s.title !== "string" || s.title.length === 0) return false;
  return true;
}

/**
 * Runtime guard for `TransferPreviewDto`. Rejects every drift: unknown
 * `kind`, missing/extra fields, wrong types, unrecognized enum strings,
 * malformed `deviceIdentifier` / `story.id`, non-integer `unchangedCount`.
 * The UI must never render against an arbitrary object — a drift is a
 * fail-loud bug.
 */
export function isTransferPreviewDto(
  value: unknown,
): value is TransferPreviewDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.kind !== "string") return false;
  if (!hasOnlyAllowedKeys(c, ALLOWED_KEYS.get(c.kind))) return false;
  switch (c.kind) {
    case "noDevice":
      return true;
    case "unsupported":
      return typeof c.reason === "string" && UNSUPPORTED_REASONS.has(c.reason);
    case "ready":
      if (
        typeof c.deviceIdentifier !== "string" ||
        !DEVICE_IDENTIFIER_PATTERN.test(c.deviceIdentifier)
      )
        return false;
      if (!isTransferPreviewStory(c.story)) return false;
      if (typeof c.onDevice !== "boolean") return false;
      if (
        typeof c.unchangedCount !== "number" ||
        !Number.isInteger(c.unchangedCount) ||
        c.unchangedCount < 0
      )
        return false;
      if (typeof c.transferable !== "boolean") return false;
      return true;
    default:
      return false;
  }
}
