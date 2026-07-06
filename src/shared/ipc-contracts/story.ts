import type { StoryCardDto } from "./library";

/**
 * Wire contract for the `create_story` Tauri command input. Mirror of
 * `src-tauri/src/ipc/dto/story.rs::CreateStoryInputDto`. The Rust side
 * enforces `deny_unknown_fields`, so extending this interface without a
 * matching Rust change will fail deserialization at runtime.
 */
export interface CreateStoryInput {
  title: string;
}

/**
 * Wire contract for the `update_story` Tauri command input. Mirror of
 * `src-tauri/src/ipc/dto/story.rs::UpdateStoryInputDto`.
 */
export interface UpdateStoryInput {
  id: string;
  title: string;
}

/**
 * Durable `.rustory` import review state as carried on the story detail and
 * on every write acknowledgement. `blocked` is never persisted, so it never
 * appears on this wire; `resolved` is written by the write-path review
 * resolution only. `null` = nothing to carry (a native story, a device
 * pack, or any non-full edit scope).
 */
export type StoryImportState =
  | "recognized"
  | "partial"
  | "needsReview"
  | "resolved";

const STORY_IMPORT_STATES: readonly StoryImportState[] = [
  "recognized",
  "partial",
  "needsReview",
  "resolved",
];

/** The story's declared edit scope (FR21), derived in Rust only: `full` =
 *  the complete editor (a native story or a `.rustory` import); `titleOnly`
 *  = a device pack (only the title, a local metadata, is editable). */
export type StoryEditScope = "full" | "titleOnly";

/**
 * The `importState` key of a detail / acknowledgement payload: REQUIRED
 * (explicit `null`, never absent) and drawn from the closed persisted set.
 * `undefined` (a missing key) is drift, not something to accommodate.
 */
function isImportStateKey(value: unknown): value is StoryImportState | null {
  return (
    value === null || STORY_IMPORT_STATES.includes(value as StoryImportState)
  );
}

/**
 * Wire contract returned by `update_story` (and `apply_recovery`). Mirror
 * of `UpdateStoryOutputDto`. `updatedAt` is the ISO-8601 UTC millisecond
 * timestamp the Rust core committed. `importState` is the durable review
 * state read POST-UPDATE in the same transaction (`null` unless the story
 * carries the full edit scope) — a REQUIRED key, so the review chip
 * reconciles from the same truth as the detail.
 */
export interface UpdateStoryOutput {
  id: string;
  title: string;
  updatedAt: string;
  importState: StoryImportState | null;
}

/**
 * A resolved node media slot. Mirror of `NodeMediaSlotDto`. `state` is
 * `ready` (the bytes are present) or `attention` (a dangling reference —
 * repairable; the rest of the node stays editable). `format` / `byteSize`
 * are present only when `ready`.
 */
export interface NodeMediaSlot {
  assetId: string;
  mediaType: "image" | "audio";
  state: "ready" | "attention";
  format?: string;
  byteSize?: number;
}

/**
 * The current node PROJECTED BY RUST. Mirror of `NodeContentDto`. The UI
 * consumes this and never recomposes a node from `structureJson`.
 */
export interface NodeContentDto {
  id: string;
  text: string;
  label: string;
  image: NodeMediaSlot | null;
  audio: NodeMediaSlot | null;
}

/**
 * One option of a projected graph node. Mirror of `OptionLinkDto`. `state`
 * is DERIVED BY RUST — never re-derive it from `target` on the frontend.
 * Truth table (enforced by `isOptionLink`):
 * `unlinked` ⟺ `target = null`; `linked` / `broken` ⟺ `target` is a node id
 * (`linked` = present in the graph, `broken` = absent — rendered as
 * `destination à corriger`).
 */
export interface OptionLink {
  label: string;
  target: string | null;
  state: "unlinked" | "linked" | "broken";
}

