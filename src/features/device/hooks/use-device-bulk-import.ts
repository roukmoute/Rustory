import { useCallback, useEffect, useRef, useState } from "react";

import {
  ImportDeviceStoryContractDriftError,
  importDeviceStory,
} from "../../../ipc/commands/device-import";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";

export type DeviceBulkImportStatus =
  | { kind: "idle" }
  | {
      kind: "running";
      total: number;
      done: number;
      succeeded: number;
      failed: number;
    }
  | {
      kind: "done";
      total: number;
      succeeded: number;
      failed: number;
      /** The first failure encountered, kept so the summary can name a
       *  cause without drowning the user in one alert per failed pack. */
      firstError: AppError | null;
    };

export interface UseDeviceBulkImport {
  status: DeviceBulkImportStatus;
  /** Import every pack in `packUuids` from `deviceIdentifier`, SEQUENTIALLY.
   *  One pack's failure never aborts the batch — the rest still copy, and the
   *  outcome carries the succeeded/failed tally. Re-entrant calls are
   *  swallowed while a batch is in flight. Resolves when the batch settles. */
  start(deviceIdentifier: string, packUuids: readonly string[]): Promise<void>;
  /** Dismiss the terminal `done` summary back to idle. No-op while running. */
  dismiss(): void;
}

export interface UseDeviceBulkImportOptions {
  /** Called once after the batch settles (whatever the tally), while the hook
   *  is still mounted. The route uses it to run the authoritative re-reads
   *  (local overview + device inventory) a SINGLE time, not once per pack. */
  onCompleted?: () => void;
}

/**
 * Orchestrates a BULK device-story copy: the same Rust-owned single-copy
 * boundary (`importDeviceStory`) driven sequentially over a selection.
 * Sequential on purpose — the copies share one USB bus and the Rust side
 * bounds each one; firing them in parallel would contend, not accelerate.
 *
 * Same StrictMode-safe mount flag and synchronous re-entrancy guard as the
 * single-copy hook. Library coherence is preserved per success (the overview
 * cache is dropped as soon as a pack lands, even before the mounted guard),
 * so an unmount mid-batch never leaves a stale snapshot; the visible progress
 * is the only thing lost in that window (the same trade-off the single flow
 * documents).
 */
export function useDeviceBulkImport(
  options?: UseDeviceBulkImportOptions,
): UseDeviceBulkImport {
  const [status, setStatus] = useState<DeviceBulkImportStatus>({
    kind: "idle",
  });

  const statusRef = useRef<DeviceBulkImportStatus>(status);
  statusRef.current = status;

  const onCompletedRef = useRef<UseDeviceBulkImportOptions["onCompleted"]>(
    options?.onCompleted,
  );
  onCompletedRef.current = options?.onCompleted;

  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const inFlightRef = useRef(false);

  const start = useCallback(
    async (
      deviceIdentifier: string,
      packUuids: readonly string[],
    ): Promise<void> => {
      if (inFlightRef.current) return;
      if (packUuids.length === 0) return;
      inFlightRef.current = true;

      const total = packUuids.length;
      let succeeded = 0;
      let failed = 0;
      let firstError: AppError | null = null;

      try {
        if (mountedRef.current) {
          setStatus({ kind: "running", total, done: 0, succeeded, failed });
        }

        for (let i = 0; i < packUuids.length; i += 1) {
          const packUuid = packUuids[i]!;
          try {
            await importDeviceStory({ deviceIdentifier, packUuid });
            succeeded += 1;
            // Coherence: the canonical store changed → drop the stale
            // overview snapshot now, before any mounted guard.
            invalidateLibraryOverviewCache();
          } catch (err) {
            failed += 1;
            // A contract drift still committed the import in Rust: the
            // snapshot is stale regardless of the unrenderable outcome.
            if (err instanceof ImportDeviceStoryContractDriftError) {
              invalidateLibraryOverviewCache();
            }
            if (firstError === null) firstError = toAppError(err);
            // Deliberately continue: one pack's failure never aborts the
            // batch — the remaining selection still copies.
          }
          if (mountedRef.current) {
            setStatus({
              kind: "running",
              total,
              done: i + 1,
              succeeded,
              failed,
            });
          }
        }

        // The batch settled. Trigger the route's authoritative re-reads ONCE,
        // outside any per-pack path, and never let a throwing callback
        // reclassify a committed batch.
        try {
          onCompletedRef.current?.();
        } catch {
          // Deliberately swallowed.
        }

        if (mountedRef.current) {
          setStatus({ kind: "done", total, succeeded, failed, firstError });
        }
      } finally {
        inFlightRef.current = false;
      }
    },
    [],
  );

  const dismiss = useCallback((): void => {
    if (statusRef.current.kind !== "idle") {
      setStatus({ kind: "idle" });
    }
  }, []);

  return { status, start, dismiss };
}
