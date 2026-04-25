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