/**
 * One node of the projected graph. Mirror of `NodeGraphDto`. `hasIssue` is
 * Rust-derived (at least one broken option link) — localized on the node,
 * never hiding the rest of the list.
 */
export interface NodeGraph {
  id: string;
  label: string;
  isStart: boolean;
  hasIssue: boolean;
  options: OptionLink[];
}

/**
 * The story's node graph, projected LIGHT for the structure navigator.
 * Mirror of `StoryStructureDto`. Node order = display / navigation order.
 */
export interface StoryStructure {
  startNodeId: string;
  nodes: NodeGraph[];
}

/**
 * Full wire projection of a single story for the edit surface. Mirror of
 * `src-tauri/src/ipc/dto/story.rs::StoryDetailDto`. `structureJson` is the
 * exact byte sequence covered by `contentChecksum` — never reserialize or
 * reformat it on the frontend. `editScope` is the story's DECLARED edit
 * scope (FR21) and `editable` its derived compatibility flag (always
 * `editScope === "full"`); `importState` is the durable `.rustory` review
 * state, projected ONLY for a full-scope story (explicit `null` otherwise —
 * a REQUIRED key). Both survive a BLOCKING canonical degradation (story
 * metadata, not canonical content). `structure` is the Rust-projected node
 * graph and `node` the SELECTED node's content (the start node by default).
 * Both are `null` when a BLOCKING canonical issue prevents projecting
 * (degraded state); a FIXABLE issue (a broken option link) keeps them
 * projected.
 */
export interface StoryDetailDto {
  id: string;
  title: string;
  schemaVersion: number;
  structureJson: string;
  contentChecksum: string;
  createdAt: string;
  updatedAt: string;
  editable: boolean;
  editScope: StoryEditScope;
  importState: StoryImportState | null;
  structure: StoryStructure | null;
  node: NodeContentDto | null;
}

/** Mirror of `StructureWriteOutputDto` — the outcome of every structural
 *  write; the UI reconciles from its re-projected `structure`, and the
 *  local detail keeps `structureJson` in step with `contentChecksum` (the
 *  contract says those exact bytes are what the checksum covers).
 *  `importState` is read POST-UPDATE in the same transaction (a REQUIRED
 *  key, `null` unless the full edit scope). */
export interface StructureWriteOutput {
  id: string;
  updatedAt: string;
  contentChecksum: string;
  structureJson: string;
  structure: StoryStructure;
  importState: StoryImportState | null;
}

/** Mirror of `OptionRefDto` (the `linkFrom` of `add_story_node`). */
export interface OptionRef {
  nodeId: string;
  optionIndex: number;
}

/** Mirror of `AddStoryNodeInputDto`. */
export interface AddStoryNodeInput {
  storyId: string;
  linkFrom?: OptionRef;
}

/** Mirror of `DeleteStoryNodeInputDto`. */
export interface DeleteStoryNodeInput {
  storyId: string;
  nodeId: string;
}

/** Mirror of `MoveStoryNodeInputDto`. */
export interface MoveStoryNodeInput {
  storyId: string;
  nodeId: string;
  direction: "up" | "down";
}

/** Mirror of `AddNodeOptionInputDto`. */
export interface AddNodeOptionInput {
  storyId: string;
  nodeId: string;
  label: string;
}

/** Mirror of `SetNodeOptionLinkInputDto` — `target: null` unlinks. */
export interface SetNodeOptionLinkInput {
  storyId: string;
  nodeId: string;
  optionIndex: number;
  target: string | null;
}

/** Mirror of `RemoveNodeOptionInputDto`. */
export interface RemoveNodeOptionInput {
  storyId: string;
  nodeId: string;
  optionIndex: number;
}

/** Mirror of `NodeWriteOutputDto` — the outcome of every node write.
 *  `importState` is read POST-UPDATE in the same transaction (a REQUIRED
 *  key, `null` unless the full edit scope). */
