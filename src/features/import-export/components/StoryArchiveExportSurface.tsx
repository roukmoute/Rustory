import type React from "react";

import type { AppErrorCode } from "../../../shared/errors/app-error";
import { Button, ProgressIndicator, StateChip } from "../../../shared/ui";
import type { StoryArchiveExportStatus } from "../hooks/use-story-archive-export";

import "./StoryArchiveExportSurface.css";

export interface StoryArchiveExportSurfaceProps {
  status: StoryArchiveExportStatus;
  /** Re-fire the export of the failed story (the save dialog reopens). */
  onRetry: () => void;
  /** Dismiss a settled status back to idle. */
  onDismiss: () => void;
}

/**
 * In-context surface of the story archive export — never a toast, never a
 * modal: the library stays usable while a long story is converted.
 *
 * - `idle`: nothing visible; the polite live region stays mounted (empty)
 *   so the later `exported` announcement is reliably picked up.
 * - `exporting`: the story being exported and a progress bar —
 *   indeterminate until the first tick, then the real percent. Not
 *   announced (ephemeral noise).
 * - `exported`: what was written and where, announced politely; stays
 *   until dismissed (a long export may finish while the user looks away).
 * - `failed`: `role="alert"` with the canonical `message` + `userAction`.
 *   The retry affordance follows the refusal: another destination can fix
 *   a destination problem, nothing but fixing the story fixes a story
 *   problem — that one offers no retry at all.
 */
export function StoryArchiveExportSurface({
  status,
  onRetry,
  onDismiss,
}: StoryArchiveExportSurfaceProps): React.JSX.Element {
  return (
    <div className="story-archive-export">
      <div
        className="story-archive-export__live"
        aria-live="polite"
        aria-atomic="true"
      >
        {status.kind === "exported"
          ? `Archive exportée : ${status.storyTitle}`
          : ""}
      </div>

      {status.kind === "exporting" ? (
        <section
          className="story-archive-export__panel"
          aria-label="Export en archive"
        >
          <p className="story-archive-export__title">
            Export de « {status.storyTitle} » en archive
          </p>
          {status.percent === null ? (
            <ProgressIndicator
              mode="indeterminate"
              label="Exportation en cours…"
            />
          ) : (
            <ProgressIndicator
              mode="determinate"
              label={`Exportation en cours… ${status.percent} %`}
              value={status.percent}
            />
          )}
          <p className="story-archive-export__hint">
            Les images et les sons sont convertis au format de la Lunii. Tu
            peux continuer à utiliser Rustory pendant ce temps.
          </p>
        </section>
      ) : null}

      {status.kind === "exported" ? (
        <section
          className="story-archive-export__panel story-archive-export__panel--exported"
          aria-label="Export en archive"
        >
          <StateChip tone="success" label="Archive exportée" />
          <p className="story-archive-export__title">
            « {status.storyTitle} »
          </p>
          <p className="story-archive-export__summary">
            {describeArchive(
              status.audioCount,
              status.imageCount,
              status.bytesWritten,
            )}
          </p>
          <p className="story-archive-export__path">
            Exportée vers {status.destinationPath}
          </p>
          <p className="story-archive-export__hint">
            Cette archive s'importe telle quelle dans un autre Rustory et dans
            les outils qui lisent ce format de pack.
          </p>
          <div className="story-archive-export__actions">
            <Button variant="quiet" onClick={onDismiss}>
              Fermer
            </Button>
          </div>
        </section>
      ) : null}

      {status.kind === "failed" ? (
        <section
          className="story-archive-export__panel story-archive-export__panel--failed"
          aria-label="Export en archive"
        >
          <div role="alert">
            <p className="story-archive-export__title">Exportation échouée</p>
            <p className="story-archive-export__story">
              « {status.storyTitle} »
            </p>
            <p className="story-archive-export__message">
              {status.error.message}
            </p>
            {status.error.userAction ? (
              <p className="story-archive-export__hint">
                {status.error.userAction}
              </p>
            ) : null}
          </div>
          <div className="story-archive-export__actions">
            {retryLabel(status.error.code) ? (
              <Button variant="secondary" onClick={onRetry}>
                {retryLabel(status.error.code)}
              </Button>
            ) : null}
            <Button variant="quiet" onClick={onDismiss}>
              Fermer
            </Button>
          </div>
        </section>
      ) : null}
    </div>
  );
}

/**
 * The retry affordance a refusal deserves. A destination problem is fixed
 * by picking another destination; a refusal about the story itself (it
 * does not lay out as a pack, a media is gone, it no longer exists) is not
 * fixed by trying again — its `userAction` says what to do instead.
 */
function retryLabel(code: AppErrorCode): string | null {
  switch (code) {
    case "EXPORT_DESTINATION_UNAVAILABLE":
      return "Choisir un autre emplacement";
    case "EXPORT_FAILED":
    case "LIBRARY_INCONSISTENT":
      return null;
    default:
      return "Réessayer";
  }
}

/** What the archive carries, in one line: its sounds, its pictures (when it
 *  has any) and its size. */
function describeArchive(
  audioCount: number,
  imageCount: number,
  bytes: number,
): string {
  const parts = [`${audioCount} ${audioCount > 1 ? "sons" : "son"}`];
  if (imageCount > 0) {
    parts.push(`${imageCount} ${imageCount > 1 ? "images" : "image"}`);
  }
  return `${parts.join(" et ")} · ${formatArchiveSize(bytes)}`;
}

/**
 * A byte count as a parent-friendly size, in the decimal units and French
 * comma the device-space copy already uses: megabytes, promoted to
 * gigabytes (one decimal) from a thousand.
 */
export function formatArchiveSize(bytes: number): string {
  const megabytes = Math.round(bytes / 1_000_000);
  if (megabytes < 1) return "moins de 1 Mo";
  if (megabytes < 1000) return `${megabytes} Mo`;
  return `${(bytes / 1_000_000_000).toFixed(1).replace(".", ",")} Go`;
}
