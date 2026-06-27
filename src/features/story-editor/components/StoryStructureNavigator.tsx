import type React from "react";
import { useId, useMemo } from "react";

import "./StoryStructureNavigator.css";

/**
 * Minimal front projection of the canonical structure, used for DISPLAY
 * ONLY. The v1 canonical model is `{ "schemaVersion": 1, "nodes": [] }`
 * with an always-empty `nodes` array; the frontend deliberately does NOT
 * model seasons / nodes / option links — when a real node model exists it
 * will be projected FROM Rust, never recomposed here.
 */
type ParsedStructure = { kind: "ok" } | { kind: "unreadable" };

/**
 * Defensive read of `structureJson` for presentation. Rust is authoritative
 * and the bytes are covered by `content_checksum`, so a drift is
 * near-impossible — but a corrupted or drifted payload must degrade to a
 * NAMED state, never crash the editor and never be masked as a normal empty
 * structure. The bytes are never re-serialized or reformatted: we only read
 * them to decide what to show.
 *
 * The v1 shell can honestly project ONLY the v1 canonical reality:
 * `schemaVersion === 1` with an empty node list. A missing / future /
 * non-numeric `schemaVersion`, a
 * non-array `nodes`, or any non-empty `nodes` is a contract drift the v1 shell
 * cannot render — it resolves to the named degraded state, not to a silent
 * "normal empty" view. The real node model and its Rust-side projection land
 * later; until then, anything beyond the v1 shape is surfaced honestly.
 */
export function parseStoryStructure(structureJson: string): ParsedStructure {
  let value: unknown;
  try {
    value = JSON.parse(structureJson);
  } catch {
    return { kind: "unreadable" };
  }
  if (typeof value !== "object" || value === null) {
    return { kind: "unreadable" };
  }
  const candidate = value as Record<string, unknown>;
  // Strict v1 gate: any other schema version (missing, future, non-numeric)
  // is a drift the v1 shell must not pass off as a normal structure.
  if (candidate.schemaVersion !== 1) {
    return { kind: "unreadable" };
  }
  // v1 carries an empty node list. A non-array or non-empty `nodes` is drift.
  if (!Array.isArray(candidate.nodes) || candidate.nodes.length !== 0) {
    return { kind: "unreadable" };
  }
  return { kind: "ok" };
}

export interface StoryStructureNavigatorProps {
  /** Persisted story title, shown as the structure root. */
  title: string;
  /** Exact `structureJson` bytes from `StoryDetailDto` — parsed read-only. */
  structureJson: string;
}

/**
 * `Story Structure Navigator` — the editor's global-structure zone.
 *
 * v1 reality: the canonical structure carries no season and no node, so the
 * navigator shows the story as the structure root plus a NAMED empty state
 * (UX-DR38), never a blank panel and never a fabricated node. The zone is a
 * keyboard focus stop (the global `:focus-visible` ring makes it visible) so
 * the structure participates in the stable focus order even before a real
 * tree exists. Meaning is carried by glyph + text, never color alone.
 */
export function StoryStructureNavigator({
  title,
  structureJson,
}: StoryStructureNavigatorProps): React.JSX.Element {
  const parsed = useMemo(
    () => parseStoryStructure(structureJson),
    [structureJson],
  );
  // Name the region by pointing at the visible heading rather than repeating
  // its text in an aria-label — one source of truth for the accessible name.
  const headingId = useId();

  return (
    <section
      className="story-structure-navigator"
      aria-labelledby={headingId}
    >
      <h2 id={headingId} className="story-structure-navigator__heading">
        Structure de l'histoire
      </h2>
      {parsed.kind === "unreadable" ? (
        // The degraded state is ALSO a keyboard focus stop, so the structure
        // zone stays reachable (with a visible focus ring) even when the
        // payload could not be read — mirroring the focusable root of the
        // normal case (AC3).
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
          {/* The story itself is the structure root. Focusable so the
              structure zone is reachable at the keyboard in v1, before any
              real node exists (the arrow-key tree navigation lands when the
              node model does). */}
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
          {/* The parse guarantees the v1 empty node list, so the named empty
              state is always the honest rendering here. */}
          <p className="story-structure-navigator__empty">
            Aucune saison ni nœud pour l'instant.
          </p>
        </div>
      )}
    </section>
  );
}
