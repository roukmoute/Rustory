import type React from "react";

import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import type {
  ImportFinding,
  ImportState,
} from "../../../shared/ipc-contracts/import-export";
import { StateChip, SurfacePanel } from "../../../shared/ui";

import "./StoryCard.css";

export type StoryCardSelectionMode = "replace" | "toggle";

/** Preparation / transfer state reflected as a discreet card badge (AC2 —
 *  "éléments prêts/bloquants"). Derived from `useStoryPreparation` /
 *  `useStoryTransfer`, never a new source of truth; the authoritative surface
 *  stays the decision panel. */
export type StoryPreparationBadge =
  | "preparing"
  | "retryable"
  | "transferring"
  | "incomplete"
  | "verified"
  | "partial";

export interface StoryCardProps {
  story: StoryCardDto;
  isSelected: boolean;
  /** Total number of selected cards in the collection. Needed so a plain
   *  click on the single selected card can deselect it, while preserving
   *  the no-op behavior when a multi-selection is in play (avoids a
   *  multi-select collapse on the first click of a double-click). */
  selectionSize?: number;
  /** Discreet preparation badge for this card. Omitted ⇒ no badge. */
  preparationBadge?: StoryPreparationBadge;
  onSelect: (id: string, mode: StoryCardSelectionMode) => void;
  onOpen: (id: string) => void;
}

/**
 * Interactive library card. Click replaces the selection, Ctrl/Cmd+click
 * toggles the id in/out of the current multi-selection, double-click opens
 * the edit route. Keyboard: Space toggles selection, Enter opens the draft.
 *
 * The selected state is signaled redundantly (border + visible prefix glyph +
 * `aria-pressed`) so the affordance survives grayscale and color-blindness
 * checks — never color-only.
 */
export function StoryCard({
  story,
  isSelected,
  selectionSize = 0,
  preparationBadge,
  onSelect,
  onOpen,
}: StoryCardProps): React.JSX.Element {
  const handleClick = (event: React.MouseEvent<HTMLDivElement>): void => {
    // Shift+click is reserved for range selection, which is explicitly out
    // of the MVP. Swallow it so power users don't get a silent `replace`.
    if (event.shiftKey) {
      event.preventDefault();
      return;
    }
    if (event.metaKey || event.ctrlKey) {
      onSelect(story.id, "toggle");
      return;
    }
    // Preserve a multi-selection against a stray first click of a
    // double-click sequence: on the already-selected card, a plain click
    // stays a no-op so the pending dblclick can still fire.
    if (isSelected && selectionSize > 1) return;
    // On a single-selected card, a plain click toggles the selection off.
    // Users expect a click to be the inverse of itself — forcing Ctrl+click
    // just to deselect surprises anyone who hasn't learned the shortcut.
    if (isSelected && selectionSize === 1) {
      onSelect(story.id, "toggle");
      return;
    }
    onSelect(story.id, "replace");
  };

  const handleDoubleClick = (event: React.MouseEvent<HTMLDivElement>): void => {
    event.preventDefault();
    onOpen(story.id);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>): void => {
    // Swallow OS-level key auto-repeat so a held Space / Enter does not
    // flicker the selection or navigate repeatedly.
    if (event.repeat) {
      if (event.key === " " || event.key === "Spacebar" || event.key === "Enter") {
        event.preventDefault();
      }
      return;
    }
    if (event.key === " " || event.key === "Spacebar") {
      event.preventDefault();
      onSelect(story.id, "toggle");
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      onOpen(story.id);
    }
  };

  const className = [
    "story-card__focusable",
    isSelected ? "story-card__focusable--selected" : null,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <SurfacePanel elevation={1} as="article" className="story-card">
      <div
        className={className}
        tabIndex={0}
        role="button"
        aria-pressed={isSelected}
        aria-label={story.title}
        onClick={handleClick}
        onDoubleClick={handleDoubleClick}
        onKeyDown={handleKeyDown}
      >
        {isSelected ? (
          <span className="story-card__marker" aria-hidden="true">
            ✓
          </span>
        ) : null}
        <h3 className="story-card__title">{story.title}</h3>
        {preparationBadge ? (
          <StateChip
            tone={badgeTone(preparationBadge)}
            label={badgeLabel(preparationBadge)}
            className="story-card__preparation-chip"
          />
        ) : null}
      </div>
      {story.importState ? (
        <ImportProvenance
          state={story.importState}
          report={story.importReport ?? []}
        />
      ) : null}
    </SurfacePanel>
  );
}

