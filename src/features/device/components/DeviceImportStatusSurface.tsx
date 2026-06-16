import type React from "react";

import { Button, ProgressIndicator, StateChip } from "../../../shared/ui";
import type { DeviceStoryImportStatus } from "../hooks/use-device-story-import";

import "./DeviceImportStatusSurface.css";

export interface DeviceImportStatusSurfaceProps {
  status: DeviceStoryImportStatus;
  onRetry: () => void;
  onDismiss: () => void;
  /** Open the official device-support profile. When a runtime refusal is
   *  a profile refusal (`DEVICE_UNSUPPORTED` — e.g. the live device turned
   *  out non-importable after a stale snapshot), the alert offers this
   *  next gesture INSTEAD of a futile `Réessayer`, mirroring the pre-click
   *  affordance in the inspector. Omitted ⇒ the button is hidden. */
  onConsultSupportProfile?: () => void;
}

/**
 * Visual surface mirroring the `DeviceStoryImportStatus` state machine,
 * rendered INSIDE the inspector (the copy feedback lives where the action
 * happened — never a toast, never a modal). Structural clone of
 * `ExportStatusSurface`:
 *
 * - `idle`: no content, but the `aria-live="polite"` region stays mounted
 *   (empty) so a later `imported` transition is reliably announced.
 * - `importing`: calm indeterminate progress, deliberately NOT announced.
 * - `imported`: sober success ("Histoire copiée dans ta bibliothèque") +
 *   the created local title + an explicit dismiss. No auto-hide.
 * - `failed`: `role="alert"` with the canonical `message` + `userAction`
 *   and the `Réessayer` button BEFORE `Fermer` in tab order.
 */
export function DeviceImportStatusSurface({
  status,
  onRetry,
  onDismiss,
  onConsultSupportProfile,
}: DeviceImportStatusSurfaceProps): React.JSX.Element {
  // A profile refusal is not retryable (the live device simply does not
  // allow the copy) — offer the support consultation as the next gesture,
  // never a `Réessayer` that would hit the same wall.
  const isProfileRefusal =
    status.kind === "failed" && status.error.code === "DEVICE_UNSUPPORTED";
  const showSupportProfile =
    isProfileRefusal && onConsultSupportProfile !== undefined;

  return (
    <div className="device-import-status">
      {/* Polite region mounted in ALL states with an atomic update so
          screen readers consistently pick up the success announcement —
          a lazily mounted region is ignored by some assistive tech. */}
      <div
        className="device-import-status__live"
        aria-live="polite"
        aria-atomic="true"
      >
        {status.kind === "imported"
          ? "Histoire copiée dans ta bibliothèque"
          : ""}
      </div>

      {status.kind === "importing" ? (
        <div className="device-import-status__pending">
          <ProgressIndicator mode="indeterminate" label="Copie en cours…" />
        </div>
      ) : null}

      {status.kind === "imported" ? (
        <div className="device-import-status__success">
          <StateChip tone="success" label="Histoire copiée dans ta bibliothèque" />
          <p className="device-import-status__success-title">
            {status.story.title}
          </p>
          <Button variant="quiet" onClick={onDismiss}>
            Fermer
          </Button>
        </div>
      ) : null}

      {status.kind === "failed" ? (
        <div className="device-import-status__alert" role="alert">
          <p className="device-import-status__alert-title">Copie impossible</p>
          <p className="device-import-status__alert-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="device-import-status__alert-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="device-import-status__actions">
            {isProfileRefusal ? (
              showSupportProfile ? (
                <Button
                  variant="secondary"
                  onClick={onConsultSupportProfile}
                  aria-label="Consulter le profil de support officiel"
                >
                  Consulter le profil de support
                </Button>
              ) : null
            ) : (
              <Button variant="secondary" onClick={onRetry}>
                Réessayer
              </Button>
            )}
            <Button variant="quiet" onClick={onDismiss}>
              Fermer
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
