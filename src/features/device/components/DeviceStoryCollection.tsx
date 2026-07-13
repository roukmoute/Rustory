import type React from "react";
import { useLayoutEffect, useRef } from "react";

import { LibraryErrorBanner } from "../../library/components/LibraryErrorBanner";
import { ProgressIndicator, StateChip, SurfacePanel } from "../../../shared/ui";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";
import type { DeviceLibraryState } from "../hooks/use-device-library";
import { usePackCover } from "../hooks/use-pack-cover";
import {
  titleProvenanceChip,
  titleProvenancePhrase,
} from "../title-provenance";

import "./DeviceStoryCollection.css";

export interface DeviceStoryCollectionProps {
  state: DeviceLibraryState;
  /** True while a re-read runs on top of an already-displayed snapshot. */
  isRefreshing: boolean;
  /** Human label of the connected device (e.g. "Lunii V3"), shown in the
   *  section heading so the provenance is explicit. */
  deviceLabel?: string;
  /** UUID of the device story currently selected for inspection, or
   *  null/undefined when none is. Only meaningful when `onSelectStory` is
   *  wired. */
  selectedUuid?: string | null;
  /** Select (or, on the already-selected card, deselect) a device story for
   *  inspection. When omitted, the cards stay non-interactive — listing
   *  only, the pre-inspection behavior. */
  onSelectStory?: (uuid: string) => void;
  /** Re-run the device-library read (recovery action on the error state). */
  onRetry: () => void;
}

/**
 * Device-side library, rendered as a DISTINCT section inside the center
 * column — never merged into the local collection, never in the right
 * decision panel. The section heading + provenance chip keep "appareil"
 * vs "bibliothèque locale" readable at all times (AC1).
 *
 * Each entry is an opaque, "non reconnue" pack identity (the device stores
 * no title; Rustory consults no catalog in the MVP). When `onSelectStory`
 * is wired, each entry becomes single-selectable so the user can inspect it
 * before import (see ui-states.md#Device Story Inspection Contract); the
 * import flow itself is a later story.
 *
 * States map 1-to-1 to the hook:
 * - `idle`    → nothing to show (no readable device): render nothing.
 * - `loading` → calm in-context progress ("état non encore chargé").
 * - `ready`   → the list, or a distinct "aucune histoire" empty state.
 * - `error`   → recoverable in-context banner with a retry (never a toast,
 *               and the LOCAL library — a separate hook — stays intact).
 */
export function DeviceStoryCollection({
  state,
  isRefreshing,
  deviceLabel,
  selectedUuid,
  onSelectStory,
  onRetry,
}: DeviceStoryCollectionProps): React.JSX.Element | null {
  const headingRef = useRef<HTMLHeadingElement>(null);
  const focusedCardRef = useRef<HTMLElement | null>(null);

  // Rescue keyboard focus when the focused card is removed from the DOM — e.g.
  // its entry was purged after a re-read that no longer lists it. Without this,
  // focus falls to <body> and the keyboard user loses their place. We keep
  // `focusedCardRef` on a blur with no relatedTarget (the focused node was
  // removed, or focus fell to <body>) precisely so the removal is detectable
  // here; an intentional move to another element clears it (see handleListBlur).
  useLayoutEffect(() => {
    const node = focusedCardRef.current;
    if (node && !document.contains(node) && headingRef.current) {
      focusedCardRef.current = null;
      headingRef.current.focus();
    }
  });

  if (state.kind === "idle") {
    return null;
  }

  const handleListFocus = (event: React.FocusEvent<HTMLUListElement>): void => {
    focusedCardRef.current = event.target as HTMLElement;
  };
  const handleListBlur = (event: React.FocusEvent<HTMLUListElement>): void => {
    // Clear only on an intentional move to another real element; a blur with no
    // relatedTarget means the focused card was removed (or focus fell to
    // <body>) — keep the ref so the layout effect can rescue the focus.
    if (event.relatedTarget) {
      focusedCardRef.current = null;
    }
  };

  const heading = deviceLabel
    ? `Histoires sur l'appareil — ${deviceLabel}`
    : "Histoires sur l'appareil";

  return (
    <section
      className="device-story-collection"
      aria-label="Bibliothèque de l'appareil"
    >
      <header className="device-story-collection__header">
        <h2
          className="device-story-collection__title"
          ref={headingRef}
          tabIndex={-1}
        >
          {heading}
        </h2>
        <StateChip tone="info" label="Sur l'appareil" />
      </header>

      {state.kind === "loading" ? (
        <div
          className="device-story-collection__pending"
          role="status"
          aria-live="polite"
        >
          <ProgressIndicator
            mode="indeterminate"
            label="Lecture de la bibliothèque de l'appareil…"
          />
        </div>
      ) : null}

      {state.kind === "error" ? (
        <LibraryErrorBanner
          error={state.error}
          onRetry={onRetry}
          title="Bibliothèque de l'appareil indisponible"
        />
      ) : null}

      {state.kind === "ready" && state.stories.length === 0 ? (
        <section
          className="device-story-collection__empty"
          role="status"
          aria-live="polite"
        >
          <h3 className="device-story-collection__empty-title">
            Aucune histoire sur l'appareil
          </h3>
          <p className="device-story-collection__empty-hint">
            L'appareil connecté ne contient aucune histoire lisible.
          </p>
        </section>
      ) : null}

      {state.kind === "ready" && state.stories.length > 0 ? (
        <>
          <p
            className="device-story-collection__counter"
            role="status"
            aria-live="polite"
          >
            {isRefreshing
              ? "Actualisation…"
              : countLabel(state.stories.length)}
          </p>
          <ul
            className="device-story-collection__list"
            onFocus={handleListFocus}
            onBlur={handleListBlur}
          >
            {state.stories.map((story, index) => (
              <li
                key={`${story.uuid}::${index}`}
                className="device-story-collection__item"
              >
                <DeviceStoryCard
                  story={story}
                  isSelected={
                    onSelectStory !== undefined && story.uuid === selectedUuid
                  }
                  onSelect={onSelectStory}
                />
              </li>
            ))}
          </ul>
        </>
      ) : null}
    </section>
  );
}

