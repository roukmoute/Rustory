import type React from "react";

import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import { SurfacePanel } from "../../../shared/ui";

import "./StoryCard.css";

export type StoryCardSelectionMode = "replace" | "toggle";

export interface StoryCardProps {
  story: StoryCardDto;
  isSelected: boolean;
  /** Total number of selected cards in the collection. Needed so a plain
   *  click on the single selected card can deselect it, while preserving
   *  the no-op behavior when a multi-selection is in play (avoids a
   *  multi-select collapse on the first click of a double-click). */
  selectionSize?: number;
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
      </div>
    </SurfacePanel>
  );
}
