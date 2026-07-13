import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  AcceptArtifactImportInput,
  AcceptStructuredCreationInput,
  ExportStoryDialogInput,
  ExportStoryDialogOutcome,
  ImportArtifactAnalysis,
  RssCreationOutcome,
  RssItemRef,
  RssPreview,
  StructuredCreationAnalysis,
} from "../../shared/ipc-contracts/import-export";
import {
  isExportStoryDialogOutcome,
  isImportArtifactAnalysis,
  isImportFinding,
  isRssPreview,
  isStructuredCreationAnalysis,
} from "../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../shared/ipc-contracts/library";
import { isStoryCardDto } from "../../shared/ipc-contracts/library";

/**
 * Error thrown when `export_story_with_save_dialog` returns a payload
 * that does not match the wire contract. The raw response is attached
 * to `raw` so production debugging surfaces the shape that drifted
 * (instead of "something broke" without context).
 */
export class ExportStoryContractDriftError extends Error {
  readonly raw: unknown;
  constructor(raw: unknown) {
    super(
      "export_story_with_save_dialog returned a payload that does not match the contract",
    );
    this.name = "ExportStoryContractDriftError";
    this.raw = raw;
  }
}

/**
 * Open the native save dialog and, if the user confirms a destination,
 * persist the currently stored story as a `.rustory` artifact there.
 * The Rust side owns the dialog, the validation, and the disk I/O — the
 * frontend never sees (or constructs) the absolute filesystem path.
 *
 * A cancelled dialog resolves with `{ kind: "cancelled" }`; the caller
 * MUST treat that as a silent no-op (no alert, no chip). Unrecoverable
 * failures (permissions, I/O, consistency) reject with a normalized
 * `AppError`.
 *
 * The response is validated via [`isExportStoryDialogOutcome`]; a shape
 * mismatch rejects with [`ExportStoryContractDriftError`] so the UI
 * never renders against an arbitrary object.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so
 * the wire contract stays owned by `src/ipc/`.
 */
export async function exportStoryWithSaveDialog(
  input: ExportStoryDialogInput,
): Promise<ExportStoryDialogOutcome> {
  const raw = await invoke<unknown>("export_story_with_save_dialog", {
    input,
  });
  if (!isExportStoryDialogOutcome(raw)) {
    throw new ExportStoryContractDriftError(raw);
  }
  return raw;
}

/**
 * Error thrown when a local-artifact import command returns a payload that
 * does not match the wire contract. The raw response is attached to `raw`
 * so production debugging surfaces the shape that drifted.
 */
export class ImportArtifactContractDriftError extends Error {
  readonly raw: unknown;
  constructor(command: string, raw: unknown) {
    super(`${command} returned a payload that does not match the contract`);
    this.name = "ImportArtifactContractDriftError";
    this.raw = raw;
  }
}

/**
 * Open the native file picker and analyze the chosen `.rustory` artifact
 * (phase 1, NO mutation). Rust owns the dialog, the bounded read and the
 * recognition verdict — the frontend never sees (or constructs) the
 * absolute filesystem path.
 *
 * A cancelled dialog resolves with `{ kind: "cancelled" }`; the caller MUST
 * treat that as a silent no-op. A TRANSPORT failure (file unreadable,
 * dialog backend) rejects with a normalized `AppError`. The functional
 * verdict (bad version, corruption, normalized title) is the resolved value,
 * never a rejection.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so the
 * wire contract stays owned by `src/ipc/`.
 */
export async function analyzeArtifactForImport(): Promise<ImportArtifactAnalysis> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("analyze_artifact_for_import");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isImportArtifactAnalysis(raw)) {
    throw new ImportArtifactContractDriftError("analyze_artifact_for_import", raw);
  }
  return raw;
}

/**
 * Commit a recognized artifact (phase 2). Sends the validated content from a
 * prior analysis; Rust re-validates it from zero before the canonical commit
 * and returns the created local Story Card. A failure rejects with a
 * normalized `AppError`.
 */
export async function acceptArtifactImport(
  input: AcceptArtifactImportInput,
): Promise<StoryCardDto> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("accept_artifact_import", { input });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isStoryCardDto(raw)) {
    throw new ImportArtifactContractDriftError("accept_artifact_import", raw);
  }
  return raw;
}

/**
 * Error thrown when a structured-folder creation command returns a payload
 * that does not match the wire contract. A payload outside the contract
 * NEVER renders a screen — the raw response is attached for debugging.
 */
export class StructuredCreationContractDriftError extends Error {
  readonly raw: unknown;
  constructor(command: string, raw: unknown) {
    super(`${command} returned a payload that does not match the contract`);
    this.name = "StructuredCreationContractDriftError";
    this.raw = raw;
  }
}

