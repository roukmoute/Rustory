import type React from "react";

import { LibraryErrorBanner } from "../../library/components/LibraryErrorBanner";
import { ProgressIndicator, StateChip, SurfacePanel } from "../../../shared/ui";
import type { DeviceLibraryState } from "../hooks/use-device-library";

import "./DeviceStoryCollection.css";

export interface DeviceStoryCollectionProps {
  state: DeviceLibraryState;
  /** True while a re-read runs on top of an already-displayed snapshot. */
  isRefreshing: boolean;
  /** Human label of the connected device (e.g. "Lunii V3"), shown in the
   *  section heading so the provenance is explicit. */
  deviceLabel?: string;
  /** Re-run the device-library read (recovery action on the error state). */
  onRetry: () => void;
}

/**
 * Device-side library, rendered as a DISTINCT section inside the center
 * column — never merged into the local collection, never in the right
 * decision panel. The section heading + provenance chip keep "appareil"
 * vs "bibliothèque locale" readable at all times (AC1).
 *
 * Scope: listing only. Each entry is an opaque, "non reconnue" pack
 * identity (the device stores no title; Rustory consults no catalog in
 * the MVP). Device entries are NOT selectable/editable here — inspection
 * and import are later flows.
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
  onRetry,
}: DeviceStoryCollectionProps): React.JSX.Element | null {
  if (state.kind === "idle") {
    return null;
  }

  const heading = deviceLabel
    ? `Histoires sur l'appareil — ${deviceLabel}`
    : "Histoires sur l'appareil";

  return (
    <section
      className="device-story-collection"
      aria-label="Bibliothèque de l'appareil"
    >
      <header className="device-story-collection__header">
        <h2 className="device-story-collection__title">{heading}</h2>
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
            La Lunii connectée ne contient aucune histoire lisible.
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
          <ul className="device-story-collection__list">
            {state.stories.map((story, index) => (
              <li
                key={`${story.uuid}::${index}`}
                className="device-story-collection__item"
              >
                <SurfacePanel
                  elevation={1}
                  as="article"
                  className="device-story-card"
                >
                  <h3 className="device-story-card__title">
                    Histoire non reconnue
                  </h3>
                  <p className="device-story-card__id">
                    <span className="device-story-card__id-label">
                      Identifiant&nbsp;:
                    </span>{" "}
                    <code className="device-story-card__id-value">
                      {story.shortId}
                    </code>
                  </p>
                  <div className="device-story-card__flags">
                    {story.hidden ? (
                      <StateChip tone="neutral" label="Masquée" />
                    ) : null}
                    {!story.contentPresent ? (
                      <StateChip tone="warning" label="Contenu incomplet" />
                    ) : null}
                  </div>
                </SurfacePanel>
              </li>
            ))}
          </ul>
        </>
      ) : null}
    </section>
  );
}

function countLabel(count: number): string {
  return `${count} histoire${count > 1 ? "s" : ""} sur l'appareil`;
}
