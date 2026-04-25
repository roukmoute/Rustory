import { invoke } from "@tauri-apps/api/core";

import type {
  ExportStoryDialogInput,
  ExportStoryDialogOutcome,
} from "../../shared/ipc-contracts/import-export";
import { isExportStoryDialogOutcome } from "../../shared/ipc-contracts/import-export";

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
