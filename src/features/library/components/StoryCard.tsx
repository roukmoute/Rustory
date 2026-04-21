import type React from "react";

import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import { SurfacePanel } from "../../../shared/ui";

import "./StoryCard.css";

export interface StoryCardProps {
  story: StoryCardDto;
}

/**
 * Default read-only projection of a single story. Selection, double-click to
 * edit, and derived state badges are explicitly out of scope here — they'll
 * land once selection and the transfer-state contract are wired end-to-end.
 *
 * Keyboard reachability: the card is a `group` region with `tabIndex={0}` so
 * it participates in the library tab sequence (search → sort → filter →
 * first card) even before selection is wired.
 */
export function StoryCard({ story }: StoryCardProps): React.JSX.Element {
  return (
    <SurfacePanel
      elevation={1}
      as="article"
      className="story-card"
    >
      <div
        className="story-card__focusable"
        tabIndex={0}
        role="group"
        aria-label={story.title}
      >
        <h3 className="story-card__title">{story.title}</h3>
        <span className="story-card__meta" aria-label="Identifiant court">
          {story.id.slice(0, 8)}
        </span>
      </div>
    </SurfacePanel>
  );
}