export interface NodeWriteOutput {
  id: string;
  updatedAt: string;
  contentChecksum: string;
  node: NodeContentDto;
  importState: StoryImportState | null;
}

/** Mirror of `UpdateNodeContentInputDto`. */
export interface UpdateNodeContentInput {
  storyId: string;
  nodeId: string;
  text: string;
  label: string;
}

/** Which media slot an attach / remove targets. */
export type NodeMediaSlotKind = "image" | "audio";

/** Mirror of `NodeMediaSlotInputDto` (attach / remove). */
export interface NodeMediaSlotInput {
  storyId: string;
  nodeId: string;
  slot: NodeMediaSlotKind;
}

/** Tagged outcome of `attach_node_media`. `cancelled` covers a dismissed
 *  file picker (never an error). */
export type AttachNodeMediaOutcome =
  | { kind: "cancelled" }
  | { kind: "attached"; output: NodeWriteOutput };

/** Mirror of `NodeMediaPreviewDto` — a self-contained `data:` URL. */
export interface NodeMediaPreview {
  dataUrl: string;
}

/** Mirror of `RecordNodeDraftInputDto` (NFR8 recovery buffer). */
export interface RecordNodeDraftInput {
  storyId: string;
  nodeId: string;
  draftText: string;
  draftLabel: string;
}

/** Tagged union returned by `read_recoverable_node_draft`. */
export type RecoverableNodeDraft =
  | { kind: "none" }
  | {
      kind: "recoverable";
      storyId: string;
      nodeId: string;
      draftText: string;
      draftLabel: string;
      draftAt: string;
      persistedText: string;
      persistedLabel: string;
    };

const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/;

/** ISO-8601 UTC timestamp with millisecond precision. The contract
 *  mandates the `Z` suffix AND exactly three fractional digits; any
 *  other representation (including the semantically-equivalent
 *  `+00:00` offset, fractional nanoseconds, or zero-fractional) is
 *  refused so Rust stays the single source for the canonical wire
 *  shape — a drift in the Rust serializer must fail loudly here, not
 *  be quietly accommodated. The previous greedy `\.\d+` accepted
 *  fractional nanoseconds (10 digits) which would have masked a
 *  formatter regression on the Rust side. */
const ISO8601_UTC_PATTERN =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

/**
 * Runtime guard for a `StoryDetailDto` payload. Rust is authoritative, but
 * the frontend still refuses to trust a wire shape that drifts from the
 * contract — the edit surface must never render against an arbitrary
 * object.
 */
export function isStoryDetailDto(value: unknown): value is StoryDetailDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.id !== "string" || candidate.id.length === 0)
    return false;
  if (typeof candidate.title !== "string" || candidate.title.trim().length === 0)
    return false;
  if (
    typeof candidate.schemaVersion !== "number" ||
    !Number.isInteger(candidate.schemaVersion) ||
    candidate.schemaVersion < 1
  ) {
    return false;
  }
  if (typeof candidate.structureJson !== "string") return false;
  if (
    typeof candidate.contentChecksum !== "string" ||
    !SHA256_HEX_PATTERN.test(candidate.contentChecksum)
  ) {
    return false;
  }
  if (
    typeof candidate.createdAt !== "string" ||
    !ISO8601_UTC_PATTERN.test(candidate.createdAt)
  ) {
    return false;
  }
  if (
    typeof candidate.updatedAt !== "string" ||
    !ISO8601_UTC_PATTERN.test(candidate.updatedAt)
  ) {
    return false;
  }
  if (typeof candidate.editable !== "boolean") return false;
  // FR21 fields: `editScope` is REQUIRED and closed; `importState` is a
  // REQUIRED key (explicit null). A payload missing either is drift.
  if (candidate.editScope !== "full" && candidate.editScope !== "titleOnly") {
    return false;
  }
  if (!isImportStateKey(candidate.importState)) return false;
  // `editable` is Rust-derived from the scope — a payload where the two
  // disagree is an impossible DTO, not something to accommodate.
  if (candidate.editable !== (candidate.editScope === "full")) return false;
  // The import state is projected ONLY for a full-scope story (the forged
  // two-table case is neutralized in Rust) — a non-null state on titleOnly
  // is drift.
  if (candidate.importState !== null && candidate.editScope !== "full") {
    return false;
  }
  if (
    candidate.structure !== null &&
    !isStoryStructureDto(candidate.structure)
  ) {
    return false;
  }
  if (candidate.node !== null && !isNodeContentDto(candidate.node)) return false;
  // Rust degrades BOTH projections together on a blocking issue and projects
  // BOTH on a sound graph (the selected node falls back to the start node) —
  // a payload where one is null and the other is not is an impossible DTO.
  if ((candidate.structure === null) !== (candidate.node === null)) return false;
  return true;
}

