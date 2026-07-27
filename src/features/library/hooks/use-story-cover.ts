import { useEffect, useState } from "react";

import { readNodeMedia } from "../../../ipc/commands/story";

/**
 * Module-local cache of loaded cover data URLs, keyed by
 * `storyId:assetId`. Asset ids are content-addressed and REPLACED when the
 * user changes a node image, so the key self-invalidates on edit — no
 * explicit invalidation needed. Shared across card instances so a re-render
 * of the collection never refetches every cover.
 */
const cache = new Map<string, string>();

/**
 * Load a library story's cover (its start node's display PNG) as a data URL
 * through the existing `read_node_media` boundary. `null` while absent /
 * loading / failed — a cover is decoration: a load failure renders nothing,
 * never an error surface.
 */
export function useStoryCover(
  storyId: string,
  coverAssetId: string | undefined,
): string | null {
  const key = coverAssetId ? `${storyId}:${coverAssetId}` : null;
  const [dataUrl, setDataUrl] = useState<string | null>(() =>
    key ? (cache.get(key) ?? null) : null,
  );

  useEffect(() => {
    if (!key || !coverAssetId) {
      setDataUrl(null);
      return;
    }
    const cached = cache.get(key);
    if (cached) {
      setDataUrl(cached);
      return;
    }
    let alive = true;
    readNodeMedia({ storyId, assetId: coverAssetId })
      .then((preview) => {
        cache.set(key, preview.dataUrl);
        if (alive) setDataUrl(preview.dataUrl);
      })
      .catch(() => {
        // Decorative: a missing/corrupt cover simply renders nothing.
        if (alive) setDataUrl(null);
      });
    return () => {
      alive = false;
    };
  }, [key, storyId, coverAssetId]);

  return dataUrl;
}
