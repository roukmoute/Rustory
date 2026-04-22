import type React from "react";

import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import { SurfacePanel } from "../../../shared/ui";

import "./StoryCard.css";

export type StoryCardSelectionMode = "replace" | "toggle";

export interface StoryCardProps {
  story: StoryCardDto;
  isSelected: boolean;
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
    // A click on an already-selected card is a no-op. This keeps a
    // multi-selection intact when the user starts a double-click on one of
    // the selected cards — the first click would otherwise collapse the
    // selection to a singleton before the dblclick fires.
    if (isSelected && !event.metaKey && !event.ctrlKey) return;
    const mode: StoryCardSelectionMode =
      event.metaKey || event.ctrlKey ? "toggle" : "replace";
    onSelect(story.id, mode);
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
        <span className="story-card__marker" aria-hidden="true">
          {isSelected ? "✓" : ""}
        </span>
        <h3 className="story-card__title">{story.title}</h3>
        <span className="story-card__meta" aria-label="Identifiant court">
          {story.id.slice(0, 8)}
        </span>
      </div>
    </SurfacePanel>
  );
}