/**
 * Runtime guard for an `OptionLink`. STRICT on the `state`↔`target`
 * coupling so a Rust/TS drift (or a frontend re-derivation bug) fails
 * loudly instead of rendering a broken link as linked: `unlinked` MUST
 * carry `target: null`; `linked` and `broken` MUST carry a non-empty
 * target id.
 */
export function isOptionLink(value: unknown): value is OptionLink {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.label !== "string") return false;
  if (c.state === "unlinked") return c.target === null;
  if (c.state === "linked" || c.state === "broken") {
    return typeof c.target === "string" && c.target.length > 0;
  }
  return false;
}

/**
 * Runtime guard for a `NodeGraph`. Also STRICT on the `hasIssue`↔options
 * coupling: the flag is Rust-derived from the broken links, so a payload
 * where the two disagree is drift, not something to accommodate.
 */
export function isNodeGraph(value: unknown): value is NodeGraph {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.id !== "string" || c.id.length === 0) return false;
  if (typeof c.label !== "string") return false;
  if (typeof c.isStart !== "boolean") return false;
  if (typeof c.hasIssue !== "boolean") return false;
  if (!Array.isArray(c.options) || !c.options.every(isOptionLink)) return false;
  const hasBroken = (c.options as OptionLink[]).some(
    (o) => o.state === "broken",
  );
  return c.hasIssue === hasBroken;
}

/**
 * Runtime guard for a `StoryStructure`. Checks the graph-level coherence the
 * navigator relies on: a non-empty start id that references a listed node,
 * and per-node `isStart` flags that agree with it.
 */
export function isStoryStructureDto(value: unknown): value is StoryStructure {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.startNodeId !== "string" || c.startNodeId.length === 0)
    return false;
  if (!Array.isArray(c.nodes) || !c.nodes.every(isNodeGraph)) return false;
  const nodes = c.nodes as NodeGraph[];
  if (nodes.length === 0) return false;
  // Rust never projects a graph with duplicate node ids (Blocking) — a
  // payload that carries them is drift, and every id-based lookup below
  // would silently resolve to the wrong node.
  const ids = new Set(nodes.map((node) => node.id));
  if (ids.size !== nodes.length) return false;
  // Rust never projects a graph whose start node is missing (Blocking) —
  // a payload that does is drift.
  if (!ids.has(c.startNodeId)) return false;
  if (!nodes.every((node) => node.isStart === (node.id === c.startNodeId))) {
    return false;
  }
  // The per-option `state` is Rust-derived FROM this very graph: `linked`
  // must reference a listed node, `broken` must NOT. With the whole graph in
  // hand the guard re-checks the coupling — a mismatch would repaint a
  // broken link as live (or vice versa).
  return nodes.every((node) =>
    node.options.every((option) => {
      if (option.state === "linked") {
        return option.target !== null && ids.has(option.target);
      }
      if (option.state === "broken") {
        return option.target !== null && !ids.has(option.target);
      }
      return true;
    }),
  );
}

