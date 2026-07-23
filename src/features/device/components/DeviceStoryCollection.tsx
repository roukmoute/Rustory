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

/** How a selection gesture changes the current set: `replace` collapses it to
 *  the one story (single inspection); `toggle` adds/removes it (multi). Mirror
 *  of the local library's `StoryCardSelectionMode`. */
export type DeviceStorySelectionMode = "replace" | "toggle";

const EMPTY_UUIDS: ReadonlySet<string> = new Set();

export interface DeviceStoryCollectionProps {
  state: DeviceLibraryState;
  /** True while a re-read runs on top of an already-displayed snapshot. */
  isRefreshing: boolean;
  /** Human label of the connected device (e.g. "Lunii V3"), shown in the
   *  section heading so the provenance is explicit. */
  deviceLabel?: string;
  /** UUIDs currently selected. Exactly one → single inspection; several →
   *  the bulk surface. Only meaningful when `onSelectStory` is wired. */
  selectedUuids?: ReadonlySet<string>;
  /** Select a device story. `replace` collapses the selection to this one
   *  (inspection); `toggle` adds/removes it (multi-selection for a bulk
   *  action). When omitted, the cards stay non-interactive — listing only,
   *  the pre-inspection behavior. */
  onSelectStory?: (uuid: string, mode: DeviceStorySelectionMode) => void;
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
 * is wired, each entry becomes selectable: a plain click selects exactly one
 * (single inspection), Ctrl/Cmd+click toggles it into a multi-selection for a
 * bulk action (see ui-states.md#Device Story Inspection Contract). The whole
 * gesture set mirrors the local library so the two collections feel alike.
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
  selectedUuids = EMPTY_UUIDS,
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
              : countLabel(state.stories.length, selectedUuids.size)}
          </p>
          {onSelectStory !== undefined ? (
            <p className="device-story-collection__selection-hint">
              Clique une histoire pour l'inspecter.{" "}
              <kbd className="device-story-collection__kbd">Ctrl</kbd>
              <span aria-hidden="true">+</span>clic (ou{" "}
              <kbd className="device-story-collection__kbd">Cmd</kbd>
              <span aria-hidden="true">+</span>clic sur macOS) pour en
              sélectionner plusieurs et les importer en une fois.
            </p>
          ) : null}
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
                    onSelectStory !== undefined &&
                    selectedUuids.has(story.uuid)
                  }
                  selectionSize={selectedUuids.size}
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
  /** Size of the whole selection, so a plain click can preserve a
   *  multi-selection (never silently collapse it) exactly like the local
   *  library's card. */
  selectionSize: number;
  /** When provided, the card is an interactive selection control; when
   *  omitted, it renders as a static, listing-only entry. */
  onSelect?: (uuid: string, mode: DeviceStorySelectionMode) => void;
}

/**
 * One device-story entry. When a local index recognizes the pack, the card
 * shows the real title + a provenance chip (officiel / non-officiel / saisi);
 * otherwise it falls back to "Histoire non reconnue" — reserved for genuinely
 * unknown packs (AC1). With `onSelect`, the card is a `role="button"
 * aria-pressed` focus stop: plain click / Enter select exactly this one,
 * Ctrl/Cmd+click / Space toggle it into a multi-selection. The state is
 * signaled redundantly (border + visible `✓` prefix + `aria-pressed`) so it
 * survives grayscale and color-blindness. There is no open/edit affordance
 * here; naming happens in the inspector.
 */
function DeviceStoryCard({
  story,
  isSelected,
  selectionSize,
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

  const handleClick = (event: React.MouseEvent<HTMLDivElement>): void => {
    // Shift+click (range selection) is out of scope, exactly like the local
    // library — swallow it so it never falls through to a silent replace.
    if (event.shiftKey) {
      event.preventDefault();
      return;
    }
    if (event.metaKey || event.ctrlKey) {
      onSelect?.(story.uuid, "toggle");
      return;
    }
    // On the already-selected card within a multi-selection, a plain click is
    // a no-op so it never silently collapses the set to this one.
    if (isSelected && selectionSize > 1) return;
    // On the only selected card, a plain click toggles it off — a click should
    // be the inverse of itself without forcing the Ctrl shortcut.
    if (isSelected && selectionSize === 1) {
      onSelect?.(story.uuid, "toggle");
      return;
    }
    onSelect?.(story.uuid, "replace");
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
    // Space toggles this card into/out of the selection (multi); Enter selects
    // exactly this one (single inspection) — the keyboard mirror of plain vs
    // Ctrl+click.
    if (event.key === " " || event.key === "Spacebar") {
      event.preventDefault();
      onSelect?.(story.uuid, "toggle");
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      onSelect?.(story.uuid, "replace");
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

function countLabel(count: number, selectedCount: number): string {
  const base = `${count} histoire${count > 1 ? "s" : ""} sur l'appareil`;
  if (selectedCount === 0) return base;
  const selectedClause =
    selectedCount === 1
      ? " — 1 sélectionnée"
      : ` — ${selectedCount} sélectionnées`;
  return `${base}${selectedClause}`;
}
