import { useEffect, useState } from "react";

import { readPackCover } from "../../../ipc/commands/device-catalog";

/**
 * Module-local cache of resolved covers, keyed by pack UUID. Holds the
 * `data:` URL string, or `null` once we know a pack has no cached cover, so
 * repeated renders/cards never re-hit the backend. Shared across hook
 * instances (the same pack shown in the collection and the inspector
 * resolves once).
 */
const coverCache = new Map<string, string | null>();

/** Drop the cached covers — call after a catalog refresh/import so the new
 *  covers (or their absence) are re-resolved. */
export function invalidatePackCoverCache(): void {
  coverCache.clear();
}

/**
 * Resolve the cached cover for `packUuid` to a `data:` URL, or `null` when
 * there is none (or `hasCover` is false — the DTO's `thumbnail` signals
 * presence, so a caller passes `story.thumbnail !== null`). Purely a LOCAL
 * read behind `read_pack_cover`; never triggers a network call. A failure or
 * absence resolves to `null` — covers are decorative.
 */
export function usePackCover(
  packUuid: string,
  hasCover: boolean,
): string | null {
  const [url, setUrl] = useState<string | null>(() =>
    hasCover ? (coverCache.get(packUuid) ?? null) : null,
  );

  useEffect(() => {
    if (!hasCover) {
      setUrl(null);
      return;
    }
    if (coverCache.has(packUuid)) {
      setUrl(coverCache.get(packUuid) ?? null);
      return;
    }
    let active = true;
    readPackCover(packUuid)
      .then((dto) => {
        const resolved = dto?.dataUrl ?? null;
        coverCache.set(packUuid, resolved);
        if (active) setUrl(resolved);
      })
      .catch(() => {
        // Decorative: a failed cover read just shows no cover.
        if (active) setUrl(null);
      });
    return () => {
      active = false;
    };
  }, [packUuid, hasCover]);

  return url;
}
