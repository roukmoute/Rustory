import type React from "react";

import { Button } from "../../../shared/ui";
import type { AppError } from "../../../shared/errors/app-error";

import "./RecoveryBanner.css";

export interface RecoveryReadErrorBannerProps {
  error: AppError;
  /** Re-fire `readRecoverableDraft`. */
  onRetry: () => void;
  /** Best-effort dismiss: the user gives up on recovery and resumes
   *  editing from the persisted state. The hook drops the in-flight
   *  recovery state to `kind: "none"` so the Field re-enables. */
  onDismiss: () => void;
}

/**
 * Variant of the recovery banner that renders when the initial
 * `read_recoverable_draft` IPC call fails. There is no draft payload to
 * display — the user only knows that recovery is not currently available.
 *
 * Per AC3 the user must always receive a useful next action: the banner
 * exposes both `Réessayer la récupération` (re-fetch) and `Conserver
 * l'état enregistré` (give up on recovery, keep editing the persisted
 * value). The Field stays disabled while this surface is visible so the
 * decision is committed before the user resumes typing.
 */
export function RecoveryReadErrorBanner({
  error,
  onRetry,
  onDismiss,
}: RecoveryReadErrorBannerProps): React.JSX.Element {
  return (
    <section
      id="story-edit-recovery-banner"
      role="region"
      aria-label="Récupération indisponible"
      className="ds-surface ds-surface--elevation-1 recovery-banner"
    >
      <h2 className="recovery-banner__title">Récupération indisponible</h2>
      <div className="recovery-banner__alert" role="alert">
        <p className="recovery-banner__alert-message">{error.message}</p>
        {error.userAction ? (
          <p className="recovery-banner__alert-action">{error.userAction}</p>
        ) : null}
      </div>
      <div className="recovery-banner__actions">
        <Button variant="primary" onClick={onRetry}>
          Réessayer la récupération
        </Button>
        <Button variant="secondary" onClick={onDismiss}>
          Conserver l'état enregistré
        </Button>
      </div>
    </section>
  );
}
