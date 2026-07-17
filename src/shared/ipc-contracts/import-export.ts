/**
 * Wire contract for the `export_story_with_save_dialog` Tauri command
 * input. Mirror of
 * `src-tauri/src/ipc/dto/import_export.rs::ExportStoryDialogInputDto`.
 *
 * `suggestedFilename` is the default text pre-filled in the native save
 * dialog. The frontend never constructs the actual destination path —
 * the dialog returns it, and Rust validates it at the boundary.
 */
export interface ExportStoryDialogInput {
  storyId: string;
  suggestedFilename: string;
}

/**
 * Tagged outcome returned by `export_story_with_save_dialog`. Mirror of
 * `ExportStoryDialogOutcomeDto`.
 *
 * A cancelled dialog is NOT an error — the command resolves with
 * `{ kind: "cancelled" }` so the UI can silently return to idle.
 * Unrecoverable failures cross the boundary as `AppError` rejections.
 */
export type ExportStoryDialogOutcome =
  | {
      kind: "exported";
      destinationPath: string;
      bytesWritten: number;
      contentChecksum: string;
    }
  | { kind: "cancelled" };

const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/;

/**
 * Runtime guard for an [`ExportStoryDialogOutcome`] payload. Rust is
 * authoritative, but the frontend still refuses to trust a wire shape
 * that drifts from the contract — the export success surface must
 * never render against an arbitrary object.
 */
export function isExportStoryDialogOutcome(
  value: unknown,
): value is ExportStoryDialogOutcome {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.kind === "cancelled") {
    // The cancelled variant carries no other field.
    return Object.keys(candidate).length === 1;
  }
  if (candidate.kind !== "exported") return false;
  if (
    typeof candidate.destinationPath !== "string" ||
    candidate.destinationPath.length === 0
  ) {
    return false;
  }
  if (
    typeof candidate.bytesWritten !== "number" ||
    !Number.isInteger(candidate.bytesWritten) ||
    candidate.bytesWritten < 0
  ) {
    return false;
  }
  if (
    typeof candidate.contentChecksum !== "string" ||
    !SHA256_HEX_PATTERN.test(candidate.contentChecksum)
  ) {
    return false;
  }
  return true;
}

// ===== Local artifact import (`.rustory` file → library) =====
//
// Mirror of `src-tauri/src/ipc/dto/import_export.rs` (import side). Rust is
// authoritative; these guards refuse a wire shape that drifts so the import
// surface never renders against an arbitrary object.

/** Global recognition quality (UI: Propre / Partiellement exploitable / Inexploitable). */
export type ImportQuality = "clean" | "partial" | "unusable";

/** Durable per-story import state. On a Story Card only `recognized` /
 *  `partial` / `needsReview` ever appear. */
export type ImportState =
  "recognized" | "partial" | "needsReview" | "blocked" | "resolved";

/** The aspect of the analyzed input a finding refers to (`media` belongs
 *  to the structured-folder and RSS flows, `source` to the RSS ingestion
 *  flow only). */
export type ImportAspect =
  | "envelope"
  | "formatVersion"
  | "schemaVersion"
  | "structure"
  | "integrity"
  | "title"
  | "timestamps"
  | "media"
  | "source";

/** The recognition category of a finding. */
export type ImportCategory =
  "recognized" | "ambiguous" | "missing" | "blocking";

/** A single recognition finding: a closed `(aspect, category)` pair plus the
 *  canonical FR message. Rust owns the message; the UI renders it verbatim. */
export interface ImportFinding {
  aspect: ImportAspect;
  category: ImportCategory;
  message: string;
}

/** The validated canonical content carried from the analyze verdict to the
 *  accept call. The frontend round-trips it verbatim; Rust re-validates it. */
export interface ImportableContent {
  title: string;
  structureJson: string;
  contentChecksum: string;
  createdAt: string;
  updatedAt: string;
}

/** Tagged outcome of `analyze_artifact_for_import`. A cancelled dialog is NOT
 *  an error — `{ kind: "cancelled" }` is a silent no-op. */
