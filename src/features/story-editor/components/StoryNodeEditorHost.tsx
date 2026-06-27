import type React from "react";
import { useId } from "react";

import "./StoryNodeEditorHost.css";

/**
 * `Nœud courant` host — the editor zone reserved for the future node editor.
 *
 * v1 reality: no node exists in the canonical model, so nothing is selectable
 * and there is nothing to edit. The zone renders a NAMED empty state
 * (UX-DR38) rather than a blank panel or a fabricated "current node", and
 * stays a keyboard focus stop (the global `:focus-visible` ring) so it keeps
 * its place in the stable structure → node → actions focus order. Editing a
 * node's content and media lands in a later iteration; this only reserves and
 * names the place it will live.
 */
export function StoryNodeEditorHost(): React.JSX.Element {
  // Name the region by its visible heading (aria-labelledby) rather than a
  // duplicate aria-label — one source of truth for the accessible name.
  const headingId = useId();
  return (
    <section className="story-node-editor-host" aria-labelledby={headingId}>
      <h2 id={headingId} className="story-node-editor-host__heading">
        Nœud courant
      </h2>
      <div className="story-node-editor-host__empty" tabIndex={0}>
        Aucun nœud à éditer pour l'instant.
      </div>
    </section>
  );
}
