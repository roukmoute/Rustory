import { useCallback, useEffect, useRef, useState } from "react";

import {
  ImportDeviceStoryContractDriftError,
  importDeviceStory,
} from "../../../ipc/commands/device-import";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";
import type { ImportDeviceStoryOutcome } from "../../../shared/ipc-contracts/device-import";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";

export type DeviceStoryImportStatus =
  | { kind: "idle" }
  | { kind: "importing" }
  | { kind: "imported"; story: StoryCardDto; packShortId: string }
  | { kind: "failed"; error: AppError };

export interface UseDeviceStoryImport {
  status: DeviceStoryImportStatus;
  /** Pack UUID the current `status` belongs to, or `null` when idle. Set
   *  ONLY when a copy actually starts (past the synchronous re-entrancy
   *  guard) — a trigger swallowed while another copy is in flight never
   *  re-points it, so a caller can safely gate "is this status mine?" on
   *  `targetPackUuid === <selected uuid>`. Cleared by `dismissStatus`. */
  targetPackUuid: string | null;
  /** Fire a single device-story copy. Returns a Promise that resolves
   *  when the full flow has settled so callers (and tests) can chain an
   *  explicit step after it. Re-entrant calls are swallowed while a copy
   *  is in flight. */
  triggerImport(deviceIdentifier: string, packUuid: string): Promise<void>;
  /** Re-fire the copy from a failed state using the last trigger's
   *  identifiers. No-op in any other state. */
  retryImport(): Promise<void>;
  /** Dismiss the current status back to idle — success AND failure
   *  alike. Nothing else ever wipes a `failed` alert implicitly (no
   *  auto-hide, no navigation side effect), but the alert's own
   *  explicit `Fermer` button must work. */
  dismissStatus(): void;
}

export interface UseDeviceStoryImportOptions {
  /** Called after a copy lands, with the wire outcome, ONLY while the
   *  hook is still mounted. The route uses it to orchestrate the
   *  post-success authoritative re-reads (local overview + device
   *  inventory). The overview CACHE invalidation itself does NOT depend
   *  on this callback — it runs before the mounted guard so coherence
   *  survives an unmount during the copy. */
  onImported?: (outcome: ImportDeviceStoryOutcome) => void;
}

/**
 * Orchestrates a single device-story copy through the Rust-owned
 * acquisition boundary. Structural clone of `useStoryExport`: same
 * StrictMode-safe mount flag, same synchronous re-entrancy guard and
 * retry mechanics — without the dialog-cancel branch (there is no
 * dialog: the copy is non-destructive and starts immediately), and with
 * a dismiss that also closes the failure alert (its explicit `Fermer`
 * must work; nothing else ever wipes it implicitly).
 *
 * Known residual limit (same trade-off as the export flow): the visual
 * feedback is lost if the user leaves the route while the copy is in
 * flight. The canonical state stays coherent (Rust finishes the copy and
 * the overview cache is invalidated regardless); a global notifier is a
 * deliberately deferred, separate concern.
 */
export function useDeviceStoryImport(
  options?: UseDeviceStoryImportOptions,
): UseDeviceStoryImport {
  const [status, setStatus] = useState<DeviceStoryImportStatus>({
    kind: "idle",
  });

  // Which pack the current status belongs to. Set past the re-entrancy
  // guard so a swallowed trigger (a second card clicked while a copy is in
  // flight) can never re-point it onto the wrong card.
  const [targetPackUuid, setTargetPackUuid] = useState<string | null>(null);

  const statusRef = useRef<DeviceStoryImportStatus>(status);
  statusRef.current = status;

  // Keep the latest callback in a ref so `performImport` stays stable
  // across renders while never calling a stale closure.
  const onImportedRef = useRef<UseDeviceStoryImportOptions["onImported"]>(
    options?.onImported,
  );
  onImportedRef.current = options?.onImported;

  // StrictMode-safe mount flag: set on every mount phase, not just the
  // first — a synthetic unmount+remount must re-arm it.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Synchronous re-entrancy flag: the rendered status flips to
  // `importing` only after a state flush, so a double-activation in the
  // same tick would otherwise start two copies.
  const inFlightRef = useRef(false);

  const lastTriggerRef = useRef<{
    deviceIdentifier: string;
    packUuid: string;
  } | null>(null);

  const performImport = useCallback(
    async (deviceIdentifier: string, packUuid: string): Promise<void> => {
      if (inFlightRef.current) return;
      inFlightRef.current = true;
      lastTriggerRef.current = { deviceIdentifier, packUuid };

      try {
        if (mountedRef.current) {
          // Attach the target ONLY now that the copy is actually
          // starting (past the guard above) — never on a swallowed call.
          setTargetPackUuid(packUuid);
          setStatus({ kind: "importing" });
        }

        let outcome: ImportDeviceStoryOutcome;
        try {
          outcome = await importDeviceStory({
            deviceIdentifier,
            packUuid,
          });
        } catch (err) {
          // A contract drift rejects AFTER Rust committed the import:
          // the canonical store HAS changed even though the outcome is
          // unrenderable. Drop the stale overview snapshot so the next
          // read shows the truth (a blind retry will surface the honest
          // `already_imported` refusal, not a phantom duplicate).
          if (err instanceof ImportDeviceStoryContractDriftError) {
            invalidateLibraryOverviewCache();
          }
          if (!mountedRef.current) return;
          setStatus({ kind: "failed", error: toAppError(err) });
          return;
        }

        // Library coherence: the canonical store HAS changed, so the
        // module-local overview snapshot is stale no matter what the
        // UI does next. Invalidate BEFORE the mounted guard so an
        // unmount during the copy still drops the stale cache.
        invalidateLibraryOverviewCache();
        if (!mountedRef.current) return;

        setStatus({
          kind: "imported",
          story: outcome.story,
          packShortId: outcome.packShortId,
        });
        // The route's orchestration (authoritative re-reads) runs OUTSIDE
        // the import's failure path: a throwing callback must never
        // reclassify a committed, already-rendered import as failed. The
        // re-reads it skipped happen at the next natural read anyway.
        try {
          onImportedRef.current?.(outcome);
        } catch {
          // Deliberately swallowed — see comment above.
        }
      } finally {
        inFlightRef.current = false;
      }
    },
    [],
  );

  const triggerImport = useCallback(
    (deviceIdentifier: string, packUuid: string): Promise<void> =>
      performImport(deviceIdentifier, packUuid),
    [performImport],
  );

  const retryImport = useCallback(async (): Promise<void> => {
    if (statusRef.current.kind !== "failed") return;
    const trigger = lastTriggerRef.current;
    if (!trigger) return;
    await performImport(trigger.deviceIdentifier, trigger.packUuid);
  }, [performImport]);

  const dismissStatus = useCallback((): void => {
    if (statusRef.current.kind !== "idle") {
      setStatus({ kind: "idle" });
      setTargetPackUuid(null);
    }
  }, []);

  return {
    status,
    targetPackUuid,
    triggerImport,
    retryImport,
    dismissStatus,
  };
}
