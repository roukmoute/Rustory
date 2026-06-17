import { useCallback, useEffect, useRef, useState } from "react";

import { setDeviceStoryTitle } from "../../../ipc/commands/device-title";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { DeviceStoryTitleDto } from "../../../shared/ipc-contracts/device-title";

export type SetDeviceStoryTitleStatus =
  | { kind: "idle" }
  | { kind: "saving" }
  | { kind: "failed"; error: AppError };

export interface UseDeviceStoryTitle {
  status: SetDeviceStoryTitleStatus;
  /** Pack UUID the current status belongs to, or `null` when idle. Set past
   *  the synchronous re-entrancy guard so a swallowed call never re-points
   *  it onto the wrong card. */
  targetPackUuid: string | null;
  /** Persist a user-typed title for `packUuid`. Resolves with `true` when
   *  the write committed (so the caller can close its editor) and `false`
   *  on a swallowed re-entrant call or a failure (the error is surfaced via
   *  `status`). Title validation is Rust-authoritative. */
  setTitle(packUuid: string, title: string): Promise<boolean>;
  /** Reset the status (and clear a failure) back to idle. */
  reset(): void;
}

export interface UseDeviceStoryTitleOptions {
  /** Called after a title is committed, ONLY while the hook is still
   *  mounted. The route re-reads the device library so the new title
   *  surfaces from the single Rust-owned resolution. */
  onTitled?: (packUuid: string, outcome: DeviceStoryTitleDto) => void;
}

/**
 * Orchestrates naming/renaming a device story through the Rust-owned write
 * boundary. Structural sibling of `useDeviceStoryImport`: same
 * StrictMode-safe mount flag and synchronous re-entrancy guard, minus the
 * retry/dialog mechanics (a re-name is a single bounded write — the user
 * just edits and saves again). The error (e.g. a too-long title) renders
 * in-context next to the editor, never as a toast.
 */
export function useDeviceStoryTitle(
  options?: UseDeviceStoryTitleOptions,
): UseDeviceStoryTitle {
  const [status, setStatus] = useState<SetDeviceStoryTitleStatus>({
    kind: "idle",
  });
  const [targetPackUuid, setTargetPackUuid] = useState<string | null>(null);

  const onTitledRef = useRef<UseDeviceStoryTitleOptions["onTitled"]>(
    options?.onTitled,
  );
  onTitledRef.current = options?.onTitled;

  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const inFlightRef = useRef(false);

  const setTitle = useCallback(
    async (packUuid: string, title: string): Promise<boolean> => {
      if (inFlightRef.current) return false;
      inFlightRef.current = true;
      try {
        if (mountedRef.current) {
          setTargetPackUuid(packUuid);
          setStatus({ kind: "saving" });
        }

        let outcome: DeviceStoryTitleDto;
        try {
          outcome = await setDeviceStoryTitle({ packUuid, title });
        } catch (err) {
          if (mountedRef.current) {
            setStatus({ kind: "failed", error: toAppError(err) });
          }
          return false;
        }

        if (mountedRef.current) {
          setStatus({ kind: "idle" });
          setTargetPackUuid(null);
          // Notify the route ONLY while still mounted — a post-unmount
          // callback (e.g. the route's device re-read) would touch a torn-down
          // tree. The write itself already committed regardless. The call is
          // wrapped so a throwing callback never reclassifies it as failed.
          try {
            onTitledRef.current?.(packUuid, outcome);
          } catch {
            // Deliberately swallowed.
          }
        }
        return true;
      } finally {
        inFlightRef.current = false;
      }
    },
    [],
  );

  const reset = useCallback((): void => {
    setStatus({ kind: "idle" });
    setTargetPackUuid(null);
  }, []);

  return { status, targetPackUuid, setTitle, reset };
}
