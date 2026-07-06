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
  | "recognized"
  | "partial"
  | "needsReview"
  | "blocked"
  | "resolved";

/** The aspect of the analyzed input a finding refers to (`media` belongs
 *  to the structured-folder flow only). */
export type ImportAspect =
  | "envelope"
  | "formatVersion"
  | "schemaVersion"
  | "structure"
  | "integrity"
  | "title"
  | "timestamps"
  | "media";

/** The recognition category of a finding. */
export type ImportCategory =
  | "recognized"
  | "ambiguous"
  | "missing"
  | "blocking";

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
  if (findings.some((f) => f.category === "ambiguous" || f.category === "missing")) {
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
