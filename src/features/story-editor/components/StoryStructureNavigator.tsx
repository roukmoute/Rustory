import type React from "react";
import { useId } from "react";

import type { NodeContentDto } from "../../../shared/ipc-contracts/story";

import "./StoryStructureNavigator.css";

export interface StoryStructureNavigatorProps {
  /** Persisted story title, shown as the structure root. */
  title: string;
  /** The current node PROJECTED BY RUST (`detail.node`), or `null` when the
   *  structure could not be projected (degraded). */
  node: NodeContentDto | null;
  /** The id of the node currently in focus, so it is clearly marked (AC3). */
  currentNodeId: string | null;
}

/**
 * `Story Structure Navigator` — the editor's global-structure zone.
 *
 * It consumes the current node PROJECTED FROM RUST (never re-parsing
 * `structureJson`): the story is the structure root and the current node hangs
 * under it, clearly identified by its stable id so the focus/identity never
 * drifts across a long edit session (AC3). When Rust could not project a node
 * (a corrupt / drifted structure — near-impossible since the bytes are
 * checksum-guarded) the zone degrades to a NAMED state (`Structure illisible`),
 * never a crash and never a fabricated node. Meaning is carried by glyph +
 * text, never color alone.
 */
export function StoryStructureNavigator({
  title,
  node,
  currentNodeId,
}: StoryStructureNavigatorProps): React.JSX.Element {
  const headingId = useId();

  return (
    <section className="story-structure-navigator" aria-labelledby={headingId}>
      <h2 id={headingId} className="story-structure-navigator__heading">
        Structure de l'histoire
      </h2>
      {node === null ? (
        <p className="story-structure-navigator__degraded" tabIndex={0}>
          <span
            className="story-structure-navigator__glyph"
            aria-hidden="true"
          >
            !
          </span>
          Structure illisible.
        </p>
      ) : (
        <div className="story-structure-navigator__tree">
          <div className="story-structure-navigator__root" tabIndex={0}>
            <span
              className="story-structure-navigator__glyph"
              aria-hidden="true"
            >
              •
            </span>
            <span className="story-structure-navigator__root-label">
              {title}
            </span>
          </div>
          {/* The single current node, projected from Rust and clearly marked
              as the one in focus (AC3). Focusable so it participates in the
              stable structure → node → actions focus order. */}
          <div
            className={
              node.id === currentNodeId
                ? "story-structure-navigator__node story-structure-navigator__node--current"
                : "story-structure-navigator__node"
            }
            tabIndex={0}
            aria-current={node.id === currentNodeId ? "true" : undefined}
          >
            <span
              className="story-structure-navigator__glyph"
              aria-hidden="true"
            >
              ›
            </span>
            <span className="story-structure-navigator__node-label">
              {node.label.trim().length > 0 ? node.label : "Nœud courant"}
            </span>
            {node.id === currentNodeId ? (
              <span className="story-structure-navigator__node-marker">
                {" "}
                — en cours d'édition
              </span>
            ) : null}
          </div>
        </div>
      )}
    </section>
  );
}
