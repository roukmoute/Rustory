import { invoke } from "@tauri-apps/api/core";

import type { LibraryOverviewDto } from "../../shared/ipc-contracts/library";

/** Upper bound for a boot-time read. Picked well under NFR1 (`3s` p95 cold
 *  start to a usable library) so a hung Rust side never freezes the UI. */
export const LIBRARY_OVERVIEW_TIMEOUT_MS = 2000;

/** Discriminant emitted when {@link getLibraryOverview} trips its timeout. */
export const LIBRARY_OVERVIEW_TIMEOUT_ERROR = {
  code: "UNKNOWN",
  message: "Rustory a mis trop de temps à charger la bibliothèque.",
  userAction:
    "Relance l'application. Si le problème persiste, signale-le avec les traces locales.",
  details: null,
} as const;

/**
 * Cancelable handle returned by {@link getLibraryOverview}. Callers that
 * unmount before the IPC settles MUST call `cancel()` so the timer guard is
 * cleared — otherwise a long-lived timer would fire after unmount and
 * accumulate timers across route switches.
 */
export interface LibraryOverviewCall {
  promise: Promise<LibraryOverviewDto>;
  cancel: () => void;
}

/**
 * Read the current library overview from the Rust core.
 *
 * Resolves with the typed DTO once the managed local storage is reachable,
 * rejects with a normalized `AppError` when storage initialization fails, and
 * rejects with a synthetic `UNKNOWN`-coded error if the Rust side does not
 * answer within {@link LIBRARY_OVERVIEW_TIMEOUT_MS}.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so the
 * wire contract stays owned by `src/ipc/`.
 *
 * Returns a handle with a `cancel()` method so consumers can tear down the
 * timer guard on unmount. Cancelling after resolution is a no-op.
 */
export function getLibraryOverview(
  timeoutMs: number = LIBRARY_OVERVIEW_TIMEOUT_MS,
): LibraryOverviewCall {
  const call = invoke<LibraryOverviewDto>("get_library_overview");

  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;

  const guard = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      timer = undefined;
      if (cancelled) return;
      reject(LIBRARY_OVERVIEW_TIMEOUT_ERROR);
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