export type ImportArtifactAnalysis =
  | {
      kind: "analyzed";
      quality: ImportQuality;
      state: ImportState;
      findings: ImportFinding[];
      /** Present iff importable (`quality !== "unusable"`). */
      importableContent?: ImportableContent;
      sourceName: string;
      artifactChecksum: string;
    }
  | { kind: "cancelled" };

/** Input to `accept_artifact_import` — the validated content + provenance. */
export interface AcceptArtifactImportInput {
  content: ImportableContent;
  sourceName: string;
  artifactChecksum: string;
}

// ===== OS open channel (a file opened through the operating system) =====
//
// Mirror of `src-tauri/src/ipc/dto/import_export.rs::OsOpenAnalysisDto`.
// The `analyzed` variant COMPOSES the exact field set of the dialog-import
// verdict (same keys, same coherence rules — one guard validates both);
// `none` is the total silent no-op; `multipleFiles` carries the Rust-frozen
// calm-limit copy, rendered verbatim.

/** Tagged outcome of `analyze_os_open_request`. A transport failure (the
 *  file became unreadable) rejects with a normalized `AppError` instead —
 *  the intent then STAYS pending Rust-side so a retry can replay it. */
export type OsOpenAnalysis =
  | { kind: "none" }
  | { kind: "multipleFiles"; message: string }
  | Extract<ImportArtifactAnalysis, { kind: "analyzed" }>;

/**
 * Runtime guard for an [`OsOpenAnalysis`] payload. Fail-closed: the
 * `multipleFiles` copy must be non-empty (a calm limit with no words would
 * render an empty status region), and the `analyzed` variant is validated
 * by the SAME dialog-import guard (structural identity, never a re-typed
 * sibling check).
 */
export function isOsOpenAnalysis(value: unknown): value is OsOpenAnalysis {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "none") {
    return Object.keys(c).length === 1;
  }
  if (c.kind === "multipleFiles") {
    return (
      Object.keys(c).length === 2 &&
      typeof c.message === "string" &&
      c.message.length > 0
    );
  }
  return c.kind === "analyzed" && isImportArtifactAnalysis(value);
}

const IMPORT_QUALITIES: ReadonlySet<string> = new Set([
  "clean",
  "partial",
  "unusable",
]);
const IMPORT_STATES: ReadonlySet<string> = new Set([
  "recognized",
  "partial",
  "needsReview",
  "blocked",
  "resolved",
]);
const IMPORT_ASPECTS: ReadonlySet<string> = new Set([
  "envelope",
  "formatVersion",
  "schemaVersion",
  "structure",
  "integrity",
  "title",
  "timestamps",
  "media",
  "source",
]);
const IMPORT_CATEGORIES: ReadonlySet<string> = new Set([
  "recognized",
  "ambiguous",
  "missing",
  "blocking",
]);

export function isImportFinding(value: unknown): value is ImportFinding {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  return (
    typeof c.aspect === "string" &&
    IMPORT_ASPECTS.has(c.aspect) &&
    typeof c.category === "string" &&
    IMPORT_CATEGORIES.has(c.category) &&
    typeof c.message === "string" &&
    c.message.length > 0
  );
}

function isImportableContent(value: unknown): value is ImportableContent {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  // Timestamps are NOT length-gated here: Rust is the authority and PRESERVES
  // any carried timestamp verbatim — an empty / off-canonical value is a
  // `Timestamps=Ambiguous` finding (quality `partial`, still importable, AC2),
  // never a transport failure. A `length > 0` gate would wrongly turn such an
  // "à revoir, importable" verdict into a contract-drift "Import impossible".
  return (
    typeof c.title === "string" &&
    typeof c.structureJson === "string" &&
    typeof c.contentChecksum === "string" &&
    SHA256_HEX_PATTERN.test(c.contentChecksum) &&
    typeof c.createdAt === "string" &&
    typeof c.updatedAt === "string"
  );
}

/**
 * Runtime guard for an [`ImportArtifactAnalysis`] payload. Rust is
 * authoritative, but the import surface must never render against an
 * arbitrary object — closed discriminants, typed findings, an importable
 * content present exactly when the verdict is not blocked.
 */
