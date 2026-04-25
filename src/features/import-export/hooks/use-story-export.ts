import { useCallback, useEffect, useRef, useState } from "react";

import { exportStoryWithSaveDialog } from "../../../ipc/commands/import-export";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";

export type ExportStatus =
  | { kind: "idle" }
  | { kind: "exporting" }
  | {
      kind: "exported";
      destinationPath: string;
      bytesWritten: number;
      contentChecksum: string;
    }
  | { kind: "failed"; error: AppError };

export interface UseStoryExport {
  status: ExportStatus;
  /** Open the native save dialog (owned by Rust) and fire a single
   *  export on confirmation. A cancelled dialog is a silent no-op.
   *
   *  Returns a Promise that resolves when the full flow has settled, so
   *  callers can chain an explicit teardown step (e.g. a test) if
   *  needed. */
  triggerExport(
    storyId: string,
    suggestedTitle: string,
  ): Promise<void>;
  /** Re-fire the export from a failed state using the last known
   *  suggestedTitle. Re-enters the Rust boundary — the dialog opens
   *  again because it is the boundary. No-op in any other state. */
  retryExport(): Promise<void>;
  /** Dismiss a non-failed non-idle status and return to idle. A
   *  `failed` status is preserved so the user can read the alert for
   *  as long as they need; use `retryExport` or an explicit cancel
   *  from the alert UI to leave that state. */
  dismissStatus(): void;
}

/**
 * Orchestrates a single-story export through the Rust-owned save
 * dialog + write boundary. Strictly read-only against the local
 * canonical state — never invalidates the library overview cache,
 * regardless of outcome.
 */
export function useStoryExport(): UseStoryExport {
  const [status, setStatus] = useState<ExportStatus>({ kind: "idle" });

  const statusRef = useRef<ExportStatus>(status);
  statusRef.current = status;

  // StrictMode-safe mount flag: an effect runs the mount phase on
  // every re-mount (after a synthetic unmount+remount), so the flag
  // must be *set* on every mount, not just initialized once.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Synchronous re-entrancy flag. `statusRef` reads the last rendered
  // state, which only flips to `exporting` AFTER the dialog resolves —
  // a second click fired while the dialog is still opening would
  // otherwise see `idle` and kick off a duplicate export.
  const inFlightRef = useRef(false);

  // Last suggested title, refreshed on every successful trigger so
  // `retryExport` can re-fire with the same suggestion.
  const lastTriggerRef = useRef<{
    storyId: string;
    suggestedTitle: string;
  } | null>(null);

  const performExport = useCallback(
    async (storyId: string, suggestedTitle: string): Promise<void> => {
      if (inFlightRef.current) return;
      inFlightRef.current = true;
      lastTriggerRef.current = { storyId, suggestedTitle };

      // Capture whether the user was looking at a failed alert BEFORE
      // the optimistic transition to `exporting`. A cancel that
      // follows MUST preserve that alert — silently wiping an error
      // the user was still reading is hostile.
      const priorFailed = statusRef.current.kind === "failed";
      const priorFailedStatus = priorFailed
        ? (statusRef.current as Extract<ExportStatus, { kind: "failed" }>)
        : null;

      try {
        // Optimistically transition to `exporting` BEFORE the IPC
        // round-trip so the button's disabled + busy state is visible
        // during the (potentially slow) Rust-side save dialog.
        if (mountedRef.current) {
          setStatus({ kind: "exporting" });
        }

        const suggestedFilename = `${suggestedTitle}.rustory`;

        try {
          const outcome = await exportStoryWithSaveDialog({
            storyId,
            suggestedFilename,
          });
          if (!mountedRef.current) return;

          if (outcome.kind === "cancelled") {
            if (priorFailedStatus) {
              // Restore the pre-existing alert — cancel does not
              // erase errors the user was still reading.
              setStatus(priorFailedStatus);
              return;
            }
            setStatus({ kind: "idle" });
            return;
          }

          setStatus({
            kind: "exported",
            destinationPath: outcome.destinationPath,
            bytesWritten: outcome.bytesWritten,
            contentChecksum: outcome.contentChecksum,
          });
        } catch (err) {
          if (!mountedRef.current) return;
          setStatus({ kind: "failed", error: toAppError(err) });
        }
      } finally {
        inFlightRef.current = false;
      }
    },
    [],
  );

  const triggerExport = useCallback(
    (storyId: string, suggestedTitle: string): Promise<void> =>
      performExport(storyId, suggestedTitle),
    [performExport],
  );

  const retryExport = useCallback(async (): Promise<void> => {
    if (statusRef.current.kind !== "failed") return;
    const trigger = lastTriggerRef.current;
    if (!trigger) return;
    await performExport(trigger.storyId, trigger.suggestedTitle);
  }, [performExport]);

  const dismissStatus = useCallback((): void => {
    if (
      statusRef.current.kind !== "idle" &&
      statusRef.current.kind !== "failed"
    ) {
      setStatus({ kind: "idle" });
    }
  }, []);

  return {
    status,
    triggerExport,
    retryExport,
    dismissStatus,
  };
}
