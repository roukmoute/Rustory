import { useCallback, useEffect, useRef, useState } from "react";

import {
  applyRecovery,
  ApplyRecoveryContractDriftError,
  discardDraft,
  readRecoverableDraft,
  ReadRecoverableDraftContractDriftError,
} from "../../../ipc/commands/story";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type {
  RecoverableDraft,
  UpdateStoryOutput,
} from "../../../shared/ipc-contracts/story";

/**
 * Map any caught error from a recovery-flow IPC façade to a typed
 * `AppError`. A drift error from the runtime guards becomes a
 * `RECOVERY_DRAFT_UNAVAILABLE` payload (with `details.source = "contract_drift"`)
 * rather than the generic `UNKNOWN` code that `toAppError` would
 * fall back to. Keeping the recovery code on these failures preserves
 * the NFR24 stable identifier the UI/log consumer expects on this
 * surface and lets the support log triage the cause without parsing
 * the user-facing message.
 */
function toRecoveryAppError(err: unknown): AppError {
  if (
    err instanceof ApplyRecoveryContractDriftError ||
    err instanceof ReadRecoverableDraftContractDriftError
  ) {
    return {
      code: "RECOVERY_DRAFT_UNAVAILABLE",
      message: "Récupération indisponible: vérifie le disque local et réessaie.",
      userAction:
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
      details: {
        source: "contract_drift",
        kind: err.name,
      },
    };
  }
  return toAppError(err);
}

/**
 * Recoverable draft refined to its `recoverable` branch — the only
 * variant that produces a visible banner in the route.
 */
type RecoveredDraftPayload = Extract<RecoverableDraft, { kind: "recoverable" }>;

export type RecoveryState =
  | { kind: "loading" }
  | { kind: "none" }
  | { kind: "recoverable"; draft: RecoveredDraftPayload }
  | {
      kind: "applying";
      draft: RecoveredDraftPayload;
      /** Which user intent is currently in flight. The banner uses
       *  this to choose its progress copy: a "Restauration en cours…"
       *  glyph for `apply` is wrong for a `discard`, where the user
       *  is dropping the draft, not restoring it. */
      intent: "apply" | "discard";
    }
  | {
      kind: "error";
      error: AppError;
      /** When an error happens AFTER a recoverable draft was loaded
       *  (apply/discard failure), keep the draft visible so the user
       *  can retry or discard. `null` for failures during the initial
       *  read — there is no draft to fall back to. */
      draft: RecoveredDraftPayload | null;
    };

export interface UseStoryRecoveryOptions {
  /** Called after a successful `applyRecovery`. The route uses this
   *  to reconcile its in-memory `useStoryEditor` detail without a
   *  follow-up `getStoryDetail` round-trip. */
  onApplied?: (output: UpdateStoryOutput) => void;
  /** Called after a successful `discardDraft`. Optional — most callers
   *  do not need to react to a discard beyond hiding the banner. */
  onDiscarded?: (storyId: string) => void;
}

export interface UseStoryRecovery {
  state: RecoveryState;
  /** Apply the buffered draft. No-op when not in `recoverable` or
   *  `error` (with draft) state, or while another apply is in flight. */
  apply: () => void;
  /** Drop the buffered draft. Idempotent — a second click while
   *  `applying` is a silent no-op. */
  discard: () => void;
  /** Re-run the initial `readRecoverableDraft` after an error. */
  retry: () => void;
  /** Dismiss an initial-read error without retrying — the user gives
   *  up on recovery and resumes editing from the persisted state. The
   *  hook transitions to `kind: "none"` so the Field re-enables. No-op
   *  when not in the `error + draft=null` shape. */
  dismissReadError: () => void;
}

/**
 * Authoritative recoverable-draft hook for a single story.
 *
 * Reads the buffered keystroke value at mount and on every `storyId`
 * change. Exposes `apply` / `discard` / `retry` actions with synchronous
 * re-entrancy guards (one in-flight write at a time).
 *
 * Apply path: a successful `applyRecovery` resolves with the
 * `UpdateStoryOutput` of the now-persisted title. The `onApplied`
 * callback is invoked synchronously after the state transitions to
 * `none` so the caller can patch its own in-memory snapshot.
 */
