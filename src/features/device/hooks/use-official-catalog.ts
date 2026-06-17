import { useCallback, useEffect, useRef, useState } from "react";

import {
  getOfficialCatalogStatus,
  importOfficialCatalog,
  refreshOfficialCatalog,
} from "../../../ipc/commands/device-catalog";
import { toAppError, type AppError } from "../../../shared/errors/app-error";

export type OfficialCatalogState =
  | { kind: "loading" }
  | { kind: "ready"; count: number }
  | { kind: "error"; error: AppError };

/** Which long-running action (if any) is in flight. */
export type OfficialCatalogAction = "idle" | "refreshing" | "importing";

export interface UseOfficialCatalog {
  /** The cached-count status (loaded on mount). */
  state: OfficialCatalogState;
  /** The in-flight action, or `"idle"`. */
  action: OfficialCatalogAction;
  /** The last action failure, or `null`. Surfaced in-context, never a toast. */
  actionError: AppError | null;
  /** EXPLICIT network refresh of the official catalog. */
  refresh(): Promise<void>;
  /** Import the catalog from a user-picked file (offline path). */
  importFile(): Promise<void>;
  /** Dismiss the current action error back to none. */
  dismissError(): void;
}

export interface UseOfficialCatalogOptions {
  /** Called after the cache actually changes (a successful refresh, or a
   *  file import that committed). The route uses it to re-read the displayed
   *  device inventory so newly recognized titles appear immediately. */
  onChanged?: () => void;
}

/**
 * Owns the official-catalog status + the two explicit catalog actions
 * (network refresh, file import). Offline-first: NOTHING here runs without a
 * deliberate user action except the initial bounded count read on mount.
 * StrictMode-safe mount flag + a synchronous re-entrancy guard so a
 * double-click never fires two fetches.
 */
export function useOfficialCatalog(
  options?: UseOfficialCatalogOptions,
): UseOfficialCatalog {
  const [state, setState] = useState<OfficialCatalogState>({ kind: "loading" });
  const [action, setAction] = useState<OfficialCatalogAction>("idle");
  const [actionError, setActionError] = useState<AppError | null>(null);

  const mountedRef = useRef(true);
  const inFlightRef = useRef(false);
  const onChangedRef = useRef<UseOfficialCatalogOptions["onChanged"]>(
    options?.onChanged,
  );
  onChangedRef.current = options?.onChanged;

  const loadStatus = useCallback(() => {
    getOfficialCatalogStatus()
      .then((status) => {
        if (!mountedRef.current) return;
        setState({ kind: "ready", count: status.count });
      })
      .catch((err: unknown) => {
        if (!mountedRef.current) return;
        setState({ kind: "error", error: toAppError(err) });
      });
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    loadStatus();
    return () => {
      mountedRef.current = false;
    };
  }, [loadStatus]);

  const refresh = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    if (mountedRef.current) {
      setAction("refreshing");
      setActionError(null);
    }
    try {
      const status = await refreshOfficialCatalog();
      if (mountedRef.current) setState({ kind: "ready", count: status.count });
      // The cache changed → let the route re-read the device inventory so
      // freshly recognized titles surface without a manual refresh. Runs
      // outside the catch and guarded by mount so a throwing/late callback
      // never reclassifies the success.
      if (mountedRef.current) {
        try {
          onChangedRef.current?.();
        } catch {
          // Deliberately swallowed.
        }
      }
    } catch (err) {
      if (mountedRef.current) setActionError(toAppError(err));
    } finally {
      inFlightRef.current = false;
      if (mountedRef.current) setAction("idle");
    }
  }, []);

  const importFile = useCallback(async (): Promise<void> => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    if (mountedRef.current) {
      setAction("importing");
      setActionError(null);
    }
    try {
      const outcome = await importOfficialCatalog();
      // A cancelled dialog is a no-op — leave the count untouched.
      if (mountedRef.current && outcome.kind === "imported") {
        setState({ kind: "ready", count: outcome.count });
        try {
          onChangedRef.current?.();
        } catch {
          // Deliberately swallowed.
        }
      }
    } catch (err) {
      if (mountedRef.current) setActionError(toAppError(err));
    } finally {
      inFlightRef.current = false;
      if (mountedRef.current) setAction("idle");
    }
  }, []);

  const dismissError = useCallback((): void => {
    setActionError(null);
  }, []);

  return { state, action, actionError, refresh, importFile, dismissError };
}