export function isImportArtifactAnalysis(
  value: unknown,
): value is ImportArtifactAnalysis {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "cancelled") {
    return Object.keys(c).length === 1;
  }
  if (c.kind !== "analyzed") return false;
  if (typeof c.quality !== "string" || !IMPORT_QUALITIES.has(c.quality)) {
    return false;
  }
  if (typeof c.state !== "string" || !IMPORT_STATES.has(c.state)) return false;
  if (!Array.isArray(c.findings) || !c.findings.every(isImportFinding)) {
    return false;
  }
  const findings = c.findings as ImportFinding[];
  if (
    typeof c.sourceName !== "string" ||
    c.sourceName.length === 0 ||
    typeof c.artifactChecksum !== "string" ||
    !SHA256_HEX_PATTERN.test(c.artifactChecksum)
  ) {
    return false;
  }
  // The quality MUST be the one derived from the findings: a blocking finding
  // dominates (unusable), else any ambiguous/missing makes it partial, else
  // clean. A drift that ships a quality inconsistent with its findings is a
  // contract error, not a surface to render.
  if (c.quality !== qualityFromFindings(findings)) return false;
  // The state MUST be coherent with the quality (the Rust derivation).
  if (!isCoherentQualityState(c.quality, c.state)) return false;
  // No aspect may appear twice — a duplicated aspect is a malformed verdict.
  const aspects = new Set<string>();
  for (const finding of findings) {
    if (aspects.has(finding.aspect)) return false;
    aspects.add(finding.aspect);
  }
  // A non-unusable verdict MUST carry importable content; an unusable
  // (blocked) one must NOT — the shape mirrors the Rust `Option`.
  const hasContent = c.importableContent !== undefined;
  if (c.quality === "unusable") {
    if (hasContent) return false;
  } else {
    if (!isImportableContent(c.importableContent)) return false;
  }
  return true;
}

/** The quality derived from a finding set: blocking ⟹ unusable, else any
 *  ambiguous/missing ⟹ partial, else clean (mirrors the Rust derivation). */
function qualityFromFindings(findings: ImportFinding[]): ImportQuality {
  if (findings.some((f) => f.category === "blocking")) return "unusable";
  if (
    findings.some((f) => f.category === "ambiguous" || f.category === "missing")
  ) {
    return "partial";
  }
  return "clean";
}

/** The allowed quality↔state couples (the Rust `import_state` mapping). */
function isCoherentQualityState(quality: string, state: string): boolean {
  switch (quality) {
    case "clean":
      return state === "recognized";
    case "partial":
      // `.rustory` uses `needsReview`; `partial` is the structured-folder
      // mapping (a referenced media is missing).
      return state === "needsReview" || state === "partial";
    case "unusable":
      return state === "blocked";
    default:
      return false;
  }
}

// ===== Structured-folder creation (folder → new canonical story) =====
//
// Mirror of `src-tauri/src/ipc/dto/import_export.rs` (structured-folder
// side). Rust is authoritative; these guards refuse a drifted wire shape so
// the creation surface never renders against an arbitrary object.

/** The creatable-content summary of an `analyzed` folder verdict: what WILL
 *  be created if accepted. The per-file media detail lives here only — the
 *  persisted findings stay aggregated pairs. */
export interface CreatableSummary {
  title: string;
  nodeCount: number;
  retainedMedia: string[];
  discardedMedia: string[];
}

/** Tagged outcome of `analyze_structured_folder_for_creation`. A cancelled
 *  dialog is NOT an error — `{ kind: "cancelled" }` is a silent no-op. */
export type StructuredCreationAnalysis =
  | {
      kind: "analyzed";
      quality: ImportQuality;
      state: ImportState;
      findings: ImportFinding[];
      /** Present iff creatable (`quality !== "unusable"`). */
      creatableSummary?: CreatableSummary;
      /** The folder's basename — the only name the surface renders. */
      folderName: string;
      /** The absolute path from the SYSTEM dialog, round-tripped to the
       *  accept call ONLY. Never rendered, never persisted, never logged. */
      folderPath: string;
    }
  | { kind: "cancelled" };

/** Input to `accept_structured_creation` — the folder pointer, verbatim. */
export interface AcceptStructuredCreationInput {
  folderPath: string;
}

