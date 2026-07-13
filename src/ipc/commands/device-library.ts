import { invoke } from "@tauri-apps/api/core";

import {
  isDeviceLibraryDto,
  type DeviceLibraryDto,
} from "../../shared/ipc-contracts/device-library";

/**
 * Upper bound for a device-library read. Sized to the Rust budget (5 s,
 * which itself covers the authoritative re-scan plus the inventory read)
 * plus a 500 ms safety margin so the timer never fires before the Rust
 * side has had a chance to return.
 */
export const READ_DEVICE_LIBRARY_TIMEOUT_MS = 5500;

/** Discriminant emitted when {@link readDeviceLibrary} trips its timeout.
 *  Device-generic copy: the read path serves every readable family
 *  (Lunii, FLAM), so the next gesture never names a specific one. */
export const READ_DEVICE_LIBRARY_TIMEOUT_ERROR = {
  code: "UNKNOWN",
  message: "Rustory a mis trop de temps à lire la bibliothèque de l'appareil.",
  userAction:
    "Réessaie la lecture. Si le problème persiste, débranche l'appareil puis rebranche-le.",
  details: null,
} as const;

/**
 * Cancelable handle returned by {@link readDeviceLibrary}. Callers that
 * unmount before the IPC settles MUST call `cancel()` so the timer guard
 * is cleared.
 */
export interface ReadDeviceLibraryCall {
  promise: Promise<DeviceLibraryDto>;
  cancel: () => void;
}

/**
 * Thrown when `read_device_library` returns a payload that does not match
 * the canonical wire shape. The captured `raw` value is kept on the error
 * for support — never surfaced verbatim to the user.
 */
export class ReadDeviceLibraryContractDriftError extends Error {
  public readonly raw: unknown;
  constructor(message: string, options: { raw: unknown }) {
    super(message);
    this.name = "ReadDeviceLibraryContractDriftError";
    this.raw = options.raw;
  }
}

/**
 * Read the installed-pack inventory of the connected supported Lunii
 * whose identifier is `deviceIdentifier`. Resolves with a
 * `DeviceLibraryDto` (tagged enum on `kind`); rejects with a normalized
 * `AppError` on a read-side failure (`DEVICE_SCAN_FAILED`); rejects with a
 * synthetic `UNKNOWN`-coded error on timeout.
 *
 * Components MUST NOT call `invoke` directly — go through this façade so
 * the wire contract stays owned by `src/ipc/`.
 */
export function readDeviceLibrary(
  deviceIdentifier: string,
  timeoutMs: number = READ_DEVICE_LIBRARY_TIMEOUT_MS,
): ReadDeviceLibraryCall {
  const call = invoke<unknown>("read_device_library", { deviceIdentifier }).then(
    (raw) => {
      if (!isDeviceLibraryDto(raw)) {
        throw new ReadDeviceLibraryContractDriftError(
          "DeviceLibraryDto wire shape drifted from the canonical contract.",
          { raw },
        );
      }
      return raw;
    },
  );

  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;

  const guard = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      timer = undefined;
      if (cancelled) return;
      reject(READ_DEVICE_LIBRARY_TIMEOUT_ERROR);
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
