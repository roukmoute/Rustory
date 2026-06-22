/**
 * Wire contract for the `read_story_validation` Tauri command. Mirror of
 * `src-tauri/src/ipc/dto/story_validation.rs::StoryValidationDto`.
 *
 * Shape: tagged enum on `kind` ∈ `"noDevice" | "ready"`. Every payload field is
 * camelCase. The verdict (`presumedTransferable` / `toFix` / `blocked`) and the
 * closed `axis × cause` blocker taxonomy are composed by RUST — the frontend
 * only PRESENTS them, never recomposes a verdict. There is no `unsupported`
 * variant: an unreadable / ambiguous / unsupported device profile is a
 * `deviceProfile` blocker inside `ready`, so the canonical axis stays visible
 * alongside (AC1). Cross-stack contract tests keep the wire shape symmetric.
 */

/** The composed validation verdict (AC1/AC3). */
export type ValidationVerdict = "presumedTransferable" | "toFix" | "blocked";

/** The two-axis taxonomy (AC1): canonical validity vs Lunii compatibility. */
export type BlockerAxis = "structure" | "media" | "filesystem" | "deviceProfile";

/** Closed set of blocker causes spanning both axes (AC2). */
export type BlockerCause =
  | "titleInvalid"
  | "schemaUnsupported"
  | "structureCorrupt"
  | "checksumMismatch"
  | "metadataUnsupported"
  | "metadataCorrupt"
  | "familyUnknown"
  | "multipleCandidates"
  | "firmwareUnsupported"
  | "operationNotAuthorized";

/** The selected local story identified in the verdict. */
export interface StoryValidationStory {
  /** Local story id (canonical lowercase UUID). */
  id: string;
  /** The local `stories.title` — the user owns this story, no recognition. */
  title: string;
}

/** A single blocker (AC2): a closed `(axis, cause)` pair plus the canonical FR
 *  `message` (cause + impact) and `userAction` (next gesture), both rendered
 *  verbatim by the UI. */
export interface ValidationBlocker {
  axis: BlockerAxis;
  cause: BlockerCause;
  message: string;
  userAction: string;
}

export type StoryValidationDto =
  | { kind: "noDevice" }
  | {
      kind: "ready";
      deviceIdentifier: string;
      story: StoryValidationStory;
      verdict: ValidationVerdict;
      blockers: ValidationBlocker[];
    };

const VERDICTS: ReadonlySet<string> = new Set([
  "presumedTransferable",
  "toFix",
  "blocked",
]);

// Closed `axis → causes` map: the taxonomy is a closed set of PAIRS, never an
// independent axis and cause. Validating them separately would accept impossible
// couples (e.g. `deviceProfile` + `checksumMismatch`) that could be grouped under
// the wrong axis. `media` / `filesystem` are declared axes with no cause yet, so
// no blocker can legitimately carry them. Mirror of the Rust
// `axis_dto` × `cause_copy` mapping (Rust is authoritative).
const CAUSES_BY_AXIS: ReadonlyMap<string, ReadonlySet<string>> = new Map([
  [
    "structure",
    new Set([
      "titleInvalid",
      "schemaUnsupported",
      "structureCorrupt",
      "checksumMismatch",
    ]),
  ],
  ["media", new Set<string>()],
  ["filesystem", new Set<string>()],
  [
    "deviceProfile",
    new Set([
      "metadataUnsupported",
      "metadataCorrupt",
      "familyUnknown",
      "multipleCandidates",
      "firmwareUnsupported",
      "operationNotAuthorized",
    ]),
  ],
]);

// Severity is a FIXED property of the cause in the Rust closed taxonomy (mirror
// for drift detection — Rust is authoritative). In MVP Phase 1 `titleInvalid` is
// the ONLY fixable cause; every other cause is blocking. A cause that is not
// listed here is treated as blocking by construction (it must already be a known
// cause to reach this check).
const FIXABLE_CAUSES: ReadonlySet<string> = new Set(["titleInvalid"]);

/** 32 lowercase hex chars — mirrors `compute_device_identifier`. */
const DEVICE_IDENTIFIER_PATTERN = /^[0-9a-f]{32}$/;

/** Canonical lowercase UUID (8-4-4-4-12) — the local story id shape. */
const STORY_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

