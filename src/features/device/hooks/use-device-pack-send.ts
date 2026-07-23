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
  /** Story id the current `status` belongs to, or `null` when idle. Set ONLY
   *  once a send actually starts (past the re-entrancy guard), so a caller can
   *  gate "is this status mine?" on `targetStoryId === <selected id>` — the
   *  same scoping as the delete/import flows. */
  targetStoryId: string | null;
  /** Send the SELECTED story to the device — the V3 branch of the single
   *  "Envoyer vers la Lunii" gesture. Re-entrant calls are swallowed while
   *  one is in flight. Resolves when the flow settles. */
  triggerSend(deviceIdentifier: string, storyId: string): Promise<void>;
  /** Dismiss the current status back to idle (success AND failure alike). */
  dismissStatus(): void;
}

export interface UseDevicePackSendOptions {
  /** Called after a send settles successfully, while the hook is still
   *  mounted. The route uses it to re-read the device inventory so the new
   *  pack appears. */
  onSent?: (outcome: SendPackToDeviceOutcome) => void;
}

/**
 * Orchestrates the V3 story send through the Rust-owned boundary (re-scan +
 * `send_archive` gate + transcode + per-device ciphering + atomic write),
 * sourced from the story's retained archive. Structural sibling of
 * `useDeviceStoryDelete`: the same StrictMode-safe mount flag, synchronous
 * re-entrancy guard and settled statuses — a single CTA click, no picker.
 */
export function useDevicePackSend(
  options?: UseDevicePackSendOptions,
): UseDevicePackSend {
  const [status, setStatus] = useState<DevicePackSendStatus>({ kind: "idle" });
  const [targetStoryId, setTargetStoryId] = useState<string | null>(null);

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
    async (deviceIdentifier: string, storyId: string): Promise<void> => {
      if (inFlightRef.current) return;
      inFlightRef.current = true;
      try {
        if (mountedRef.current) {
          setTargetStoryId(storyId);
          setStatus({ kind: "sending" });
        }
        let outcome: SendPackToDeviceOutcome;
        try {
          outcome = await sendPackToDevice({ deviceIdentifier, storyId });
        } catch (err) {
          if (!mountedRef.current) return;
          setStatus({ kind: "failed", error: toAppError(err) });
          return;
        }
        if (!mountedRef.current) return;
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
    setTargetStoryId(null);
  }, []);

  return { status, targetStoryId, triggerSend, dismissStatus };
}