/** Runtime guard for a `StructureWriteOutput`. The `importState` key is
 *  REQUIRED (explicit null) — an acknowledgement missing it is drift. */
export function isStructureWriteOutput(
  value: unknown,
): value is StructureWriteOutput {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.id !== "string" || c.id.length === 0) return false;
  if (typeof c.updatedAt !== "string" || !ISO8601_UTC_PATTERN.test(c.updatedAt)) {
    return false;
  }
  if (
    typeof c.contentChecksum !== "string" ||
    !SHA256_HEX_PATTERN.test(c.contentChecksum)
  ) {
    return false;
  }
  if (typeof c.structureJson !== "string" || c.structureJson.length === 0) {
    return false;
  }
  if (!("importState" in c) || !isImportStateKey(c.importState)) return false;
  return isStoryStructureDto(c.structure);
}

/**
 * Runtime guard for a `NodeMediaSlot`. STRICT on the `state`↔fields coupling so
 * a Rust/TS drift raises a `NodeContractDriftError` instead of being masked by a
 * `média · 0 o` fallback in the UI: a `ready` slot MUST carry a known format and
 * a safe non-negative size; an `attention` slot MUST carry neither.
 */
export function isNodeMediaSlot(value: unknown): value is NodeMediaSlot {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.assetId !== "string" || c.assetId.length === 0) return false;
  if (c.mediaType !== "image" && c.mediaType !== "audio") return false;
  if (c.state === "ready") {
    if (typeof c.format !== "string" || c.format.length === 0) return false;
    return (
      typeof c.byteSize === "number" &&
      Number.isSafeInteger(c.byteSize) &&
      c.byteSize >= 0
    );
  }
  if (c.state === "attention") {
    return c.format === undefined && c.byteSize === undefined;
  }
  return false;
}

/** Runtime guard for a `NodeContentDto`. */
export function isNodeContentDto(value: unknown): value is NodeContentDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.id !== "string" || c.id.length === 0) return false;
  if (typeof c.text !== "string") return false;
  if (typeof c.label !== "string") return false;
  if (c.image !== null && !isNodeMediaSlot(c.image)) return false;
  if (c.audio !== null && !isNodeMediaSlot(c.audio)) return false;
  return true;
}

/** Runtime guard for a `NodeWriteOutput`. The `importState` key is REQUIRED
 *  (explicit null) — an acknowledgement missing it is drift. */
export function isNodeWriteOutput(value: unknown): value is NodeWriteOutput {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.id !== "string" || c.id.length === 0) return false;
  if (typeof c.updatedAt !== "string" || !ISO8601_UTC_PATTERN.test(c.updatedAt)) {
    return false;
  }
  if (
    typeof c.contentChecksum !== "string" ||
    !SHA256_HEX_PATTERN.test(c.contentChecksum)
  ) {
    return false;
  }
  if (!("importState" in c) || !isImportStateKey(c.importState)) return false;
  return isNodeContentDto(c.node);
}

/** Runtime guard for an `AttachNodeMediaOutcome`. */
export function isAttachNodeMediaOutcome(
  value: unknown,
): value is AttachNodeMediaOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "cancelled") return Object.keys(c).length === 1;
  if (c.kind === "attached") return isNodeWriteOutput(c.output);
  return false;
}

/** Runtime guard for a `NodeMediaPreview`. */
export function isNodeMediaPreview(value: unknown): value is NodeMediaPreview {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  return typeof c.dataUrl === "string" && c.dataUrl.startsWith("data:");
}

/** Runtime guard for a `RecoverableNodeDraft`. */
export function isRecoverableNodeDraft(
  value: unknown,
): value is RecoverableNodeDraft {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "none") return Object.keys(c).length === 1;
  if (c.kind !== "recoverable") return false;
  if (typeof c.storyId !== "string" || c.storyId.length === 0) return false;
  if (typeof c.nodeId !== "string" || c.nodeId.length === 0) return false;
  if (typeof c.draftText !== "string") return false;
  if (typeof c.draftLabel !== "string") return false;
  if (typeof c.persistedText !== "string") return false;
  if (typeof c.persistedLabel !== "string") return false;
  if (typeof c.draftAt !== "string" || !ISO8601_UTC_PATTERN.test(c.draftAt)) {
    return false;
  }
  return true;
}

