import { useCallback, useEffect, useRef, useState } from "react";

import { getLibraryOverview } from "../../../ipc/commands/library";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import {
  isLibraryOverviewDto,
  type LibraryOverviewDto,
} from "../../../shared/ipc-contracts/library";

export type LibraryOverviewState =
  | { kind: "loading" }
  | { kind: "ready"; overview: LibraryOverviewDto }
  | { kind: "error"; error: AppError };

const MALFORMED_OVERVIEW_ERROR: AppError = {
  code: "LIBRARY_INCONSISTENT",
  message: "Rustory a détecté une bibliothèque incohérente.",
  userAction: "Relance Rustory pour reconstruire la vue cohérente.",
  details: null,
};

export interface UseLibraryOverview {
  state: LibraryOverviewState;
  /** Stale-while-revalidate snapshot: the last known `ready` overview, even
   *  when {@link state} is currently `loading` during a background refresh. */
  cached: LibraryOverviewDto | null;
  /** True while a background refresh is in flight on top of a cached view —
   *  distinct from {@link LibraryOverviewState.kind} `"loading"` which means
   *  no cached data is available yet. */
  isRefreshing: boolean;
  /** Re-run the IPC call, superseding any in-flight response. Used by the
   *  error banner's Réessayer and by routes that want an explicit refresh. */
  retry: () => void;
  /** Drop the module-local cache. Call when a downstream consumer discovers
   *  the stored overview is stale (e.g. after a mutation) — the next hook
   *  consumer will fetch fresh. */
  invalidate: () => void;
}

/**
 * Module-local cache, shared across hook instances. Keeps the library
 * overview in memory so navigating `/library → /story/:id/edit → /library`
 * does not flash the loading state: the stale snapshot renders immediately
 * while a background refresh runs.
 *
 * The cache is hook-local by design (not Zustand): it is a read-through
 * cache of authoritative Rust state, not a continuity UI store. The
 * architecture's forbidden-store-content rule keeps such caches out of the
 * shell store.
 */
let cachedOverview: LibraryOverviewDto | null = null;

export function invalidateLibraryOverviewCache(): void {
  cachedOverview = null;
}

/**
 * Shared authoritative read of the library overview with stale-while-
 * revalidate semantics.
 *
 * Encapsulated guardrails:
 * - a hard timeout so a hung Rust side never freezes the UI,
 * - a cancel() on the IPC handle so the timer is cleared on unmount and a
 *   route switch cannot stack two live timers,
 * - a runtime DTO guard so a drifted wire shape never reaches the UI,
 * - a normalization of non-AppError rejections to a stable `UNKNOWN` code,
 * - a StrictMode-safe active-call guard so a superseded response cannot
 *   overwrite a fresher state.
 */
export function useLibraryOverview(): UseLibraryOverview {
  const [state, setState] = useState<LibraryOverviewState>(() =>
    cachedOverview
      ? { kind: "ready", overview: cachedOverview }
      : { kind: "loading" },
  );
  const [isRefreshing, setIsRefreshing] = useState<boolean>(() => true);
  const activeCallRef = useRef(0);
  const mountedRef = useRef(true);
  const cancelRef = useRef<(() => void) | null>(null);

  const load = useCallback(() => {
    const callId = ++activeCallRef.current;
    // If we already have a cached snapshot, keep rendering it and only flip
    // `isRefreshing`. Otherwise we really have nothing to show: fall back
    // to the loading state.
    if (cachedOverview) {
      setState({ kind: "ready", overview: cachedOverview });
    } else {
      setState({ kind: "loading" });
    }
    setIsRefreshing(true);

    // Cancel any in-flight IPC handle so only the latest call's timer lives.
    if (cancelRef.current) {
      cancelRef.current();
      cancelRef.current = null;
    }
    const handle = getLibraryOverview();
    cancelRef.current = handle.cancel;

    handle.promise
      .then((overview) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        if (!isLibraryOverviewDto(overview)) {
          setState({ kind: "error", error: MALFORMED_OVERVIEW_ERROR });
          setIsRefreshing(false);
          return;
        }
        cachedOverview = overview;
        setState({ kind: "ready", overview });
        setIsRefreshing(false);
      })
      .catch((err) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        setState({ kind: "error", error: toAppError(err) });
        setIsRefreshing(false);
      });
  }, []);

  const invalidate = useCallback(() => {
    invalidateLibraryOverviewCache();
    load();
  }, [load]);

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
      if (cancelRef.current) {
        cancelRef.current();
        cancelRef.current = null;
      }
    };
  }, [load]);

  return {
    state,
    cached: state.kind === "ready" ? state.overview : cachedOverview,
    isRefreshing,
    retry: load,
    invalidate,
  };
}
