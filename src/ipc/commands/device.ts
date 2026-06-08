import { invoke } from "@tauri-apps/api/core";

import {
  isConnectedDeviceDto,
  type ConnectedDeviceDto,
} from "../../shared/ipc-contracts/device";

/**
 * Upper bound for a device-scan IPC call. Sized to the Rust budget
 * (4 s) plus a 500 ms safety margin so the timer never fires before the
 * scan loop has had a chance to truncate cleanly. NFR4 caps the
 * end-to-end at 5 s.
 */
export const READ_CONNECTED_LUNII_TIMEOUT_MS = 4500;

/** Discriminant emitted when {@link readConnectedLunii} trips its timeout. */
export const READ_CONNECTED_LUNII_TIMEOUT_ERROR = {
  code: "UNKNOWN",
  message: "Rustory a mis trop de temps à détecter l'appareil.",
  userAction:
    "Réessaie la détection. Si le problème persiste, débranche l'appareil puis rebranche-le.",
  details: null,
} as const;

/**
 * Cancelable handle returned by {@link readConnectedLunii}. Callers that
 * unmount before the IPC settles MUST call `cancel()` so the timer
 * guard is cleared — otherwise a long-lived timer would fire after
 * unmount and accumulate timers across route switches.
 */
export interface ReadConnectedLuniiCall {
  promise: Promise<ConnectedDeviceDto>;
  cancel: () => void;
}

/**
 * Thrown when `read_connected_lunii` returns a payload that does not
 * match the canonical wire shape (drift in `kind`, missing fields,
 * unrecognized enum strings). The captured `raw` value is kept on the
 * error instance for support / debugging — never surfaced verbatim to
 * the user.
 */
export class ReadConnectedLuniiContractDriftError extends Error {
  public readonly raw: unknown;
  constructor(message: string, options: { raw: unknown }) {
    super(message);
    this.name = "ReadConnectedLuniiContractDriftError";
    this.raw = options.raw;
  }
}

/**
 * Probe the system for a connected supported Lunii. Resolves with a
 * `ConnectedDeviceDto` (tagged enum on `kind`) once the Rust scan
 * completes; rejects with a normalized `AppError` on a scan-side
 * failure (`DEVICE_SCAN_FAILED`); rejects with a synthetic
 * `UNKNOWN`-coded error if the Rust side does not answer within
 * {@link READ_CONNECTED_LUNII_TIMEOUT_MS}.
 *
 * Components MUST NOT call `invoke` directly — go through this façade
 * so the wire contract stays owned by `src/ipc/`.
 */
export function readConnectedLunii(
  timeoutMs: number = READ_CONNECTED_LUNII_TIMEOUT_MS,
): ReadConnectedLuniiCall {
  const call = invoke<unknown>("read_connected_lunii").then((raw) => {
    if (!isConnectedDeviceDto(raw)) {
      throw new ReadConnectedLuniiContractDriftError(
        "ConnectedDeviceDto wire shape drifted from the canonical contract.",
        { raw },
      );
    }
    return raw;
  });

  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;

  const guard = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      timer = undefined;
      if (cancelled) return;
      reject(READ_CONNECTED_LUNII_TIMEOUT_ERROR);
    }, timeoutMs);
  });

  const promise = Promise.race([call, guard]).finally(() => {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  });

  const cancel = (): void => {
    cancelled = true;
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  };

  return { promise, cancel };
}
