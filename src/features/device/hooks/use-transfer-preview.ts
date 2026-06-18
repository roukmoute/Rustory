import { useCallback, useEffect, useRef, useState } from "react";

import {
  readTransferPreview,
  ReadTransferPreviewContractDriftError,
} from "../../../ipc/commands/transfer-preview";
import { toAppError, type AppError } from "../../../shared/errors/app-error";

export type TransferPreviewState =
  | { kind: "idle" }
  | { kind: "loading" }
  | {
      kind: "ready";
      onDevice: boolean;
      unchangedCount: number;
      storyTitle: string;
      transferable: boolean;
    }
  | { kind: "error"; error: AppError };

const DRIFT_ERROR: AppError = {
  code: "DEVICE_SCAN_FAILED",
  message: "Comparaison avant envoi indisponible: réponse invalide.",
  userAction:
    "Réessaie la comparaison. Si le problème persiste, signale-le avec les traces locales.",
  details: null,
};

// Surfaced when a comparison was requested for a readable device but the
// authoritative re-read no longer resolves to it (unplugged / swapped /
// payload identifiers don't match the request). This is NOT "no device":
// a device WAS detected, so the recoverable "ça a changé, réessaie" wording
// (with the retry CTA) is honest, where a "plug a Lunii" hint would mislead.
const DEVICE_CHANGED_ERROR: AppError = {
  code: "DEVICE_SCAN_FAILED",
  message: "L'appareil a changé pendant la comparaison.",
  userAction:
    "Vérifie que la Lunii est toujours branchée puis réessaie la comparaison.",
  details: null,
};

export interface UseTransferPreview {
  state: TransferPreviewState;
  /** User-triggered re-read. No-op when there is no comparable pair. */
  refresh: () => void;
}

/**
 * Compose the read-only pre-transfer comparison for the selected local
 * `storyId` against the readable device `deviceIdentifier`. Pass `null` for
 * either when there is no comparable pair (no single local selection, or no
 * readable device) — the hook then sits in `idle` and issues no IPC.
 *
 * Guardrails:
 * - the comparison is composed in Rust; the hook only PRESENTS it.
 * - **Always reads fresh.** This is a pre-write DECISION surface: the
 *   architecture mandates an authoritative re-read here, so the hook never
 *   renders a cached (potentially stale) add/replace verdict — every
 *   (re)mount, pair change, and `refresh()` goes through `loading` → fresh.
 * - re-reads when the `(storyId, deviceIdentifier)` pair changes; clears to
 *   `idle` when either goes `null`.
 * - the `ready` payload's identifiers are checked against the active request
 *   before display: a misrouted / superseded response surfaces as a
 *   recoverable "device changed" rather than a verdict for the wrong target.
 * - a `noDevice` / `unsupported` response (the requested readable device is no
 *   longer there) surfaces a recoverable "device changed" error with a retry —
 *   never an `idle` that the route would mislabel "plug a Lunii", and never a
 *   touch on the local library. `idle` is reserved for "no comparable pair".
 * - StrictMode-safe active-call guard + cancel() on unmount.
 */
export function useTransferPreview(
  storyId: string | null,
  deviceIdentifier: string | null,
): UseTransferPreview {
  const [state, setState] = useState<TransferPreviewState>(() =>
    storyId && deviceIdentifier ? { kind: "loading" } : { kind: "idle" },
  );
  const activeCallRef = useRef(0);
  const mountedRef = useRef(true);
  const cancelRef = useRef<(() => void) | null>(null);

  const load = useCallback((sid: string, did: string) => {
    const callId = ++activeCallRef.current;
    setState({ kind: "loading" });
    if (cancelRef.current) {
      cancelRef.current();
      cancelRef.current = null;
    }

    const handle = readTransferPreview({ storyId: sid, deviceIdentifier: did });
    cancelRef.current = handle.cancel;

    handle.promise
      .then((dto) => {
        if (!mountedRef.current) return;
        if (callId !== activeCallRef.current) return;
        cancelRef.current = null;
        if (dto.kind === "ready") {
          // Defense-in-depth: only display a verdict that belongs to the
          // request we made. A payload whose identifiers do not match the
          // active `(sid, did)` is a misroute/race for a device that is no
          // longer the one we asked about — surface it as a recoverable
          // "device changed" rather than paint a verdict for the wrong target.
          if (dto.deviceIdentifier !== did || dto.story.id !== sid) {
            setState({ kind: "error", error: DEVICE_CHANGED_ERROR });
            return;
          }
          setState({
            kind: "ready",
            onDevice: dto.onDevice,
            unchangedCount: dto.unchangedCount,
            storyTitle: dto.story.title,
            transferable: dto.transferable,
          });
        } else {
          // `noDevice` / `unsupported`: a device WAS detected (the route only
          // calls us with a readable device id), but the authoritative re-scan
          // no longer resolves to it — it was unplugged/swapped between
          // detection and this read. Surface a recoverable "device changed"
          // (with retry), never the misleading "plug a Lunii" hint. The LOCAL
          // library stays untouched.
          setState({ kind: "error", error: DEVICE_CHANGED_ERROR });
        }
      })
      .catch((err) => {
        if (!mountedRef.current) return;
        if (callId !== activeCallRef.current) return;
        cancelRef.current = null;
        // A read failure (device changed mid-read, FS error, timeout, local
        // store unavailable) is recoverable and shown IN CONTEXT — never a
        // toast, never a touch on the local library.
        if (err instanceof ReadTransferPreviewContractDriftError) {
          setState({ kind: "error", error: DRIFT_ERROR });
        } else {
          setState({ kind: "error", error: toAppError(err) });
        }
      });
  }, []);

  const refresh = useCallback(() => {
    if (storyId && deviceIdentifier) load(storyId, deviceIdentifier);
  }, [storyId, deviceIdentifier, load]);

  useEffect(() => {
    mountedRef.current = true;
    if (storyId && deviceIdentifier) {
      load(storyId, deviceIdentifier);
    } else {
      // No comparable pair: supersede any in-flight read and reset to idle so a
      // late resolution cannot paint a stale comparison.
      activeCallRef.current += 1;
      if (cancelRef.current) {
        cancelRef.current();
        cancelRef.current = null;
      }
      setState({ kind: "idle" });
    }
    return () => {
      mountedRef.current = false;
      if (cancelRef.current) {
        cancelRef.current();
        cancelRef.current = null;
      }
    };
  }, [storyId, deviceIdentifier, load]);

  return { state, refresh };
}
