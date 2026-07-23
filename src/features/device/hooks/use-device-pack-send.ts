import { useCallback, useEffect, useRef, useState } from "react";

import { sendPackToDevice } from "../../../ipc/commands/device-send";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { SendPackToDeviceOutcome } from "../../../shared/ipc-contracts/device-send";

export type DevicePackSendStatus =
  | { kind: "idle" }
  | { kind: "sending" }
  | { kind: "sent"; packUuid: string; imageCount: number; audioCount: number }
  | { kind: "failed"; error: AppError };

export interface UseDevicePackSend {
  status: DevicePackSendStatus;
  /** Open the native archive picker then run a single pack send. Re-entrant
   *  calls are swallowed while one is in flight. A dismissed picker settles
   *  back to `idle` silently (a non-event). Resolves when the flow settles. */
  triggerSend(deviceIdentifier: string): Promise<void>;
  /** Dismiss the current status back to idle (success AND failure alike). */
  dismissStatus(): void;
}

export interface UseDevicePackSendOptions {
  /** Called after a send settles successfully, while the hook is still
   *  mounted. The route uses it to re-read the device inventory so the new
   *  pack appears. */
  onSent?: (outcome: Extract<SendPackToDeviceOutcome, { kind: "sent" }>) => void;
}

/**
 * Orchestrates a single pack-archive send through the Rust-owned boundary
 * (native picker + re-scan + `send_archive` gate + ciphered atomic write).
 * Structural sibling of `useDeviceStoryDelete`: the same StrictMode-safe
 * mount flag, synchronous re-entrancy guard and settled statuses — the
 * picker itself is the user's confirmation, so the send starts as soon as a
 * file is chosen.
 */
export function useDevicePackSend(
  options?: UseDevicePackSendOptions,
): UseDevicePackSend {
  const [status, setStatus] = useState<DevicePackSendStatus>({ kind: "idle" });

  const onSentRef = useRef<UseDevicePackSendOptions["onSent"]>(options?.onSent);
  onSentRef.current = options?.onSent;

  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const inFlightRef = useRef(false);

  const triggerSend = useCallback(
    async (deviceIdentifier: string): Promise<void> => {
      if (inFlightRef.current) return;
      inFlightRef.current = true;
      try {
        if (mountedRef.current) {
          setStatus({ kind: "sending" });
        }
        let outcome: SendPackToDeviceOutcome;
        try {
          outcome = await sendPackToDevice({ deviceIdentifier });
        } catch (err) {
          if (!mountedRef.current) return;
          setStatus({ kind: "failed", error: toAppError(err) });
          return;
        }
        if (!mountedRef.current) return;
        if (outcome.kind === "cancelled") {
          // A dismissed picker is a non-event: back to idle, no status.
          setStatus({ kind: "idle" });
          return;
        }
        setStatus({
          kind: "sent",
          packUuid: outcome.packUuid,
          imageCount: outcome.imageCount,
          audioCount: outcome.audioCount,
        });
        // The device inventory changed → let the route re-read it so the new
        // pack appears. Guarded so a throwing callback never reclassifies a
        // committed send.
        try {
          onSentRef.current?.(outcome);
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
  }, []);

  return { status, triggerSend, dismissStatus };
}
