import { useCallback, useEffect, useRef, useState } from "react";

import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { normalizeStoryTitle } from "../../library/validation/story-title";
import { getStoryDetail, saveStory } from "../../../ipc/commands/story";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { StoryDetailDto } from "../../../shared/ipc-contracts/story";
import { isStoryDetailDto } from "../../../shared/ipc-contracts/story";

/** Autosave debounce window after the last keystroke, in milliseconds. */
export const STORY_AUTOSAVE_DEBOUNCE_MS = 500;
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
}

interface UseStoryEditorOptions {
  debounceMs?: number;
  savedVisibleMs?: number;
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

  const load = useCallback(() => {
    if (!storyId) {
      setState({ kind: "not-found" });
      return;
    }
    const callId = ++activeCallRef.current;
    clearDebounce();
    clearSavedIdle();
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
          return {
            ...prev,
            draftTitle: next,
            saveStatus: { kind: "idle" },
          };
        }

        scheduleDebouncedSave();

        return {
          ...prev,
          draftTitle: next,
          saveStatus: { kind: "pending" },
        };
      });
    },
    [clearDebounce, clearSavedIdle, scheduleDebouncedSave],
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

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
      // Best-effort flush: if an autosave was pending but never fired, try
      // to commit the latest value before the route disappears. Explicit
      // recovery of an un-flushed draft after a crash is a separate
      // feature and not handled here.
      if (debounceTimerRef.current !== null) {
        clearTimeout(debounceTimerRef.current);
        debounceTimerRef.current = null;
      }
      const current = stateRef.current;
      if (current.kind === "ready") {
        const normalized = normalizeStoryTitle(current.draftTitle);
        // Skip the cleanup flush when:
        // - an in-flight save is already carrying the user's latest
        //   value (goBack fired `flushAutoSave` synchronously and
        //   bumped `activeCallRef`);
        // - the last save already failed and the user did NOT click
        //   `Réessayer l'enregistrement` — re-firing automatically
        //   would contradict the user's implicit decision to leave the
        //   page without retrying.
        const skip =
          current.saveStatus.kind === "saving" ||
          current.saveStatus.kind === "failed";
        if (!skip && normalized !== current.detail.title) {
          // Fire-and-forget: the UI is unmounting so we will not observe
          // the resolution. Rust still processes the UPDATE so the next
          // reopen observes the latest committed value. Cache invalidation
          // is safe because it simply drops a module-local snapshot that
          // the next `/library` mount will refetch.
          saveStory({ id: current.detail.id, title: normalized })
            .then(() => invalidateLibraryOverviewCache())
            .catch(() => {
              // No UI to surface the failure to anymore. The prior
              // persisted state remains intact by invariant.
              invalidateLibraryOverviewCache();
            });
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
  };
}
