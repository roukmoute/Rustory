import { useCallback, useEffect, useRef, useState } from "react";

import { reorderDeviceStories } from "../../../ipc/commands/device-reorder";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import { movePackUuid } from "../../../shared/ipc-contracts/device-reorder";

export type DeviceStoryReorderStatus =
  | { kind: "idle" }
  | { kind: "moving"; packUuid: string }
  | { kind: "failed"; error: AppError };

export interface UseDeviceStoryReorder {
  status: DeviceStoryReorderStatus;
  /** Move one visible pack one step up (`-1`) or down (`+1`) among
   *  `visibleOrder` (the device's visible packs, in their current order)
   *  and write the new order to the device. Re-entrant calls are swallowed
   *  while one move is in flight; a move with no room is a no-op. */
  move(
    deviceIdentifier: string,
    visibleOrder: readonly string[],
    packUuid: string,
    direction: -1 | 1,
  ): Promise<void>;
  dismissStatus(): void;
}

export interface UseDeviceStoryReorderOptions {
  /** Called after a move settles successfully (the route re-reads the
   *  device inventory so the list reflects the device's own order). */
  onReordered?: () => void;
}

/**
 * Orchestrates one device reorder at a time through the Rust-owned boundary.
 * The device is the single truth: the hook never keeps an optimistic order —
 * a success triggers a re-read, a failure keeps the list as read.
 */
export function useDeviceStoryReorder(
  options?: UseDeviceStoryReorderOptions,
): UseDeviceStoryReorder {
  const [status, setStatus] = useState<DeviceStoryReorderStatus>({ kind: "idle" });
  const onReorderedRef = useRef(options?.onReordered);
  onReorderedRef.current = options?.onReordered;
  const mountedRef = useRef(true);
  const inFlightRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const move = useCallback<UseDeviceStoryReorder["move"]>(
    async (deviceIdentifier, visibleOrder, packUuid, direction) => {
      if (inFlightRef.current) return;
      const next = movePackUuid(visibleOrder, packUuid, direction);
      if (next.every((uuid, index) => uuid === visibleOrder[index])) return;
      inFlightRef.current = true;
      setStatus({ kind: "moving", packUuid });
      try {
        await reorderDeviceStories({
          deviceIdentifier,
          orderedPackUuids: next,
        });
        if (!mountedRef.current) return;
        setStatus({ kind: "idle" });
        onReorderedRef.current?.();
      } catch (err) {
        if (!mountedRef.current) return;
        setStatus({ kind: "failed", error: toAppError(err) });
      } finally {
        inFlightRef.current = false;
      }
    },
    [],
  );

  const dismissStatus = useCallback(() => setStatus({ kind: "idle" }), []);

  return { status, move, dismissStatus };
}
