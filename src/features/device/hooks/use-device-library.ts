import { useCallback, useEffect, useRef, useState } from "react";

import {
  readDeviceLibrary,
  ReadDeviceLibraryContractDriftError,
} from "../../../ipc/commands/device-library";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";

export type DeviceLibraryState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; stories: DeviceStoryDto[]; deviceIdentifier: string }
  | { kind: "error"; error: AppError };

const DRIFT_ERROR: AppError = {
  code: "DEVICE_SCAN_FAILED",
  message: "Lecture de la bibliothèque appareil indisponible: réponse invalide.",
  userAction:
    "Réessaie la lecture. Si le problème persiste, signale-le avec les traces locales.",
  details: null,
};

export interface UseDeviceLibrary {
  state: DeviceLibraryState;
  /** True while a refresh runs on top of an already-displayed snapshot. */
  isRefreshing: boolean;
  /** User-triggered re-read of the current device. No-op when there is no
   *  readable device. */
  refresh: () => void;
}

/**
 * Module-local SWR cache keyed by `deviceIdentifier`, shared across hook
 * instances. Holds ONLY the readable inventory — never the device truth
 * (which lives in `useConnectedLunii`). Not Zustand: a forbidden "device
 * content mirror" must not become continuity state.
 */
const cache = new Map<string, DeviceStoryDto[]>();

export function invalidateDeviceLibraryCache(): void {
  cache.clear();
}

/**
 * Read the device-side library for the supported, read-authorized device
 * identified by `deviceIdentifier`. Pass `null` when no such device is
 * connected — the hook then sits in `idle` and issues no IPC.
 *
 * Guardrails:
 * - orthogonal to `useLibraryOverview`: a device-read failure never
 *   touches the LOCAL library (AC #3 — local stays intact).
 * - re-reads when `deviceIdentifier` changes (a different Lunii plugged);
 *   clears to `idle` when it goes `null` (device gone/unsupported).
 * - StrictMode-safe active-call guard so a superseded response cannot
 *   overwrite a fresher state; cancel() on unmount clears the timer.
 * - NO polling: device PRESENCE is polled by `useConnectedLunii`; the
 *   (heavier) inventory read happens on identifier change + manual refresh.
 */
export function useDeviceLibrary(
  deviceIdentifier: string | null,
): UseDeviceLibrary {
  const [state, setState] = useState<DeviceLibraryState>(() => {
    if (deviceIdentifier && cache.has(deviceIdentifier)) {
      return {
        kind: "ready",
        stories: cache.get(deviceIdentifier) as DeviceStoryDto[],
        deviceIdentifier,
      };
    }
    return deviceIdentifier ? { kind: "loading" } : { kind: "idle" };
  });
  const [isRefreshing, setIsRefreshing] = useState(false);
  const activeCallRef = useRef(0);
  const mountedRef = useRef(true);
  const cancelRef = useRef<(() => void) | null>(null);

  const load = useCallback((identifier: string) => {
    const callId = ++activeCallRef.current;
    const cached = cache.get(identifier);
    if (cached) {
      setState({ kind: "ready", stories: cached, deviceIdentifier: identifier });
      setIsRefreshing(true);
    } else {
      setState({ kind: "loading" });
      setIsRefreshing(false);
    }
    if (cancelRef.current) {
      cancelRef.current();
      cancelRef.current = null;
    }

    const handle = readDeviceLibrary(identifier);
    cancelRef.current = handle.cancel;

    handle.promise
      .then((dto) => {
        if (!mountedRef.current) return;
        if (callId !== activeCallRef.current) return;
        cancelRef.current = null;
        setIsRefreshing(false);
        if (dto.kind === "readable") {
          cache.set(identifier, dto.stories);
          setState({
            kind: "ready",
            stories: dto.stories,
            deviceIdentifier: identifier,
          });
        } else {
          // `none` / `unsupported`: the live re-scan no longer resolves to
          // a readable Lunii (unplugged or swapped between the detection
          // poll and this read). Drop the device section gracefully — the
          // decision panel already communicates the device state, and the
          // LOCAL library is untouched.
          cache.delete(identifier);
          setState({ kind: "idle" });
        }
      })
      .catch((err) => {
        if (!mountedRef.current) return;
        if (callId !== activeCallRef.current) return;
        cancelRef.current = null;
        setIsRefreshing(false);
        // A read transport failure (device disappeared mid-read, FS error,
        // timeout) is recoverable and shown IN CONTEXT — never a toast,
        // never a silent empty list, never a touch on the local library.
        if (err instanceof ReadDeviceLibraryContractDriftError) {
          setState({ kind: "error", error: DRIFT_ERROR });
        } else {
          setState({ kind: "error", error: toAppError(err) });
        }
      });
  }, []);

  const refresh = useCallback(() => {
    if (deviceIdentifier) load(deviceIdentifier);
  }, [deviceIdentifier, load]);

  useEffect(() => {
    mountedRef.current = true;
    if (deviceIdentifier) {
      load(deviceIdentifier);
    } else {
      // No readable device: supersede any in-flight read and reset to idle
      // so a late resolution cannot paint a stale device section.
      activeCallRef.current += 1;
      if (cancelRef.current) {
        cancelRef.current();
        cancelRef.current = null;
      }
      setState({ kind: "idle" });
      setIsRefreshing(false);
    }
    return () => {
      mountedRef.current = false;
      if (cancelRef.current) {
        cancelRef.current();
        cancelRef.current = null;
      }
    };
  }, [deviceIdentifier, load]);

  return { state, isRefreshing, refresh };
}