interface DeviceStoryCardProps {
  story: DeviceStoryDto;
  isSelected: boolean;
  /** When provided, the card is an interactive single-selection control;
   *  when omitted, it renders as a static, listing-only entry. */
  onSelect?: (uuid: string) => void;
}

/**
 * One device-story entry. When a local index recognizes the pack, the card
 * shows the real title + a provenance chip (officiel / non-officiel / saisi);
 * otherwise it falls back to "Histoire non reconnue" — reserved for genuinely
 * unknown packs (AC1). With `onSelect`, the card is a `role="button"
 * aria-pressed` focus stop whose activation (click, Space or Enter) toggles
 * its selection, signaled redundantly (border + visible `✓` prefix +
 * `aria-pressed`) so it survives grayscale and color-blindness. There is no
 * open/edit affordance here; naming happens in the inspector.
 */
function DeviceStoryCard({
  story,
  isSelected,
  onSelect,
}: DeviceStoryCardProps): React.JSX.Element {
  const interactive = onSelect !== undefined;

  // Reserve "Histoire non reconnue" for genuinely unknown packs: a resolved
  // title is shown verbatim with its provenance chip.
  const recognized = story.title !== null;
  const displayTitle = recognized ? story.title : "Histoire non reconnue";
  const provenance = story.titleSource
    ? titleProvenanceChip(story.titleSource)
    : null;
  const nameForAria =
    recognized && story.titleSource
      ? `${story.title}, ${titleProvenancePhrase(story.titleSource)}`
      : "Histoire non reconnue";
  // Cover from the LOCAL cache only (no network). Decorative — the title
  // carries the accessible name, so the image is aria-hidden.
  const coverUrl = usePackCover(story.uuid, story.thumbnail !== null);

  const handleClick = (): void => {
    onSelect?.(story.uuid);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>): void => {
    // Swallow OS-level key auto-repeat so a held Space / Enter does not
    // flicker the selection.
    if (event.repeat) {
      if (
        event.key === " " ||
        event.key === "Spacebar" ||
        event.key === "Enter"
      ) {
        event.preventDefault();
      }
      return;
    }
    if (
      event.key === " " ||
      event.key === "Spacebar" ||
      event.key === "Enter"
    ) {
      event.preventDefault();
      onSelect?.(story.uuid);
    }
  };

  const focusableClass = [
    "device-story-card__focusable",
    isSelected ? "device-story-card__focusable--selected" : null,
  ]
    .filter(Boolean)
    .join(" ");

  // Fold the structural flags into the accessible name: a screen-reader user
  // navigating by buttons must also hear "masquée" / "contenu incomplet" /
  // "dans ta bibliothèque" — an explicit aria-label otherwise shadows the
  // chip text inside the card.
  const flagText = [
    story.alreadyImported ? "dans ta bibliothèque" : null,
    story.hidden ? "masquée" : null,
    !story.contentPresent ? "contenu incomplet" : null,
  ]
    .filter(Boolean)
    .join(", ");
  const ariaLabel = flagText
    ? `${nameForAria}, identifiant ${story.shortId}, ${flagText}`
    : `${nameForAria}, identifiant ${story.shortId}`;

  const interactiveProps = interactive
    ? {
        role: "button" as const,
        tabIndex: 0,
        "aria-pressed": isSelected,
        "aria-label": ariaLabel,
        onClick: handleClick,
        onKeyDown: handleKeyDown,
      }
    : {};

  return (
    <SurfacePanel elevation={1} as="article" className="device-story-card">
      <div className={focusableClass} {...interactiveProps}>
        {isSelected ? (
          <span className="device-story-card__marker" aria-hidden="true">
            ✓
          </span>
        ) : null}
        {coverUrl ? (
          <img
            className="device-story-card__cover"
            src={coverUrl}
            alt=""
            aria-hidden="true"
          />
        ) : null}
        <h3 className="device-story-card__title">{displayTitle}</h3>
        <p className="device-story-card__id">
          <span className="device-story-card__id-label">
            Identifiant&nbsp;:
          </span>{" "}
          <code className="device-story-card__id-value">{story.shortId}</code>
        </p>
        <div className="device-story-card__flags">
          {provenance ? (
            <StateChip tone={provenance.tone} label={provenance.label} />
          ) : null}
          {story.alreadyImported ? (
            <StateChip tone="success" label="Dans ta bibliothèque" />
          ) : null}
          {story.hidden ? <StateChip tone="neutral" label="Masquée" /> : null}
          {!story.contentPresent ? (
            <StateChip tone="warning" label="Contenu incomplet" />
          ) : null}
        </div>
      </div>
    </SurfacePanel>
  );
}

function countLabel(count: number): string {
  return `${count} histoire${count > 1 ? "s" : ""} sur l'appareil`;
}