// ===== RSS external-source creation (feed → new canonical story) =====
//
// Mirror of `src-tauri/src/ipc/dto/import_export.rs` (RSS side). Rust is
// authoritative; these guards refuse a drifted wire shape so the creation
// surface never renders against an arbitrary object.

/** The stable reference of one previewed feed item, round-tripped verbatim
 *  to `accept_rss_story_creation` and re-resolved by Rust against a FRESH
 *  fetch (strict guid, else exact title+link). `fingerprint` is the
 *  canonical proof of the PREVIEWED content — Rust recomputes it on the
 *  fresh item and refuses any divergence (same guid, different text ⇒
 *  `sourceChanged`), so a creation can never ingest content the user
 *  never reread. */
export type RssItemRef =
  | { kind: "guid"; guid: string; fingerprint: string }
  | {
      kind: "titleLink";
      title: string;
      link: string | null;
      fingerprint: string;
    };

/** One selectable item of a previewed feed. `title` may be empty (the
 *  surface then leads with the summary); `summary` is a bounded excerpt. */
export interface RssPreviewItem {
  title: string;
  summary: string;
  hasEnclosure: boolean;
  itemRef: RssItemRef;
}

/** The typed outcome of `fetch_rss_source_preview`. `blocked` mirrors
 *  `state === "blocked"` (the guard refuses a divergence); a blocked
 *  verdict carries no item. A TRANSPORT failure rejects with
 *  `RSS_SOURCE_UNREACHABLE` instead — the content verdict is NEVER an
 *  error. */
export interface RssPreview {
  sourceHost: string;
  items: RssPreviewItem[];
  findings: ImportFinding[];
  state: ImportState;
  blocked: boolean;
}

/** Tagged outcome of `accept_rss_story_creation`: the created card + its
 *  report, or the honest recoverable refusal (`sourceChanged` — the feed
 *  diverged since the preview; NOTHING was created). The `story` payload is
 *  validated by the facade with the library card guard (no contract-module
 *  cycle). */
export type RssCreationOutcome<Story = unknown> =
  | { kind: "created"; story: Story; report: ImportFinding[] }
  | { kind: "sourceChanged" };

export function isRssItemRef(value: unknown): value is RssItemRef {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  // The previewed-content proof is REQUIRED on every reference.
  if (
    typeof c.fingerprint !== "string" ||
    !SHA256_HEX_PATTERN.test(c.fingerprint)
  ) {
    return false;
  }
  if (c.kind === "guid") {
    return typeof c.guid === "string" && c.guid.length > 0;
  }
  if (c.kind === "titleLink") {
    return (
      typeof c.title === "string" &&
      (c.link === null || typeof c.link === "string")
    );
  }
  return false;
}

/** Mirror of the Rust host bound (`Histoire de {hôte}` must stay a valid
 *  canonical title without truncation). */
const MAX_RSS_HOST_CHARS = 96;

/** True iff `value` is a plausible HOST-ONLY source name: bounded, free of
 *  URL structure (`/ \ : ? # @`), whitespace and control characters. The
 *  backend promises "host only — never the full address" (feed query
 *  strings can carry private tokens); this predicate refuses a drift
 *  before the surface renders it. */
function isHostOnly(value: string): boolean {
  if (value.length === 0 || Array.from(value).length > MAX_RSS_HOST_CHARS) {
    return false;
  }
  for (const ch of value) {
    if ("/\\:?#@".includes(ch) || /\s/.test(ch)) return false;
    const code = ch.codePointAt(0) ?? 0;
    if (code < 0x20 || (code >= 0x7f && code <= 0x9f)) return false;
  }
  return true;
}

function isRssPreviewItem(value: unknown): value is RssPreviewItem {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  return (
    typeof c.title === "string" &&
    typeof c.summary === "string" &&
    typeof c.hasEnclosure === "boolean" &&
    isRssItemRef(c.itemRef) &&
    // An item with neither a title nor a summary would be unselectable —
    // Rust never ships one (exploitability gate).
    (c.title.length > 0 || c.summary.length > 0)
  );
}