// A Map (not a plain object): `.get(kind)` returns `undefined` for an unknown OR
// inherited key (`"constructor"`, `"toString"`, …). A plain-object lookup would
// resolve those to an Object.prototype value and make `hasOnlyAllowedKeys` throw
// on `allowed.has` instead of reporting contract drift.
const ALLOWED_KEYS: ReadonlyMap<string, ReadonlySet<string>> = new Map([
  ["noDevice", new Set(["kind"])],
  [
    "ready",
    new Set(["kind", "deviceIdentifier", "story", "verdict", "blockers"]),
  ],
]);

const ALLOWED_STORY_KEYS: ReadonlySet<string> = new Set(["id", "title"]);
const ALLOWED_BLOCKER_KEYS: ReadonlySet<string> = new Set([
  "axis",
  "cause",
  "message",
  "userAction",
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

function isStoryValidationStory(
  value: unknown,
): value is StoryValidationStory {
  if (typeof value !== "object" || value === null) return false;
  const s = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(s, ALLOWED_STORY_KEYS)) return false;
  if (typeof s.id !== "string" || !STORY_ID_PATTERN.test(s.id)) return false;
  // A blank title is a serializer drift — the verdict labels the story.
  if (typeof s.title !== "string" || s.title.length === 0) return false;
  return true;
}

export function isValidationBlocker(value: unknown): value is ValidationBlocker {
  if (typeof value !== "object" || value === null) return false;
  const b = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(b, ALLOWED_BLOCKER_KEYS)) return false;
  if (typeof b.axis !== "string") return false;
  // Validate the (axis, cause) PAIR against the closed taxonomy, not the axis
  // and cause independently: an impossible couple (wrong axis for a cause) is a
  // drift that could group a blocker under the wrong heading.
  const allowedCauses = CAUSES_BY_AXIS.get(b.axis);
  if (!allowedCauses) return false;
  if (typeof b.cause !== "string" || !allowedCauses.has(b.cause)) return false;
  // Both strings are Rust-authoritative and rendered verbatim — an empty one is
  // a drift (an opaque blocker with no cause text / no next gesture).
  if (typeof b.message !== "string" || b.message.length === 0) return false;
  if (typeof b.userAction !== "string" || b.userAction.length === 0)
    return false;
  return true;
}

/**
 * The verdict is DERIVED in Rust from the blockers (blocking > fixable > none).
 * The frontend re-checks that coherence so an IPC drift can never paint a verdict
 * that contradicts its blockers (e.g. `Bloquée` with no blocker, or
 * `Présumée transférable` carrying one). Severity is read from the closed
 * taxonomy mirror (`FIXABLE_CAUSES`). Callers pass only blockers that already
 * passed {@link isValidationBlocker}.
 */
function verdictMatchesBlockers(
  verdict: string,
  blockers: ValidationBlocker[],
): boolean {
  const hasBlocking = blockers.some((b) => !FIXABLE_CAUSES.has(b.cause));
  const hasFixable = blockers.some((b) => FIXABLE_CAUSES.has(b.cause));
  switch (verdict) {
    case "presumedTransferable":
      return blockers.length === 0;
    case "toFix":
      return hasFixable && !hasBlocking;
    case "blocked":
      return hasBlocking;
    default:
      return false;
  }
}

/**
 * Runtime guard for `StoryValidationDto`. Rejects every drift: unknown `kind`,
 * missing/extra fields, wrong types, unrecognized `verdict` / `axis` / `cause`
 * strings, malformed `deviceIdentifier` / `story.id`, empty blocker copy. The UI
 * must never render against an arbitrary object — a drift is a fail-loud bug.
 */
export function isStoryValidationDto(
  value: unknown,
): value is StoryValidationDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.kind !== "string") return false;
  if (!hasOnlyAllowedKeys(c, ALLOWED_KEYS.get(c.kind))) return false;
  switch (c.kind) {
    case "noDevice":
      return true;
    case "ready":
      if (
        typeof c.deviceIdentifier !== "string" ||
        !DEVICE_IDENTIFIER_PATTERN.test(c.deviceIdentifier)
      )
        return false;
      if (!isStoryValidationStory(c.story)) return false;
      if (typeof c.verdict !== "string" || !VERDICTS.has(c.verdict))
        return false;
      if (!Array.isArray(c.blockers)) return false;
      if (!c.blockers.every(isValidationBlocker)) return false;
      // The verdict must be coherent with its blockers (derived in Rust); reject
      // a drift that would show a verdict contradicting the blocker list.
      if (!verdictMatchesBlockers(c.verdict, c.blockers as ValidationBlocker[]))
        return false;
      return true;
    default:
      return false;
  }
}
