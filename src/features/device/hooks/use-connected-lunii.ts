import { useCallback, useEffect, useRef, useState } from "react";

import {
  readConnectedLunii,
  ReadConnectedLuniiContractDriftError,
} from "../../../ipc/commands/device";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { ConnectedDeviceDto } from "../../../shared/ipc-contracts/device";

export type ConnectedLuniiState =
  | { kind: "loading" }
  | { kind: "ready"; device: ConnectedDeviceDto }
  | { kind: "error"; error: AppError };

const DRIFT_ERROR: AppError = {
  code: "DEVICE_SCAN_FAILED",
  message: "Détection indisponible: réponse appareil invalide.",
  userAction:
    "Réessaie la détection. Si le problème persiste, signale-le avec les traces locales.",
  details: null,
};

/**
 * Polling cadence for the silent background hotplug check. 3 s gives
 * the user a perceptible reaction to plug/unplug events while staying
 * well under the NFR4 cap, and a single scan completes in tens of
 * milliseconds on the happy path — overhead is negligible.
 */
export const CONNECTED_LUNII_POLL_INTERVAL_MS = 3000;

export interface UseConnectedLunii {
  state: ConnectedLuniiState;
  /** Stale-while-revalidate snapshot kept across refresh cycles. */
  cached: ConnectedDeviceDto | null;
  /** True while a USER-VISIBLE refresh is in flight on top of a cached
   *  snapshot — distinct from the loading state (no data yet) and
   *  distinct from the silent background poll (which never flips this
   *  to avoid `Détection en cours…` flashing every 3 s). */
  isRefreshing: boolean;
  /** User-triggered re-scan. Supersedes any in-flight call. */
  refresh: () => void;
}

/**
 * Module-local SWR cache, shared across hook instances. Mirrors the
 * pattern used by `useLibraryOverview`: a route remount re-renders the
 * cached snapshot while a fresh scan runs in background. Not Zustand
 * because the value is a read-through cache of authoritative Rust
 * truth, not a continuity UI store.
 */
let cachedDevice: ConnectedDeviceDto | null = null;

export function invalidateConnectedLuniiCache(): void {
  cachedDevice = null;
}

/**
 * Authoritative read of the currently-connected supported Lunii.
 *
 * Encapsulated guardrails:
 * - hard timeout in the IPC façade so a hung Rust scan never freezes the UI
 * - cancel() on the IPC handle so the timer is cleared on unmount
 * - drift error → typed `DEVICE_SCAN_FAILED` AppError so the UI never
 *   has to reason about an arbitrary object
 * - StrictMode-safe active-call guard so a superseded response cannot
 *   overwrite a fresher state
 */
export function useConnectedLunii(): UseConnectedLunii {
  const [state, setState] = useState<ConnectedLuniiState>(() =>
    cachedDevice
      ? { kind: "ready", device: cachedDevice }
      : { kind: "loading" },
  );
  const [isRefreshing, setIsRefreshing] = useState<boolean>(() => true);
  const activeCallRef = useRef(0);
  const mountedRef = useRef(true);
  // Separate cancel handles for the visible and silent paths so a
  // silent poll never tears down a user-initiated refresh in flight,
  // and a fresh visible refresh tears down whatever stale work was
  // happening. Both are cleared on unmount to stop slow polls from
  // piling up across navigations.
  const visibleCancelRef = useRef<(() => void) | null>(null);
  const silentCancelRef = useRef<(() => void) | null>(null);
  // True iff a USER-VISIBLE call is in flight. While this is true,
  // silent polls skip entirely — the visible call is about to flip
  // the same state, and letting a silent poll resolve first would
  // leave `isRefreshing` stuck (the visible callback would see its
  // callId superseded and early-return without clearing the flag).
  const visibleInFlightRef = useRef(false);

  const load = useCallback((options?: { silent?: boolean }) => {
    const silent = options?.silent === true;

    // Silent polls defer to any visible call already in flight; the
    // visible result is more authoritative and resolves the same
    // state anyway.
    if (silent && visibleInFlightRef.current) return;

    const callId = ++activeCallRef.current;
    if (!silent) {
      if (cachedDevice) {
        setState({ kind: "ready", device: cachedDevice });
      } else {
        setState({ kind: "loading" });
      }
      setIsRefreshing(true);
      visibleInFlightRef.current = true;
      // A visible refresh supersedes both prior silent and visible
      // work — cancel both so their timer guards stop ticking.
      if (silentCancelRef.current) {
        silentCancelRef.current();
        silentCancelRef.current = null;
      }
      if (visibleCancelRef.current) {
        visibleCancelRef.current();
        visibleCancelRef.current = null;
      }
    } else {
      // A new silent poll supersedes the previous silent one only —
      // never the visible work.
      if (silentCancelRef.current) {
        silentCancelRef.current();
        silentCancelRef.current = null;
      }
    }

    const handle = readConnectedLunii();
    if (silent) {
      silentCancelRef.current = handle.cancel;
    } else {
      visibleCancelRef.current = handle.cancel;
    }

    handle.promise
      .then((device) => {
        if (!mountedRef.current) return;
        const superseded = callId !== activeCallRef.current;
        if (superseded) {
          // A newer call has already taken over the flag, the cancel
          // ref and (where relevant) the cache: do NOT touch those
          // refs — clearing them here would clobber the in-flight
          // newer call's bookkeeping (StrictMode double-trigger,
          // visible-after-visible race). The newer call will release
          // the bookkeeping when it settles.
          return;
        }
        cachedDevice = device;
        setState({ kind: "ready", device });
        if (silent) {
          silentCancelRef.current = null;
        } else {
          setIsRefreshing(false);
          visibleInFlightRef.current = false;
          visibleCancelRef.current = null;
        }
      })
      .catch((err) => {
        if (!mountedRef.current) return;
        const superseded = callId !== activeCallRef.current;
        if (superseded) {
          // Same rationale as the `then` branch: leave bookkeeping
          // to the newer call. Silently dropping the error is the
          // right policy for a superseded call regardless of the
          // silent/visible discriminant.
          return;
        }
        if (silent) {
          // Silent polls swallow errors entirely — we keep the last
          // known state visible instead of flapping to an error
          // banner every 3 s on a transient hiccup. A persistent
          // failure will surface the next time the user clicks
          // `Réessayer`.
          silentCancelRef.current = null;
          return;
        }
        if (err instanceof ReadConnectedLuniiContractDriftError) {
          setState({ kind: "error", error: DRIFT_ERROR });
        } else {
          setState({ kind: "error", error: toAppError(err) });
        }
        setIsRefreshing(false);
        visibleInFlightRef.current = false;
        visibleCancelRef.current = null;
      });
  }, []);

  const refresh = useCallback(() => {
    load();
  }, [load]);

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
      if (visibleCancelRef.current) {
        visibleCancelRef.current();
        visibleCancelRef.current = null;
      }
      if (silentCancelRef.current) {
        silentCancelRef.current();
        silentCancelRef.current = null;
      }
      visibleInFlightRef.current = false;
    };
  }, [load]);

  // Silent background polling: scans the device tree every
  // CONNECTED_LUNII_POLL_INTERVAL_MS and updates state ONLY if the
  // result changed. Gives the user automatic hotplug detection
  // without ever showing a `Détection en cours…` flash.
  useEffect(() => {
    const id = setInterval(() => {
      load({ silent: true });
    }, CONNECTED_LUNII_POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [load]);

  return {
    state,
    cached: state.kind === "ready" ? state.device : cachedDevice,
    isRefreshing,
    refresh,
  };
}
