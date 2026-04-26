import { useCallback, useEffect, useRef, useState } from "react";

import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { normalizeStoryTitle } from "../../library/validation/story-title";
import {
  discardDraft,
  getStoryDetail,
  recordDraft,
  saveStory,
} from "../../../ipc/commands/story";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type {
  StoryDetailDto,
  UpdateStoryOutput,
} from "../../../shared/ipc-contracts/story";
import { isStoryDetailDto } from "../../../shared/ipc-contracts/story";

/** Autosave debounce window after the last keystroke, in milliseconds. */
export const STORY_AUTOSAVE_DEBOUNCE_MS = 500;
/** Recovery-draft buffer debounce. Shorter than the autosave so a crash
 *  between two keystrokes still preserves the typed value: if the user
 *  pauses ≥ 150 ms the draft has already been written to `story_drafts`
 *  via `record_draft`, even though the autosave UPDATE has not fired yet. */
export const STORY_DRAFT_RECORD_DEBOUNCE_MS = 150;
/** How long the "Enregistré" chip stays visible before settling back to the
 *  quiescent "Brouillon local" label. Short enough to avoid stale reassurance
 *  while long enough for a glance to register it. */
export const STORY_AUTOSAVE_SAVED_VISIBLE_MS = 3000;

const MALFORMED_DETAIL_ERROR: AppError = {
  code: "LIBRARY_INCONSISTENT",
  message: "Rustory a détecté une bibliothèque incohérente.",
  userAction: "Relance Rustory pour reconstruire la vue cohérente.",
  details: null,
};

export type SaveStatus =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "saving" }
  | { kind: "saved"; at: string }
  | { kind: "failed"; error: AppError; attemptedTitle: string };

export type StoryEditorState =
  | { kind: "loading" }
  | { kind: "not-found" }
  | { kind: "error"; error: AppError }
  | {
      kind: "ready";
      detail: StoryDetailDto;
      draftTitle: string;
      saveStatus: SaveStatus;
    };

export interface UseStoryEditor {
  state: StoryEditorState;
  /** Update the live field value. Plans a debounced autosave, clears a
   *  stale failure alert, or settles back to idle if the new value matches
   *  the persisted title after normalization. */
  setDraftTitle: (next: string) => void;
  /** Re-run the initial detail read after an error state. */
  retry: () => void;
  /** Re-fire the autosave from a failed state using the current draft. */
  retrySave: () => void;
  /** Cancel the debounce and commit the pending save immediately. Called
   *  when the user clicks "Retour à la bibliothèque" or the route unmounts. */
  flushAutoSave: () => void;
  /** Patch the in-memory `detail` from a successful `applyRecovery`
   *  output without re-fetching. Aligns `draftTitle` with the new
   *  persisted value and resets the save status to `idle`. */
  reloadDetailFromOutput: (output: UpdateStoryOutput) => void;
}

interface UseStoryEditorOptions {
  debounceMs?: number;
  savedVisibleMs?: number;
  /** Override the recovery-draft buffer debounce, in milliseconds. */
  recordDraftDebounceMs?: number;
}

/**
 * Authoritative editor hook for a single story.
 *
 * - Reads the detail from Rust on mount and on every `storyId` change.
 *   The route must never assume cached overview data is fresh enough.
 * - Debounces autosave so keystrokes do not flood the IPC boundary.
 * - Preserves the persisted `detail` on save failure, so the UI never
 *   paints "Enregistré" over an unsaved change. Acceptance Criterion 3.
 * - Invalidates the library overview cache after every successful save
 *   so returning to `/library` shows the new title.
 */
