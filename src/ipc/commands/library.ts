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
 * Read the current library overview from the Rust core.
 *
 * Resolves with the typed DTO once the managed local storage is reachable,
 * rejects with a normalized `AppError` when storage initialization fails, and
 * rejects with a synthetic `UNKNOWN`-coded error if the Rust side does not
 * answer within {@link LIBRARY_OVERVIEW_TIMEOUT_MS}.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so the
 * wire contract stays owned by `src/ipc/`.
 */
export async function getLibraryOverview(
  timeoutMs: number = LIBRARY_OVERVIEW_TIMEOUT_MS,
): Promise<LibraryOverviewDto> {
  const call = invoke<LibraryOverviewDto>("get_library_overview");

  let timer: ReturnType<typeof setTimeout> | undefined;
  const guard = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(LIBRARY_OVERVIEW_TIMEOUT_ERROR), timeoutMs);
  });

  try {
    return await Promise.race([call, guard]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