/**
 * Open the native FOLDER picker and analyze the chosen structured folder
 * (phase 1, NO mutation). Rust owns the dialog, the bounded reads and the
 * recognition verdict; the returned `folderPath` exists only to be passed
 * back to [`acceptStructuredCreation`] — never rendered, never persisted,
 * never logged.
 *
 * A cancelled dialog resolves with `{ kind: "cancelled" }` (silent no-op).
 * A TRANSPORT failure rejects with a normalized `AppError`; the functional
 * verdict (manifest absent, media missing…) is the resolved value, never a
 * rejection. A drifted payload rejects with
 * [`StructuredCreationContractDriftError`].
 *
 * Components MUST NOT call `invoke` directly — go through this facade so
 * the wire contract stays owned by `src/ipc/`.
 */
export async function analyzeStructuredFolderForCreation(): Promise<StructuredCreationAnalysis> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("analyze_structured_folder_for_creation");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isStructuredCreationAnalysis(raw)) {
    throw new StructuredCreationContractDriftError(
      "analyze_structured_folder_for_creation",
      raw,
    );
  }
  return raw;
}

/**
 * Commit an analyzed structured folder (phase 2). Sends the folder pointer
 * back; Rust RE-ANALYZES the disk from zero (the wire is never an
 * authority) and returns the created local Story Card. A failure rejects
 * with a normalized `AppError`; a drifted payload rejects with
 * [`StructuredCreationContractDriftError`].
 */
export async function acceptStructuredCreation(
  input: AcceptStructuredCreationInput,
): Promise<StoryCardDto> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("accept_structured_creation", { input });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isStoryCardDto(raw)) {
    throw new StructuredCreationContractDriftError(
      "accept_structured_creation",
      raw,
    );
  }
  return raw;
}

/**
 * Error thrown when an RSS external-source command returns a payload that
 * does not match the wire contract. A payload outside the contract NEVER
 * renders a screen — the raw response is attached for debugging.
 */
export class RssCreationContractDriftError extends Error {
  readonly raw: unknown;
  constructor(command: string, raw: unknown) {
    super(`${command} returned a payload that does not match the contract`);
    this.name = "RssCreationContractDriftError";
    this.raw = raw;
  }
}

/**
 * Fetch + analyze a user-provided RSS feed (phase 1, NO mutation, NO DB —
 * the preview is pure). The ONLY networked action of the flow, on the
 * explicit `Récupérer le flux` click. Rust owns the address validation,
 * the bounded fetch and the bounded parse.
 *
 * A feed-CONTENT problem (unreadable XML, a non-RSS root, zero exploitable
 * item) is the resolved BLOCKED verdict, never a rejection; only TRANSPORT
 * (invalid address, unreachable source, over-cap response) rejects with a
 * normalized `AppError`. A drifted payload rejects with
 * [`RssCreationContractDriftError`].
 *
 * Components MUST NOT call `invoke` directly — go through this facade so
 * the wire contract stays owned by `src/ipc/`.
 */
export async function fetchRssSourcePreview(
  feedUrl: string,
): Promise<RssPreview> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("fetch_rss_source_preview", { feedUrl });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isRssPreview(raw)) {
    throw new RssCreationContractDriftError("fetch_rss_source_preview", raw);
  }
  return raw;
}

/** The validated accept outcome: the created card, or the honest
 *  `sourceChanged` refusal (nothing was created). */
export type RssStoryCreationOutcome = RssCreationOutcome<StoryCardDto>;

function isRssStoryCreationOutcome(
  value: unknown,
): value is RssStoryCreationOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.kind === "sourceChanged") {
    return Object.keys(c).length === 1;
  }
  if (c.kind !== "created") return false;
  if (!isStoryCardDto(c.story)) return false;
  return Array.isArray(c.report) && c.report.every(isImportFinding);
}

/**
 * Commit one previewed feed item into a canonical local draft (phase 2).
 * Sends the feed address + the item reference back; Rust RE-FETCHES and
 * re-parses the feed from zero (the source is the authority — the wire
 * carries a pointer, never content). A diverged source resolves with the
 * typed `{ kind: "sourceChanged" }` refusal (nothing created); only
 * transport rejects with a normalized `AppError`. A drifted payload
 * rejects with [`RssCreationContractDriftError`].
 */
export async function acceptRssStoryCreation(
  feedUrl: string,
  itemRef: RssItemRef,
): Promise<RssStoryCreationOutcome> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("accept_rss_story_creation", {
      feedUrl,
      itemRef,
    });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isRssStoryCreationOutcome(raw)) {
    throw new RssCreationContractDriftError("accept_rss_story_creation", raw);
  }
  return raw;
}