/**
 * Runtime guard for an [`RssPreview`] payload. Deterministic like the
 * folder guard: `blocked` must mirror `state === "blocked"`; a blocked
 * verdict carries a blocking finding and ZERO item; an exploitable one
 * floors at `needsReview` (never `recognized` — the ingestion floor) and
 * always carries the nominal `(source, ambiguous)` finding.
 */
export function isRssPreview(value: unknown): value is RssPreview {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.sourceHost !== "string" || !isHostOnly(c.sourceHost)) {
    return false;
  }
  if (typeof c.state !== "string" || !IMPORT_STATES.has(c.state)) return false;
  if (typeof c.blocked !== "boolean") return false;
  if ((c.state === "blocked") !== c.blocked) return false;
  if (!Array.isArray(c.findings) || !c.findings.every(isImportFinding)) {
    return false;
  }
  const findings = c.findings as ImportFinding[];
  if (!Array.isArray(c.items) || !c.items.every(isRssPreviewItem)) {
    return false;
  }
  const items = c.items as RssPreviewItem[];
  if (c.blocked) {
    if (items.length > 0) return false;
    if (!findings.some((f) => f.category === "blocking")) return false;
  } else {
    if (c.state !== "needsReview") return false;
    if (items.length === 0) return false;
    if (findings.some((f) => f.category === "blocking")) return false;
    if (
      !findings.some((f) => f.aspect === "source" && f.category === "ambiguous")
    ) {
      return false;
    }
  }
  return true;
}

// ===== Content-source activation policy (distribution governance) =====
//
// Mirror of `src-tauri/src/ipc/dto/import_export.rs` (policy side). Rust
// alone decides the policy (`Content Source Activation Contract`): the
// frontend renders what it declares and never hardcodes the source list,
// the labels or the reasons. These guards refuse a drifted wire shape so
// the creation dialog never renders against an arbitrary object.

/** Closed set of content-source kinds this product speaks about. */
export type ContentSourceKind = "rss" | "atom" | "jsonFeed";

/** Closed set of activation states a distribution can assign to a kind. */
export type ContentSourceActivation =
  "enabled" | "notActivated" | "blockedByPolicy";

/** One line of the read policy: the closed tags, the frozen label, and
 *  EXACTLY ONE of — the frozen entry-level activation marker on an
 *  enabled line, or the frozen disabled-entry reason on a non-enabled
 *  one (the guard refuses incoherence). */
export interface ContentSourceEntry {
  kind: ContentSourceKind;
  label: string;
  activation: ContentSourceActivation;
  /** Present IFF the line is enabled — the Rust-owned entry-level
   *  marker every rendering surface (creation dialog, support-profile
   *  screen) renders verbatim. */
  activationMarker?: string;
  /** Present IFF the line is not enabled (the guard refuses
   *  incoherence); an enabled entry carries the marker instead. */
  reason?: string;
}

/** The read content-source policy: every line of the distribution's
 *  matrix, in its stable order. */
export interface ContentSourcePolicy {
  sources: ContentSourceEntry[];
}

/** The frozen kind → label couples, exactly as Rust serializes them.
 *  VALIDATION literals only (the rendering keeps using the Rust-carried
 *  values): the guard's job is to refuse a drifted copy before it is
 *  ever rendered as authoritative. */
const CONTENT_SOURCE_LABELS: Readonly<Record<ContentSourceKind, string>> = {
  rss: "Flux RSS",
  atom: "Flux Atom",
  jsonFeed: "Flux JSON Feed",
};

/** The frozen activation → reason couples for the non-enabled lines
 *  (an enabled line carries NO reason — the marker replaces it). */
const CONTENT_SOURCE_REASONS: Readonly<
  Record<Exclude<ContentSourceActivation, "enabled">, string>
> = {
  notActivated:
    "Source indisponible: non activée dans la distribution officielle",
  blockedByPolicy:
    "Source indisponible: bloquée par la politique de distribution",
};

/** The frozen entry-level activation marker of an enabled line, exactly
 *  as Rust serializes it (VALIDATION literal — the rendering keeps
 *  using the Rust-carried value). */
const CONTENT_SOURCE_ACTIVATION_MARKER =
  "Activée par la distribution officielle";