/**
 * Runtime guard for `UpdateStoryOutput`. `saveStory` and `applyRecovery`
 * both resolve with this exact payload — locking the shape here keeps the
 * two title-write paths symmetric. The `importState` key is REQUIRED
 * (explicit null): an acknowledgement missing it is drift.
 */
export function isUpdateStoryOutput(value: unknown): value is UpdateStoryOutput {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.id !== "string" || candidate.id.length === 0)
    return false;
  if (typeof candidate.title !== "string" || candidate.title.trim().length === 0)
    return false;
  if (
    typeof candidate.updatedAt !== "string" ||
    !ISO8601_UTC_PATTERN.test(candidate.updatedAt)
  ) {
    return false;
  }
  if (!("importState" in candidate) || !isImportStateKey(candidate.importState)) {
    return false;
  }
  return true;
}

/**
 * Wire input for the `record_draft` Tauri command. Mirror of
 * `RecordDraftInputDto`. `draftTitle` may be empty (the user erased
 * everything) and may carry control characters — re-validation only
 * kicks in at apply time, never at record time.
 */
export interface RecordDraftInput {
  storyId: string;
  draftTitle: string;
}

/** Wire input for the `apply_recovery` Tauri command. */
export interface ApplyRecoveryInput {
  storyId: string;
}

/**
 * Tagged union returned by `read_recoverable_draft`. `kind: "none"` is
 * the explicit "no draft to recover" state and is NOT an error — the UI
 * simply hides the recovery banner.
 */
export type RecoverableDraft =
  | { kind: "none" }
  | {
      kind: "recoverable";
      storyId: string;
      draftTitle: string;
      draftAt: string;
      persistedTitle: string;
    };

/** Hard cap mirrored from the Rust-side `MAX_DRAFT_TITLE_CHARS`. */
const MAX_DRAFT_TITLE_LENGTH = 4096;

/**
 * Runtime guard for `RecoverableDraft`. A drifted wire shape (missing
 * `kind`, unknown `kind` value, malformed timestamp) is rejected so the
 * recovery banner never renders against an arbitrary object.
 */
export function isRecoverableDraft(value: unknown): value is RecoverableDraft {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.kind === "none") {
    // The `none` variant must not carry any extra fields — anything else
    // is drift.
    return Object.keys(candidate).length === 1;
  }
  if (candidate.kind !== "recoverable") return false;
  if (typeof candidate.storyId !== "string" || candidate.storyId.length === 0)
    return false;
  // `draftTitle` MAY be empty (user erased everything before crash).
  if (typeof candidate.draftTitle !== "string") return false;
  // Cap is anti-DoS, mirrored from Rust's `MAX_DRAFT_TITLE_CHARS`. Rust
  // counts Unicode scalars via `chars().count()`; JS `.length` would
  // count UTF-16 code units, which double-counts surrogate pairs (e.g.
  // an emoji = 2). Use the iterator form to match Rust exactly.
  if ([...candidate.draftTitle].length > MAX_DRAFT_TITLE_LENGTH) return false;
  if (
    typeof candidate.draftAt !== "string" ||
    !ISO8601_UTC_PATTERN.test(candidate.draftAt)
  ) {
    return false;
  }
  // `persistedTitle` mirrors `stories.title`, which the CHECK constraint
  // forbids from being blank — a blank value here means drift.
  if (
    typeof candidate.persistedTitle !== "string" ||
    candidate.persistedTitle.trim().length === 0
  ) {
    return false;
  }
  return true;
}

export type { StoryCardDto };