/**
 * Durable file-import provenance + issue marker (AC2), rendered OUTSIDE the
 * focusable selection button so the on-demand report disclosure never nests
 * an interactive element inside the card's `role="button"`. The chip labels
 * (`partiel` / `à revoir`) and warning tone are DEDICATED to the import flow
 * and deliberately distinct from the transfer/verification badge above — the
 * `partial` transfer sense (`état partiel`) is never reused here. The
 * detailed report appears only on demand via a native `<details>` and shows
 * BOTH groups — what Rustory recognized AND the points of attention (§5).
 */
function ImportProvenance({
  state,
  report,
}: {
  state: ImportState;
  report: ImportFinding[];
}): React.JSX.Element {
  const label = importMarkerLabel(state);
  const recognized = report.filter((f) => f.category === "recognized");
  const attention = report.filter((f) => f.category !== "recognized");
  return (
    <div className="story-card__import">
      <span className="story-card__provenance">Importée</span>
      {label ? (
        report.length > 0 ? (
          <details className="story-card__import-report">
            <summary className="story-card__import-summary">
              <StateChip
                tone="warning"
                label={label}
                className="story-card__import-chip"
              />
              <span className="story-card__import-disclose">
                Voir le rapport d'import
              </span>
            </summary>
            <div className="story-card__import-body">
              {recognized.length > 0 ? (
                <FindingGroup
                  heading="Ce que Rustory a reconnu"
                  findings={recognized}
                />
              ) : null}
              {attention.length > 0 ? (
                <FindingGroup
                  heading="Points d'attention"
                  findings={attention}
                />
              ) : null}
            </div>
          </details>
        ) : (
          <StateChip
            tone="warning"
            label={label}
            className="story-card__import-chip"
          />
        )
      ) : null}
    </div>
  );
}

function FindingGroup({
  heading,
  findings,
}: {
  heading: string;
  findings: ImportFinding[];
}): React.JSX.Element {
  return (
    <section className="story-card__import-group">
      <p className="story-card__import-heading">{heading}</p>
      <ul className="story-card__import-issues">
        {findings.map((finding) => (
          <li
            key={`${finding.aspect}-${finding.category}`}
            className="story-card__import-issue"
          >
            {finding.message}
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Dedicated import chip label (reserved for the import flow); `recognized`
 *  shows only the `Importée` provenance, with no issue chip (AC3). */
function importMarkerLabel(state: ImportState): string | null {
  switch (state) {
    case "partial":
      return "partiel";
    case "needsReview":
      return "à revoir";
    default:
      // `recognized` (and the non-card `blocked` / `resolved`) carry no
      // attention chip — only the provenance marker.
      return null;
  }
}

/** Canonical lowercase badge labels, kept in sync with the decision panel and
 *  `docs/architecture/product-language.md` (never color-only). */
function badgeLabel(badge: StoryPreparationBadge): string {
  switch (badge) {
    case "preparing":
      return "en préparation";
    case "transferring":
      return "en transfert";
    case "retryable":
      return "échec récupérable";
    case "incomplete":
      return "transfert incomplet";
    case "verified":
      return "transférée et vérifiée";
    case "partial":
      return "état partiel";
  }
}

/** Non-color-only tone per badge (paired with the distinct label above). */
function badgeTone(
  badge: StoryPreparationBadge,
): "neutral" | "info" | "success" | "warning" | "error" {
  switch (badge) {
    case "verified":
      return "success";
    case "retryable":
      return "error";
    case "incomplete":
    case "partial":
      return "warning";
    case "preparing":
    case "transferring":
      return "neutral";
  }
}