function isContentSourceKind(value: unknown): value is ContentSourceKind {
  return typeof value === "string" && value in CONTENT_SOURCE_LABELS;
}

export function isContentSourceEntry(
  value: unknown,
): value is ContentSourceEntry {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!isContentSourceKind(c.kind)) return false;
  // The label is the FROZEN couple of the kind — an arbitrary label is a
  // drift, never a copy to render as authoritative.
  if (c.label !== CONTENT_SOURCE_LABELS[c.kind]) return false;
  if (c.activation === "enabled") {
    // Only `rss` has an ingestion flow in THIS build: a policy enabling
    // any other kind is ahead of the frontend and MUST fail closed (the
    // dialog would otherwise render a disabled entry "justified" by the
    // activation marker). Activating another kind is an explicit
    // re-scope of this guard, alongside its ingestion flow.
    if (c.kind !== "rss") return false;
    // An enabled line carries NO reason and EXACTLY its frozen
    // Rust-owned entry-level marker — the mirror of the Rust `Option`
    // + `skip_serializing_if` pair.
    return (
      c.reason === undefined &&
      c.activationMarker === CONTENT_SOURCE_ACTIVATION_MARKER
    );
  }
  if (c.activation !== "notActivated" && c.activation !== "blockedByPolicy") {
    return false;
  }
  // A non-enabled line carries EXACTLY its frozen reason and NO marker.
  return (
    c.reason === CONTENT_SOURCE_REASONS[c.activation] &&
    c.activationMarker === undefined
  );
}

/**
 * Runtime guard for a [`ContentSourcePolicy`] payload: EXACTLY one line
 * per known kind (`rss` / `atom` / `jsonFeed`), each carrying its frozen
 * label and — when not enabled — its frozen reason. A partial policy
 * (a known kind missing) is a drift too: the contract promises that the
 * non-activated kinds stay VISIBLE with their reason, so their silent
 * disappearance must never render. A refused payload surfaces as a drift
 * error, which the dialog treats as a failed policy read — fail-closed,
 * never active-by-default. A distribution wanting another set is an
 * explicit re-scope of this guard, not a silent acceptance.
 */
export function isContentSourcePolicy(
  value: unknown,
): value is ContentSourcePolicy {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!Array.isArray(c.sources)) return false;
  if (!c.sources.every(isContentSourceEntry)) return false;
  const kinds = new Set<string>();
  for (const entry of c.sources as ContentSourceEntry[]) {
    // A duplicated kind is a malformed policy, not a surface to render.
    if (kinds.has(entry.kind)) return false;
    kinds.add(entry.kind);
  }
  // Exactly the current closed set — nothing missing, nothing extra.
  return (
    kinds.size === Object.keys(CONTENT_SOURCE_LABELS).length &&
    Object.keys(CONTENT_SOURCE_LABELS).every((kind) => kinds.has(kind))
  );
}

/** The folder flow's OWN aspect set (no schemaVersion / integrity /
 *  timestamps — an author manifest has none). */
const FOLDER_ASPECTS: ReadonlySet<string> = new Set([
  "envelope",
  "formatVersion",
  "title",
  "structure",
  "media",
]);

function isCreatableSummary(value: unknown): value is CreatableSummary {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  return (
    typeof c.title === "string" &&
    c.title.length > 0 &&
    typeof c.nodeCount === "number" &&
    Number.isInteger(c.nodeCount) &&
    c.nodeCount >= 1 &&
    Array.isArray(c.retainedMedia) &&
    c.retainedMedia.every((m) => typeof m === "string" && m.length > 0) &&
    Array.isArray(c.discardedMedia) &&
    c.discardedMedia.every((m) => typeof m === "string" && m.length > 0)
  );
}

/**
 * Runtime guard for a [`StructuredCreationAnalysis`] payload. Stricter than
 * the `.rustory` guard where the folder derivation is fully deterministic:
 * the `partial` state requires at least one `missing` finding, `needsReview`
 * requires an ambiguity and NO missing (missing would have derived
 * `partial`), aspects come from the FOLDER set without duplicates, a
 * non-blocked verdict carries exactly the five folder aspects AND a
 * creatable summary, and the `folderPath` / `folderName` are non-empty.
 */
