import { useCallback, useEffect, useRef, useState } from "react";

import {
  acceptRssStoryCreation,
  fetchRssSourcePreview,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";
import type {
  RssItemRef,
  RssPreview,
} from "../../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";

export type RssCreationStatus =
  | { kind: "idle" }
  | { kind: "fetching" }
  | {
      kind: "review";
      /** The address the preview was fetched from — the accept re-sends
       *  THIS one (the reviewed content's address), never the field's
       *  possibly-retyped value. */
      feedUrl: string;
      preview: RssPreview;
      selectedItemRef: RssItemRef | null;
      /** The accept refused honestly (`La source a changé depuis la
       *  récupération.`): the stale items are dead — the surface renders
       *  the frozen verdict and offers a re-fetch. */
      sourceChanged: boolean;
    }
  | { kind: "creating" }
  | { kind: "created"; story: StoryCardDto }
  | { kind: "failed"; error: AppError };

export interface UseRssCreation {
  status: RssCreationStatus;
  /** Fetch + analyze the feed at `url` (the ONLY networked action, on the
   *  explicit `Récupérer le flux` click). Resolves when the preview has
   *  settled. A re-fetch from `review` replaces the preview. */
  fetchPreview(url: string): Promise<void>;
  /** Select one previewed item (`review` only; no-op on a blocked or
   *  source-changed review). */
  selectItem(ref: RssItemRef): void;
  /** Commit the selected item (`Créer le brouillon`). No-op outside a
   *  selectable `review`. Rust re-fetches the feed from zero. */
  acceptCreation(): Promise<void>;
  /** Abandon the flow (pure frontend, NO mutation): reset to idle from ANY
   *  non-terminal state — including a long `fetching` / `creating` (the
   *  in-flight result is then ignored via a generation token; Rust may
   *  still settle its own atomic work, the UI just stops listening). The
   *  caller closes the surface. */
  abandon(): void;
  /** Dismiss a terminal status (`created` / `failed`) back to idle. */
  dismiss(): void;
}

/**
 * Orchestrates the two-phase RSS external-source creation through the
 * Rust-owned fetch + analyze + commit boundary. Structural sibling of
 * `useStructuredCreation` — the duplication is DELIBERATE (a creation-flow
 * orchestrator is a context orchestrator, never a generic reusable
 * component); the shared pieces are the label helpers, not the machines.
 *
 * No mutation before acceptance: `fetchPreview` is pure (Rust writes zero
 * byte, zero row); the library cache is invalidated ONLY after a
 * successful `acceptCreation`. `abandon` is a pure frontend reset.
 */
export function useRssCreation(): UseRssCreation {
  const [status, setStatus] = useState<RssCreationStatus>({ kind: "idle" });

  const statusRef = useRef<RssCreationStatus>(status);
  statusRef.current = status;

  // StrictMode-safe mount flag: set on every mount phase so a synthetic
  // unmount+remount re-arms it.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Abandon generation: bumped by `abandon()` so a result that lands AFTER
  // an abandon is ignored instead of resurrecting a closed surface.
  const generationRef = useRef(0);

  // Synchronous re-entrancy gate: holds the GENERATION of the in-flight
  // call (null when none). A double activation in the same tick is blocked
  // only while that call still belongs to the CURRENT generation — an
  // abandoned call (generation bumped) must never dead-lock the reopened
  // surface for the rest of its network budget; its late settlement is
  // already ignored by the generation guard.
  const inFlightGenerationRef = useRef<number | null>(null);

  const fetchPreview = useCallback(async (url: string): Promise<void> => {
    if (inFlightGenerationRef.current === generationRef.current) return;
    const generation = generationRef.current;
    inFlightGenerationRef.current = generation;
    try {
      if (mountedRef.current) setStatus({ kind: "fetching" });
      let preview: RssPreview;
      try {
        preview = await fetchRssSourcePreview(url);
      } catch (err) {
        if (!mountedRef.current || generationRef.current !== generation) {
          return;
        }
        setStatus({ kind: "failed", error: toAppError(err) });
        return;
      }
      if (!mountedRef.current || generationRef.current !== generation) return;
      setStatus({
        kind: "review",
        feedUrl: url,
        preview,
        selectedItemRef: null,
        sourceChanged: false,
      });
    } finally {
      // Only release the gate if a NEWER call has not claimed it already.
      if (inFlightGenerationRef.current === generation) {
        inFlightGenerationRef.current = null;
      }
    }
  }, []);

  const selectItem = useCallback((ref: RssItemRef): void => {
    const current = statusRef.current;
    if (current.kind !== "review") return;
    if (current.preview.blocked || current.sourceChanged) return;
    setStatus({ ...current, selectedItemRef: ref });
  }, []);

  const acceptCreation = useCallback(async (): Promise<void> => {
    if (inFlightGenerationRef.current === generationRef.current) return;
    const current = statusRef.current;
    if (current.kind !== "review") return;
    // A blocked or diverged review has nothing to create; the CTA needs a
    // selected item.
    if (current.preview.blocked || current.sourceChanged) return;
    if (current.selectedItemRef === null) return;

    const generation = generationRef.current;
    inFlightGenerationRef.current = generation;
    try {
      if (mountedRef.current) setStatus({ kind: "creating" });
      try {
        const outcome = await acceptRssStoryCreation(
          current.feedUrl,
          current.selectedItemRef,
        );
        if (outcome.kind === "sourceChanged") {
          // Honest refusal: nothing was created. The stale items are dead
          // — back to the review with the frozen verdict and a re-fetch.
          if (!mountedRef.current || generationRef.current !== generation) {
            return;
          }
          setStatus({
            ...current,
            selectedItemRef: null,
            sourceChanged: true,
          });
          return;
        }
        // The canonical store HAS changed — drop the stale overview
        // snapshot BEFORE the mounted/generation guards so an unmount or
        // an abandon mid-creation still reconciles on the next mount
        // (Rust DID commit — only the LISTENING stops on abandon).
        invalidateLibraryOverviewCache();
        if (!mountedRef.current || generationRef.current !== generation) {
          return;
        }
        setStatus({ kind: "created", story: outcome.story });
      } catch (err) {
        if (!mountedRef.current || generationRef.current !== generation) {
          return;
        }
        setStatus({ kind: "failed", error: toAppError(err) });
      }
    } finally {
      if (inFlightGenerationRef.current === generation) {
        inFlightGenerationRef.current = null;
      }
    }
  }, []);

  const abandon = useCallback((): void => {
    // Pure frontend reset — nothing the UI can roll back was mutated. From
    // a long state the in-flight result is ignored via the generation
    // token (an accept that already reached Rust still settles atomically
    // there; the fresh card then appears on the next authoritative
    // overview read — never a resurrected surface).
    const kind = statusRef.current.kind;
    if (kind !== "created" && kind !== "failed") {
      generationRef.current += 1;
      setStatus({ kind: "idle" });
    }
  }, []);

  const dismiss = useCallback((): void => {
    const kind = statusRef.current.kind;
    if (kind === "created" || kind === "failed") {
      setStatus({ kind: "idle" });
    }
  }, []);

  return {
    status,
    fetchPreview,
    selectItem,
    acceptCreation,
    abandon,
    dismiss,
  };
}