export function useStoryEditor(
  storyId: string | undefined,
  options: UseStoryEditorOptions = {},
): UseStoryEditor {
  const debounceMs = options.debounceMs ?? STORY_AUTOSAVE_DEBOUNCE_MS;
  const savedVisibleMs =
    options.savedVisibleMs ?? STORY_AUTOSAVE_SAVED_VISIBLE_MS;
  const recordDraftDebounceMs =
    options.recordDraftDebounceMs ?? STORY_DRAFT_RECORD_DEBOUNCE_MS;

  const [state, setState] = useState<StoryEditorState>(() =>
    storyId ? { kind: "loading" } : { kind: "not-found" },
  );

  // State refs — mirrors of the latest render so timer callbacks can read
  // the freshest value without capturing stale closures.
  const stateRef = useRef<StoryEditorState>(state);
  stateRef.current = state;

  // Call-correlation guards, same discipline as `useLibraryOverview`:
  // `activeCallRef` invalidates superseded IPC responses (StrictMode double
  // mount, storyId change, retry), `mountedRef` catches the unmount window.
  const activeCallRef = useRef(0);
  const mountedRef = useRef(true);

  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const savedIdleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Recovery-draft buffer timer. Independent from the autosave debounce
  // so a fast pause writes the buffer well before the autosave would
  // even start preparing its UPDATE.
  const recordDraftTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  // Tracks whether a `record_draft` was scheduled OR flushed for the
  // currently-loaded detail. Used to decide whether to fire an
  // auto-discard when the user types back to the persisted value: a
  // residual buffered keystroke would otherwise survive the session.
  // Cleared when the autosave succeeds (the DELETE in the same
  // transaction already consumed the row) or when an explicit
  // discard succeeds.
  const hasPendingDraftRef = useRef(false);
  // Synchronous in-flight flag. `stateRef.current.saveStatus` only updates
  // at the next React render, so a second synchronous call to `retrySave`
  // or `flushAutoSave` would still see the old `failed` / `pending` value
  // and issue a duplicate `saveStory` before the first has even been
  // queued. This ref is flipped synchronously inside `fireSave`.
  const saveInFlightRef = useRef(false);

  const clearDebounce = useCallback(() => {
    if (debounceTimerRef.current !== null) {
      clearTimeout(debounceTimerRef.current);
      debounceTimerRef.current = null;
    }
  }, []);

  const clearSavedIdle = useCallback(() => {
    if (savedIdleTimerRef.current !== null) {
      clearTimeout(savedIdleTimerRef.current);
      savedIdleTimerRef.current = null;
    }
  }, []);

  const clearRecordDraft = useCallback(() => {
    if (recordDraftTimerRef.current !== null) {
      clearTimeout(recordDraftTimerRef.current);
      recordDraftTimerRef.current = null;
    }
  }, []);

  const scheduleRecordDraft = useCallback(
    (storyId: string, draftTitle: string) => {
      clearRecordDraft();
      // Mark the buffer as "pending or flushed" the moment we schedule:
      // a back-to-persisted setDraftTitle that lands during the 150 ms
      // debounce must still trigger an auto-discard, because the row
      // may have been written by a previous burst within the same
      // session.
      hasPendingDraftRef.current = true;
      recordDraftTimerRef.current = setTimeout(() => {
        recordDraftTimerRef.current = null;
        // Best-effort: the autosave is the durable mechanism, so a
        // record_draft failure must NOT affect the user-visible save
        // state. The Rust side already logs the failure via the
        // recovery_log diagnostic stream.
        void recordDraft({ storyId, draftTitle }).catch(() => undefined);
      }, recordDraftDebounceMs);
    },
    [clearRecordDraft, recordDraftDebounceMs],
  );

  const load = useCallback(() => {
    if (!storyId) {
      setState({ kind: "not-found" });
      return;
    }
    const callId = ++activeCallRef.current;
    clearDebounce();
    clearSavedIdle();
    // Fresh load: any prior buffer claim from a different storyId or a
    // remounted hook is not ours to track anymore.
    hasPendingDraftRef.current = false;
    setState({ kind: "loading" });

    getStoryDetail({ storyId })
      .then((detail) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        if (detail === null) {
          setState({ kind: "not-found" });
          return;
        }
        if (!isStoryDetailDto(detail)) {
          setState({ kind: "error", error: MALFORMED_DETAIL_ERROR });
          return;
        }
        setState({
          kind: "ready",
          detail,
          draftTitle: detail.title,
          saveStatus: { kind: "idle" },
        });
      })
      .catch((err: unknown) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        setState({ kind: "error", error: toAppError(err) });
      });
  }, [storyId, clearDebounce, clearSavedIdle]);

  // Forward reference — `scheduleDebouncedSave` closes over `fireSave`
  // via the `fireSaveRef` indirection below, so `fireSave` can schedule
  // a debounce when a stale success exposes a newer draft.
  const fireSaveRef = useRef<((normalizedTitle: string) => void) | null>(null);

  const scheduleDebouncedSave = useCallback(() => {
    clearDebounce();
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null;
      if (!mountedRef.current) return;
      const current = stateRef.current;
      if (current.kind !== "ready") return;
      const normalized = normalizeStoryTitle(current.draftTitle);
      if (normalized === current.detail.title) return;
      fireSaveRef.current?.(normalized);
    }, debounceMs);
  }, [clearDebounce, debounceMs]);

  // Fire a save. Centralized so the debounce, retry and flush paths all
  // share the same success/failure branching.
  //
  // Invalidation rule: the library cache invalidates on ANY terminal save
  // outcome (success OR failure) that touches the persisted state —
  // Rust is the source of truth and a fresh overview fetch is the only
  // way to reconcile after a boundary traversal. Invariant enforcers:
  // - `activeCallRef`/`callId` makes a superseded save (newer keystroke
  //   or a storyId switch) a no-op, so late success does NOT clobber a
  //   fresher draft;
  // - `prev.detail.title === normalizedTitle` check on success preserves
  //   a user who started typing again mid-save: we do NOT paint the chip
  //   `Enregistré` when `draftTitle` has already moved past the save
  //   we are ACK-ing.
  const fireSave = useCallback(
    (normalizedTitle: string) => {
      const current = stateRef.current;
      if (current.kind !== "ready") return;
      const detailId = current.detail.id;
      const callId = ++activeCallRef.current;
      saveInFlightRef.current = true;
      clearSavedIdle();

      setState((prev) =>
        prev.kind === "ready"
          ? { ...prev, saveStatus: { kind: "saving" } }
          : prev,
      );

      saveStory({ id: detailId, title: normalizedTitle })
        .then((output) => {
          // The write committed in Rust regardless of whether the UI is
          // still mounted. Invalidate the library cache BEFORE the guard
          // return so `/library` refetches fresh truth on its next mount
          // — otherwise a flush fired during `goBack` → `navigate` would
          // commit in the DB but leave the overview stale.
          invalidateLibraryOverviewCache();
          if (callId === activeCallRef.current) {
            saveInFlightRef.current = false;
            // The autosave UPDATE deletes any `story_drafts` row for
            // this story in the same SQLite transaction, so the buffer
            // is provably empty after a confirmed success.
            hasPendingDraftRef.current = false;
          }
          if (!mountedRef.current || callId !== activeCallRef.current) return;
          setState((prev) => {
            if (prev.kind !== "ready" || prev.detail.id !== detailId) {
              return prev;
            }
            const draftNormalized = normalizeStoryTitle(prev.draftTitle);
            const detail = {
              ...prev.detail,
              title: output.title,
              updatedAt: output.updatedAt,
            };
            // The user has since typed a new value: commit the detail
            // (it IS persisted) and schedule a fresh debounce so the
            // newer draft actually lands durable. Painting `Enregistré`
            // here would lie — the field shows a value different from
            // what was just committed. Leaving the status in `pending`
            // without a timer would silently strand the newest input
            // until the next keystroke or unmount.
            if (draftNormalized !== output.title) {
              scheduleDebouncedSave();
              return { ...prev, detail, saveStatus: { kind: "pending" } };
            }
            return {
              ...prev,
              detail,
              saveStatus: { kind: "saved", at: output.updatedAt },
            };
          });
          savedIdleTimerRef.current = setTimeout(() => {
            savedIdleTimerRef.current = null;
            if (!mountedRef.current) return;
            setState((prev) =>
              prev.kind === "ready" && prev.saveStatus.kind === "saved"
                ? { ...prev, saveStatus: { kind: "idle" } }
                : prev,
            );
          }, savedVisibleMs);
        })
        .catch((err: unknown) => {
          if (callId === activeCallRef.current) {
            saveInFlightRef.current = false;
          }
          if (!mountedRef.current || callId !== activeCallRef.current) return;
          // Failure leaves the persisted state untouched (NFR9 atomicity:
          // the UPDATE either commits or is rolled back). The library
          // cache still reflects the previously-committed value — no
          // invalidation needed. The overview invalidates only on
          // confirmed success, matching the autosave contract.
          const error = toAppError(err);
          setState((prev) => {
            if (prev.kind !== "ready" || prev.detail.id !== detailId) {
              return prev;
            }
            return {
              ...prev,
              saveStatus: {
                kind: "failed",
                error,
                attemptedTitle: normalizedTitle,
              },
            };
          });
        });
    },
    [clearSavedIdle, savedVisibleMs, scheduleDebouncedSave],
  );

  // Keep the forward reference in sync so the debounce callback can
  // reach `fireSave` without creating a dependency cycle.
  fireSaveRef.current = fireSave;

  const setDraftTitle = useCallback(
    (next: string) => {
      setState((prev) => {
        if (prev.kind !== "ready") return prev;
        const normalizedNext = normalizeStoryTitle(next);
        const persisted = prev.detail.title;

        // Replan the debounce on every keystroke: cancel the old timer so
        // only the latest input plans a save, and drop the "saved → idle"
        // countdown so the chip reflects the in-progress edit.
        clearDebounce();
        clearSavedIdle();

        if (normalizedNext === persisted) {
          // The user typed their way back to the persisted value. No save
          // needed — but clear any stale failure alert so the UI does not
          // keep warning about an error that no longer applies.
          // Cancel any pending record_draft too: the canonical row
          // already reflects the value we would buffer.
          clearRecordDraft();
          // If a draft buffer is still on disk from an earlier burst,
          // drop it best-effort. The IPC call lives OUTSIDE the
          // setState updater so React.StrictMode (which double-fires
          // updaters in dev) does not double-fire the IPC. The state
          // mutation we want is a side-effect-free flip of
          // `hasPendingDraftRef`; the actual `discardDraft` is queued
          // via a synchronous check on the ref before the setState
          // returns. We capture the storyId locally because the ref
          // read happens before the updater runs to completion.
          const shouldDiscard = hasPendingDraftRef.current;
          if (shouldDiscard) {
            hasPendingDraftRef.current = false;
          }
          // We schedule the IPC AFTER React has committed the new
          // state by using `queueMicrotask`. A direct call would still
          // run inside the updater on the second StrictMode pass even
          // though we cleared the ref — `queueMicrotask` defers it
          // until after the synchronous render so only one IPC fires.
          if (shouldDiscard) {
            const storyIdForDiscard = prev.detail.id;
            queueMicrotask(() => {
              void discardDraft({ storyId: storyIdForDiscard }).catch(
                () => undefined,
              );
            });
          }
          return {
            ...prev,
            draftTitle: next,
            saveStatus: { kind: "idle" },
          };
        }

        scheduleDebouncedSave();
        // Plan a recovery-draft buffer in parallel with the autosave.
        // A 150 ms debounce protects against a kill -9 between two
        // keystrokes — the autosave alone cannot, because its 500 ms
        // window leaves 350 ms uncovered.
        scheduleRecordDraft(prev.detail.id, next);

        return {
          ...prev,
          draftTitle: next,
          saveStatus: { kind: "pending" },
        };
      });
    },
    [clearDebounce, clearRecordDraft, clearSavedIdle, scheduleDebouncedSave, scheduleRecordDraft],
  );

  const retry = useCallback(() => {
    load();
  }, [load]);

  const retrySave = useCallback(() => {
    const current = stateRef.current;
    if (current.kind !== "ready") return;
    if (current.saveStatus.kind !== "failed") return;
    // Guard against synchronous re-entrancy: a second `retrySave` call
    // fired before React re-renders would still see `failed` in
    // `stateRef.current` and queue a duplicate `saveStory`. The
    // in-flight ref flips synchronously inside `fireSave`.
    if (saveInFlightRef.current) return;
    clearDebounce();
    fireSave(current.saveStatus.attemptedTitle);
  }, [clearDebounce, fireSave]);

  const flushAutoSave = useCallback(() => {
    const current = stateRef.current;
    if (current.kind !== "ready") return;
    const normalized = normalizeStoryTitle(current.draftTitle);
    if (normalized === current.detail.title) return;
    clearDebounce();
    // `fireSave` bumps `activeCallRef`, so a save already in flight is
    // superseded by this fresher attempt: its own then/catch becomes a
    // no-op and will not clobber the newer draft. Without this call the
    // user's latest typed value would be silently lost when the prior
    // save ACKs against an older input.
    fireSave(normalized);
  }, [clearDebounce, fireSave]);

  /**
   * Patch the in-memory `detail` from a successful `applyRecovery`
   * output. Equivalent to a re-fetch but without the IPC round-trip.
   *
   * The recovery flow has just produced an authoritative `UPDATE` of the
   * story, so the in-flight `detail` snapshot is stale. We:
   * - bump `activeCallRef` so any pending save / draft timer becomes
   *   a no-op (their `callId` no longer matches);
   * - drop pending timers (the user just decided what to commit, we
   *   should not re-fire an autosave for their old keystroke);
   * - rewrite `detail.title` / `detail.updatedAt` from the output;
   * - reset `draftTitle` to the new persisted value and the chip
   *   to `idle`.
   */
  const reloadDetailFromOutput = useCallback(
    (output: UpdateStoryOutput) => {
      activeCallRef.current += 1;
      saveInFlightRef.current = false;
      clearDebounce();
      clearSavedIdle();
      clearRecordDraft();
      setState((prev) => {
        if (prev.kind !== "ready" || prev.detail.id !== output.id) return prev;
        return {
          kind: "ready",
          detail: {
            ...prev.detail,
            title: output.title,
            updatedAt: output.updatedAt,
          },
          draftTitle: output.title,
          saveStatus: { kind: "idle" },
        };
      });
    },
    [clearDebounce, clearRecordDraft, clearSavedIdle],
  );

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
      // Best-effort flush: if an autosave was pending but never fired, try
      // to commit the latest value before the route disappears. The
      // recovery-draft buffer is cancelled outright — its purpose is to
      // protect against a hard kill, and a clean unmount is exactly the
      // scenario where the autosave or the goBack flush takes over.
      if (debounceTimerRef.current !== null) {
        clearTimeout(debounceTimerRef.current);
        debounceTimerRef.current = null;
      }
      if (recordDraftTimerRef.current !== null) {
        clearTimeout(recordDraftTimerRef.current);
        recordDraftTimerRef.current = null;
      }
      const current = stateRef.current;
      if (current.kind === "ready") {
        const normalized = normalizeStoryTitle(current.draftTitle);
        const detailId = current.detail.id;
        const liveDraftTitle = current.draftTitle;
        // Skip the cleanup flush when an in-flight save is already
        // carrying the user's latest value (goBack fired
        // `flushAutoSave` synchronously and bumped `activeCallRef`).
        const inFlightSaveCovers = current.saveStatus.kind === "saving";
        if (!inFlightSaveCovers && normalized !== current.detail.title) {
          // P40/D5: cleanup is sequential, never parallel.
          //  - If the last save FAILED, do not re-fire `saveStory`
          //    (the user implicitly chose to leave without retrying).
          //    Persist a recovery buffer instead so the next session
          //    can still surface the typed value.
          //  - Otherwise, fire `saveStory` and chain a recovery-buffer
          //    fallback in the catch path. A `saveStory` success
          //    confirms the value is durable; the chained
          //    `recordDraft` only fires when the save itself failed.
          //    The two operations NEVER run in parallel — that would
          //    let a slow-but-eventually-successful `saveStory` race
          //    with a `recordDraft` of the same value, and the
          //    autosave's atomic DELETE FROM story_drafts could then
          //    drop the buffer the user just confirmed.
          // `Promise.resolve().then(...)` indirection guards against
          // a mocked façade that returns `undefined` instead of a
          // Promise — the `.catch` on a non-thenable would throw and
          // crash the unmount path under fake-timer-driven tests.
          if (current.saveStatus.kind === "failed") {
            void Promise.resolve()
              .then(() =>
                recordDraft({ storyId: detailId, draftTitle: liveDraftTitle }),
              )
              .catch(() => undefined);
          } else {
            void Promise.resolve()
              .then(() => saveStory({ id: detailId, title: normalized }))
              .then(() => invalidateLibraryOverviewCache())
              .catch(() => {
                invalidateLibraryOverviewCache();
                // The save failed and there is no UI to surface the
                // error anymore. Fall back to the recovery buffer so
                // the next session can still propose the typed value.
                return Promise.resolve()
                  .then(() =>
                    recordDraft({
                      storyId: detailId,
                      draftTitle: liveDraftTitle,
                    }),
                  )
                  .catch(() => undefined);
              });
          }
        }
      }
      if (savedIdleTimerRef.current !== null) {
        clearTimeout(savedIdleTimerRef.current);
        savedIdleTimerRef.current = null;
      }
    };
  }, [load]);

  return {
    state,
    setDraftTitle,
    retry,
    retrySave,
    flushAutoSave,
    reloadDetailFromOutput,
  };
}
