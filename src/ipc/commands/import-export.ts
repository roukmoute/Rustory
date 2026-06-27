import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  AcceptArtifactImportInput,
  ExportStoryDialogInput,
  ExportStoryDialogOutcome,
  ImportArtifactAnalysis,
} from "../../shared/ipc-contracts/import-export";
import {
  isExportStoryDialogOutcome,
  isImportArtifactAnalysis,
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