export function isStructuredCreationAnalysis(
  value: unknown,
): value is StructuredCreationAnalysis {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "cancelled") {
    return Object.keys(c).length === 1;
  }
  if (c.kind !== "analyzed") return false;
  if (typeof c.quality !== "string" || !IMPORT_QUALITIES.has(c.quality)) {
    return false;
  }
  if (typeof c.state !== "string" || !IMPORT_STATES.has(c.state)) return false;
  if (!Array.isArray(c.findings) || !c.findings.every(isImportFinding)) {
    return false;
  }
  const findings = c.findings as ImportFinding[];
  if (
    typeof c.folderName !== "string" ||
    c.folderName.length === 0 ||
    typeof c.folderPath !== "string" ||
    c.folderPath.length === 0
  ) {
    return false;
  }
  // Aspects: from the folder set, no duplicates.
  const aspects = new Set<string>();
  for (const finding of findings) {
    if (!FOLDER_ASPECTS.has(finding.aspect)) return false;
    if (aspects.has(finding.aspect)) return false;
    aspects.add(finding.aspect);
  }
  // Quality must be the one derived from the findings; state must be the
  // FOLDER derivation (deterministic): missing ⟹ partial, ambiguous-only ⟹
  // needsReview.
  if (c.quality !== qualityFromFindings(findings)) return false;
  if (!isCoherentQualityState(c.quality, c.state)) return false;
  const hasMissing = findings.some((f) => f.category === "missing");
  if (c.state === "partial" && !hasMissing) return false;
  if (c.state === "needsReview" && hasMissing) return false;
  // A non-blocked verdict analyzes the WHOLE folder matrix (five aspects)
  // and carries what will be created; a blocked one carries nothing.
  const hasSummary = c.creatableSummary !== undefined;
  if (c.quality === "unusable") {
    if (hasSummary) return false;
  } else {
    if (aspects.size !== FOLDER_ASPECTS.size) return false;
    if (!isCreatableSummary(c.creatableSummary)) return false;
  }
  return true;
}

// ===== Drop channel (a file or folder dropped on the window) =====
//
// Mirror of `src-tauri/src/ipc/dto/import_export.rs::DropAnalysisDto`. The
// `artifact` variant COMPOSES the exact field set of the dialog-import
// `analyzed` verdict and the `folder` variant the exact field set of the
// picker folder `analyzed` verdict (same keys, same coherence rules — the
// existing guards validate both, modulo the tag); `none` is the total
// silent no-op; `multipleItems` carries the Rust-frozen calm-limit copy,
// rendered verbatim.

/** Tagged outcome of `analyze_drop_request`. A transport failure (the
 *  dropped element became unreadable) rejects with a normalized `AppError`
 *  instead — the intent then STAYS pending Rust-side so a retry can
 *  replay it. */
export type DropAnalysis =
  | { kind: "none" }
  | { kind: "multipleItems"; message: string }
  | ({ kind: "artifact" } & Omit<
      Extract<ImportArtifactAnalysis, { kind: "analyzed" }>,
      "kind"
    >)
  | ({ kind: "folder" } & Omit<
      Extract<StructuredCreationAnalysis, { kind: "analyzed" }>,
      "kind"
    >);

/**
 * Runtime guard for a [`DropAnalysis`] payload. Fail-closed: the
 * `multipleItems` copy must be non-empty (a calm limit with no words would
 * render an empty status region), and the `artifact` / `folder` variants
 * are validated by the SAME dialog-import / picker-folder guards
 * (structural identity through a re-tag — never a re-typed sibling check).
 */
export function isDropAnalysis(value: unknown): value is DropAnalysis {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "none") {
    return Object.keys(c).length === 1;
  }
  if (c.kind === "multipleItems") {
    return (
      Object.keys(c).length === 2 &&
      typeof c.message === "string" &&
      c.message.length > 0
    );
  }
  if (c.kind === "artifact") {
    return isImportArtifactAnalysis({ ...c, kind: "analyzed" });
  }
  if (c.kind === "folder") {
    return isStructuredCreationAnalysis({ ...c, kind: "analyzed" });
  }
  return false;
}
