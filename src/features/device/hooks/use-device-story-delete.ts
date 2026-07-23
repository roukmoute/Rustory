import { useCallback, useEffect, useRef, useState } from "react";

import { deleteDeviceStory } from "../../../ipc/commands/device-delete";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { DeleteDeviceStoryOutcome } from "../../../shared/ipc-contracts/device-delete";

export type DeviceStoryDeleteStatus =
  | { kind: "idle" }
  | { kind: "deleting" }
  | { kind: "deleted"; wasPresent: boolean }
  | { kind: "failed"; error: AppError };

export interface UseDeviceStoryDelete {
  status: DeviceStoryDeleteStatus;
  /** Pack UUID the current `status` belongs to, or `null` when idle. Set ONLY
   *  once a delete actually starts (past the re-entrancy guard), so a caller
   *  can gate "is this status mine?" on `targetPackUuid === <selected uuid>`. */
  targetPackUuid: string | null;
  /** Fire a single device-story delete. Re-entrant calls are swallowed while
   *  one is in flight. Resolves when the flow settles. */
  triggerDelete(deviceIdentifier: string, packUuid: string): Promise<void>;
  /** Dismiss the current status back to idle (success AND failure alike). */
  dismissStatus(): void;
}

export interface UseDeviceStoryDeleteOptions {
  /** Called after a delete settles successfully, with the wire outcome, while
   *  the hook is still mounted. The route uses it to re-read the device
   *  inventory so the deleted entry disappears. */
  onDeleted?: (outcome: DeleteDeviceStoryOutcome) => void;
}

/**
 * Orchestrates a single device-story delete through the Rust-owned boundary.
 * Structural sibling of `useDeviceStoryImport`: the same StrictMode-safe mount
 * flag, synchronous re-entrancy guard and status scoping — a destructive but
 * non-blocking operation (a confirmation is the caller's concern; the hook
 * itself starts immediately once triggered).
 */
export function useDeviceStoryDelete(
  options?: UseDeviceStoryDeleteOptions,
): UseDeviceStoryDelete {
  const [status, setStatus] = useState<DeviceStoryDeleteStatus>({
    kind: "idle",
  });
  const [targetPackUuid, setTargetPackUuid] = useState<string | null>(null);

  const onDeletedRef = useRef<UseDeviceStoryDeleteOptions["onDeleted"]>(
    options?.onDeleted,
  );
  onDeletedRef.current = options?.onDeleted;

  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const inFlightRef = useRef(false);

  const triggerDelete = useCallback(
    async (deviceIdentifier: string, packUuid: string): Promise<void> => {
      if (inFlightRef.current) return;
      inFlightRef.current = true;
      try {
        if (mountedRef.current) {
          setTargetPackUuid(packUuid);
          setStatus({ kind: "deleting" });
        }
        let outcome: DeleteDeviceStoryOutcome;
        try {
          outcome = await deleteDeviceStory({ deviceIdentifier, packUuid });
        } catch (err) {
          if (!mountedRef.current) return;
          setStatus({ kind: "failed", error: toAppError(err) });
          return;
        }
        if (!mountedRef.current) return;
        setStatus({ kind: "deleted", wasPresent: outcome.wasPresent });
        // The device inventory changed → let the route re-read it so the
        // deleted entry disappears. Guarded so a throwing callback never
        // reclassifies a committed delete.
        try {
          onDeletedRef.current?.(outcome);
        } catch {
          // Deliberately swallowed.
        }
      } finally {
        inFlightRef.current = false;
      }
    },
    [],
  );

  const dismissStatus = useCallback((): void => {
    setStatus({ kind: "idle" });
    setTargetPackUuid(null);
  }, []);

  return { status, targetPackUuid, triggerDelete, dismissStatus };
}
