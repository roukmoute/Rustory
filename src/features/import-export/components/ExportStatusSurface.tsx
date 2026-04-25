import type React from "react";

import { Button, StateChip } from "../../../shared/ui";
import type { ExportStatus } from "../hooks/use-story-export";

import "./ExportStatusSurface.css";

export interface ExportStatusSurfaceProps {
  status: ExportStatus;
  onRetry: () => void;
  onDismiss: () => void;
}

/**
 * Visual surface that mirrors the `ExportStatus` state machine.
 *
 * - `idle`: no content, but the `aria-live="polite"` region stays
 *   mounted (with an empty string) so a subsequent `exported`
 *   transition is announced — a region mounted lazily is ignored by
 *   some assistive tech.
 * - `exporting`: neutral chip; deliberately NOT announced (ephemeral
 *   noise).
 * - `exported`: success chip + destination path, both inside the
 *   persistent polite region.
 * - `failed`: `role="alert"` container with canonical `message`,
 *   `userAction`, and two buttons (retry, dismiss).
 */
export function ExportStatusSurface({
  status,
  onRetry,
  onDismiss,
}: ExportStatusSurfaceProps): React.JSX.Element {
  return (
    <div className="export-status-surface">
      {/* Polite region mounted in ALL states with an atomic update
          so screen readers consistently pick up success announcements
          — mounting it lazily on transition causes some AT to miss
          the first change. The content is empty outside `exported`
          so no noise is emitted. */}
      <div
        className="export-status-surface__live"
        aria-live="polite"
        aria-atomic="true"
      >
        {status.kind === "exported" ? "Exporté" : ""}
      </div>

      {status.kind === "exporting" ? (
        <div className="export-status-surface__chip export-status-surface__chip--exporting">
          <StateChip tone="neutral" label="Exportation en cours…" />
        </div>
      ) : null}

      {status.kind === "exported" ? (
        <div className="export-status-surface__chip export-status-surface__chip--exported">
          <StateChip tone="success" label="Exporté" />
          <p className="export-status-surface__path">
            Exporté vers {status.destinationPath}
          </p>
          <Button variant="quiet" onClick={onDismiss}>
            Fermer
          </Button>
        </div>
      ) : null}

      {status.kind === "failed" ? (
        <div
          className="export-status-surface__alert"
          role="alert"
        >
          <p className="export-status-surface__alert-title">
            Exportation échouée
          </p>
          <p className="export-status-surface__alert-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="export-status-surface__alert-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="export-status-surface__actions">
            <Button variant="secondary" onClick={onRetry}>
              Choisir un autre emplacement
            </Button>
            <Button variant="quiet" onClick={onDismiss}>
              Fermer
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