export function useStoryRecovery(
  storyId: string | undefined,
  options: UseStoryRecoveryOptions = {},
): UseStoryRecovery {
  const [state, setState] = useState<RecoveryState>(() =>
    storyId ? { kind: "loading" } : { kind: "none" },
  );

  const stateRef = useRef<RecoveryState>(state);
  stateRef.current = state;

  // Same StrictMode + storyId-change discipline as `useLibraryOverview`
  // and `useStoryEditor`: bump on every new IPC call, ignore stale
  // resolutions.
  const activeCallRef = useRef(0);
  const mountedRef = useRef(true);
  // Synchronous in-flight flag for apply/discard re-entrancy (the
  // discriminated `applying` state only flips at next render).
  const writeInFlightRef = useRef(false);

  // Stash the option callbacks behind refs so the load callback below
  // does not have to capture them and re-fire on every parent render.
  const onAppliedRef = useRef<typeof options.onApplied>(undefined);
  onAppliedRef.current = options.onApplied;
  const onDiscardedRef = useRef<typeof options.onDiscarded>(undefined);
  onDiscardedRef.current = options.onDiscarded;

  const load = useCallback(() => {
    if (!storyId) {
      // Bump the active-call counter so any in-flight read for a
      // previous storyId becomes a no-op when its Promise settles.
      // Without this, a fast unmount-then-set-undefined sequence can
      // let a stale recoverable payload land on top of the `none`
      // state we are about to commit.
      activeCallRef.current += 1;
      setState({ kind: "none" });
      return;
    }
    const callId = ++activeCallRef.current;
    setState({ kind: "loading" });

    readRecoverableDraft({ storyId })
      .then((result) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        if (result.kind === "none") {
          setState({ kind: "none" });
          return;
        }
        setState({ kind: "recoverable", draft: result });
      })
      .catch((err: unknown) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        // Initial read failed — no draft to fall back to. The banner
        // surfaces a `role="alert"` with retry.
        setState({ kind: "error", error: toRecoveryAppError(err), draft: null });
      });
  }, [storyId]);

  const apply = useCallback(() => {
    if (writeInFlightRef.current) return;
    const current = stateRef.current;
    const draft =
      current.kind === "recoverable"
        ? current.draft
        : current.kind === "error"
          ? current.draft
          : null;
    if (!draft) return;

    writeInFlightRef.current = true;
    const callId = ++activeCallRef.current;
    // P20: transition through the `applying` state, which clears the
    // previous `error` payload. Without this, the bannière would
    // render `aria-busy="true"` AND keep the old `role="alert"`
    // visible — confusing for AT users (the alert is stale, the
    // operation is fresh) and visually inconsistent for everyone.
    setState({ kind: "applying", draft, intent: "apply" });

    // Wrap the IPC call in a synchronous try/catch so a drift error
    // thrown BEFORE the Promise is returned (e.g. wire-shape mismatch
    // during arg encoding) cannot leave `writeInFlightRef = true`. The
    // `.finally` guarantees the flag flips back even when the caught
    // value reaches the .catch path. Without this, a single bad apply
    // bricks every subsequent click in the session.
    let promise: Promise<UpdateStoryOutput>;
    try {
      promise = applyRecovery({ storyId: draft.storyId });
    } catch (err: unknown) {
      writeInFlightRef.current = false;
      if (mountedRef.current && callId === activeCallRef.current) {
        setState({
          kind: "error",
          error: toRecoveryAppError(err),
          draft,
        });
      }
      return;
    }

    promise
      .then((output) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        setState({ kind: "none" });
        onAppliedRef.current?.(output);
      })
      .catch((err: unknown) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        // Keep the draft visible so the user can offer Discard or
        // retry — losing it on an apply failure would be data loss.
        setState({
          kind: "error",
          error: toRecoveryAppError(err),
          draft,
        });
      })
      .finally(() => {
        if (callId === activeCallRef.current) {
          writeInFlightRef.current = false;
        }
      });
  }, []);

  const discard = useCallback(() => {
    if (writeInFlightRef.current) return;
    const current = stateRef.current;
    const draft =
      current.kind === "recoverable"
        ? current.draft
        : current.kind === "error"
          ? current.draft
          : null;
    if (!draft) return;

    writeInFlightRef.current = true;
    const callId = ++activeCallRef.current;
    // Optimistic transition: the discard is idempotent, so showing the
    // banner gone immediately is safe. On failure we restore via the
    // catch branch with the original draft kept aside.
    const previousDraft = draft;
    setState({ kind: "applying", draft, intent: "discard" });

    let promise: Promise<void>;
    try {
      // Forward the observed `draftAt` so the Rust core can
      // compare-and-swap: a concurrent `record_draft` that refreshed
      // the row between the user's observation and this click is
      // preserved instead of silently dropped. P18.
      promise = discardDraft({
        storyId: draft.storyId,
        expectedDraftAt: draft.draftAt,
      });
    } catch (err: unknown) {
      writeInFlightRef.current = false;
      if (mountedRef.current && callId === activeCallRef.current) {
        setState({
          kind: "error",
          error: toRecoveryAppError(err),
          draft: previousDraft,
        });
      }
      return;
    }

    promise
      .then(() => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        setState({ kind: "none" });
        onDiscardedRef.current?.(draft.storyId);
      })
      .catch((err: unknown) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        setState({
          kind: "error",
          error: toRecoveryAppError(err),
          draft: previousDraft,
        });
      })
      .finally(() => {
        if (callId === activeCallRef.current) {
          writeInFlightRef.current = false;
        }
      });
  }, []);

  const retry = useCallback(() => {
    load();
  }, [load]);

  const dismissReadError = useCallback(() => {
    // Guard against a retry that is still in flight: dismissing while
    // `writeInFlightRef.current === true` would race with the apply
    // / discard / load resolution. The write-flight ref is only set
    // for apply/discard, but a retry triggers a fresh `load()` that
    // mounts a new active-call id we should NOT trample on either.
    // We inspect whether the latest call id has resolved by comparing
    // the current state.kind: a `loading` state means a retry is in
    // flight, dismiss must wait for that retry to settle.
    if (writeInFlightRef.current) return;
    const current = stateRef.current;
    // Only meaningful when the initial read failed (no draft attached).
    // An apply/discard error keeps the draft and uses retry instead.
    if (current.kind === "loading") return;
    if (current.kind !== "error" || current.draft !== null) return;
    activeCallRef.current += 1;
    setState({ kind: "none" });
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
    };
  }, [load]);

  return {
    state,
    apply,
    discard,
    retry,
    dismissReadError,
  };
}
