import { useCallback } from "react";

import { exportStoryArchiveWithSaveDialog } from "../../../ipc/commands/import-export";
import { toAppError } from "../../../shared/errors/app-error";
import {
  useArchiveExportShell,
  type StoryArchiveExportStatus,
} from "../../../shell/state/archive-export-shell-store";
import { sanitizeFilename } from "../lib/sanitize-filename";

export type { StoryArchiveExportStatus };

export interface UseStoryArchiveExport {
  status: StoryArchiveExportStatus;
  /** An export is in flight (the save dialog is open, or the archive is
   *  being written). One export at a time. */
  isExporting: boolean;
  /** Export `storyId` as a shareable archive: Rust opens the native save
   *  dialog (suggesting `<title>.zip`), converts the media and writes the
   *  file. A cancelled dialog is a silent no-op. Swallowed while another
   *  export is in flight. Resolves when the flow has settled. */
  triggerExport(storyId: string, storyTitle: string): Promise<void>;
  /** Re-fire the export of the story a `failed` status concerns — the
   *  dialog opens again. No-op in any other state. */
  retryExport(): Promise<void>;
  /** Dismiss a settled status (`exported` or `failed`) back to idle. An
   *  export in flight cannot be dismissed. */
  dismissStatus(): void;
}

/**
 * Orchestrates the story archive export through the Rust-owned boundary
 * (plan → save dialog → media conversion → atomic write). Strictly
 * read-only against the library — never invalidates the overview cache,
 * whatever the outcome.
 *
 * The status lives in the shell store, not in the calling screen: an
 * export that outlasts the screen (the user opened the editor meanwhile)
 * keeps being tracked, and every caller sees the same single export.
 */
export function useStoryArchiveExport(): UseStoryArchiveExport {
  const status = useArchiveExportShell((shell) => shell.status);

  const triggerExport = useCallback(
    (storyId: string, storyTitle: string): Promise<void> =>
      performExport(storyId, storyTitle),
    [],
  );

  const retryExport = useCallback((): Promise<void> => {
    const current = useArchiveExportShell.getState().status;
    if (current.kind !== "failed") return Promise.resolve();
    return performExport(current.storyId, current.storyTitle);
  }, []);

  const dismissStatus = useCallback((): void => {
    const shell = useArchiveExportShell.getState();
    if (shell.status.kind === "exported" || shell.status.kind === "failed") {
      shell.setStatus({ kind: "idle" });
    }
  }, []);

  return {
    status,
    isExporting: status.kind === "exporting",
    triggerExport,
    retryExport,
    dismissStatus,
  };
}

async function performExport(
  storyId: string,
  storyTitle: string,
): Promise<void> {
  const shell = useArchiveExportShell.getState();
  // One export at a time: the in-flight status is the (synchronous)
  // re-entrancy guard, shared by every screen.
  if (shell.status.kind === "exporting") return;
  // A settled status the user may still be reading: a cancelled dialog
  // restores it rather than silently wiping it.
  const prior = shell.status;
  shell.setStatus({ kind: "exporting", storyId, storyTitle, percent: null });

  try {
    const outcome = await exportStoryArchiveWithSaveDialog(
      {
        storyId,
        suggestedFilename: `${sanitizeFilename(storyTitle)}.zip`,
      },
      (percent) => {
        useArchiveExportShell.getState().setProgress(storyId, percent);
      },
    );
    if (outcome.kind === "cancelled") {
      useArchiveExportShell.getState().setStatus(prior);
      return;
    }
    useArchiveExportShell.getState().setStatus({
      kind: "exported",
      storyTitle,
      destinationPath: outcome.destinationPath,
      bytesWritten: outcome.bytesWritten,
      imageCount: outcome.imageCount,
      audioCount: outcome.audioCount,
    });
  } catch (err) {
    useArchiveExportShell.getState().setStatus({
      kind: "failed",
      storyId,
      storyTitle,
      error: toAppError(err),
    });
  }
}
