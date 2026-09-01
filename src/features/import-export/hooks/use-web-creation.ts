import { useCallback, useEffect, useRef, useState } from "react";

import {
  acceptWebPodcastCreation,
  fetchWebPodcastPreview,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import type { AppError } from "../../../shared/errors/app-error";
import { toAppError } from "../../../shared/errors/app-error";
import type { WebPreview } from "../../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";

export type WebCreationStatus =
  | { kind: "idle" }
  | { kind: "fetching" }
  | {
      kind: "review";
      /** The address the preview was fetched from — the accept re-sends
       *  THIS one (the reviewed content's address), never the field's
       *  possibly-retyped value. */
      webUrl: string;
      preview: WebPreview;
      /** The accept refused honestly (`La page a changé depuis la
       *  récupération.`): the stale preview is dead — the surface
       *  renders the frozen verdict and offers a re-fetch. */
      sourceChanged: boolean;
    }
  | { kind: "creating" }
  | { kind: "created"; story: StoryCardDto }
  | { kind: "failed"; error: AppError }
  /** The content-source POLICY refused the flow
   *  (`CONTENT_SOURCE_UNAVAILABLE` — defence in depth, nominally
   *  unreachable since the dialog never activates a non-enabled source).
   *  A calm state, never confused with the transport `failed`: no retry
   *  gesture exists (a retry cannot change a distribution policy) — the
   *  only way out is `abandon`. */
  | { kind: "unavailable"; error: AppError };

export interface UseWebCreation {
  status: WebCreationStatus;
  /** Fetch + analyze the page at `url` (the ONLY networked action, on
   *  the explicit `Récupérer la page` click). Resolves when the preview
   *  has settled. A re-fetch from `review` replaces the preview. */
  fetchPreview(url: string): Promise<void>;
  /** Commit the FULL previewed page (all episodes, page order — no
   *  single-episode selection). No-op outside a live review. Rust
   *  re-fetches the page from zero and downloads every audio. */
  acceptCreation(): Promise<void>;
  /** Abandon the flow (pure frontend, NO mutation): reset to idle from
   *  ANY non-terminal state — including a long `fetching` / `creating`
   *  (the in-flight result is then ignored via a generation token; Rust
   *  may still settle its own atomic work, the UI just stops listening).
   *  The caller closes the surface. */
  abandon(): void;
  /** Dismiss a terminal status (`created` / `failed`) back to idle. */
  dismiss(): void;
}

/** Map a failed IPC call to its surface state: the policy refusal
 *  (`CONTENT_SOURCE_UNAVAILABLE`) lands on the dedicated calm
 *  `unavailable` state; every other failure (the motivated S4 / S5 /
 *  S6 refusals included) stays the transport `failed` (which keeps the
 *  field + `Réessayer`). Two sealed regimes — a policy refusal is never
 *  rendered as a breakage. */
function statusFromWebError(err: unknown): WebCreationStatus {
  const error = toAppError(err);
  if (error.code === "CONTENT_SOURCE_UNAVAILABLE") {
    return { kind: "unavailable", error };
  }
  return { kind: "failed", error };
}

/**
 * Orchestrates the two-phase web external-source creation through the
 * Rust-owned fetch + analyze + commit boundary. Structural sibling of
 * `useRssCreation` minus the single-episode selection: the web accept
 * commits the FULL page (every extracted episode, in page order) — the
 * review is a read-only confirmation, never a picker.
 *
 * No mutation before acceptance: `fetchPreview` is pure (Rust writes
 * zero byte, zero row); the library cache is invalidated ONLY after a
 * successful `acceptCreation`. `abandon` is a pure frontend reset.
 */
export function useWebCreation(): UseWebCreation {
  const [status, setStatus] = useState<WebCreationStatus>({ kind: "idle" });

  const statusRef = useRef<WebCreationStatus>(status);
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

  // Abandon generation: bumped by `abandon()` so a result that lands
  // AFTER an abandon is ignored instead of resurrecting a closed
  // surface.
  const generationRef = useRef(0);

  // Synchronous re-entrancy gate: holds the GENERATION of the in-flight
  // call (null when none). A double activation in the same tick is
  // blocked only while that call still belongs to the CURRENT generation
  // — an abandoned call (generation bumped) must never dead-lock the
  // reopened surface for the rest of its network budget; its late
  // settlement is already ignored by the generation guard.
  const inFlightGenerationRef = useRef<number | null>(null);

  const fetchPreview = useCallback(async (url: string): Promise<void> => {
    if (inFlightGenerationRef.current === generationRef.current) return;
    // A policy refusal is a dead end for THIS surface: no retry can
    // change a distribution decision, so the fetch action is a no-op
    // there (the only way out is `abandon`).
    if (statusRef.current.kind === "unavailable") return;
    const generation = generationRef.current;
    inFlightGenerationRef.current = generation;
    try {
      if (mountedRef.current) setStatus({ kind: "fetching" });
      let preview: WebPreview;
      try {
        preview = await fetchWebPodcastPreview(url);
      } catch (err) {
        if (!mountedRef.current || generationRef.current !== generation) {
          return;
        }
        setStatus(statusFromWebError(err));
        return;
      }
      if (!mountedRef.current || generationRef.current !== generation) return;
      setStatus({
        kind: "review",
        webUrl: url,
        preview,
        sourceChanged: false,
      });
    } finally {
      // Only release the gate if a NEWER call has not claimed it
      // already.
      if (inFlightGenerationRef.current === generation) {
        inFlightGenerationRef.current = null;
      }
    }
  }, []);

  const acceptCreation = useCallback(async (): Promise<void> => {
    if (inFlightGenerationRef.current === generationRef.current) return;
    const current = statusRef.current;
    // `unavailable` is covered by the review-only gate below; the accept
    // is a no-op there like every other retry-shaped action.
    if (current.kind !== "review") return;
    // A diverged review has nothing to create: the page no longer
    // matches the checksum, and the gesture is a re-fetch.
    if (current.sourceChanged) return;

    const generation = generationRef.current;
    inFlightGenerationRef.current = generation;
    try {
      if (mountedRef.current) setStatus({ kind: "creating" });
      try {
        const outcome = await acceptWebPodcastCreation(
          current.webUrl,
          current.preview.pageChecksum,
        );
        if (outcome.kind === "sourceChanged") {
          // Honest refusal: nothing was created. The stale preview is
          // dead — back to the review with the frozen verdict and a
          // re-fetch.
          if (!mountedRef.current || generationRef.current !== generation) {
            return;
          }
          setStatus({ ...current, sourceChanged: true });
          return;
        }
        // The canonical store HAS changed — drop the stale overview
        // snapshot BEFORE the mounted/generation guards so an unmount
        // or an abandon mid-creation still reconciles on the next mount
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
        setStatus(statusFromWebError(err));
      }
    } finally {
      if (inFlightGenerationRef.current === generation) {
        inFlightGenerationRef.current = null;
      }
    }
  }, []);

  const abandon = useCallback((): void => {
    // Pure frontend reset — nothing the UI can roll back was mutated.
    // From a long state the in-flight result is ignored via the
    // generation token (an accept that already reached Rust still
    // settles atomically there; the fresh card then appears on the next
    // authoritative overview read — never a resurrected surface).
    // `unavailable` exits through here too: closing IS the policy
    // refusal's only gesture.
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
    acceptCreation,
    abandon,
    dismiss,
  };
}
