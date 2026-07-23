import type React from "react";
import { useId } from "react";

import {
  Button,
  ProgressIndicator,
  StateChip,
  SurfacePanel,
} from "../../../shared/ui";
import type { DevicePackSendStatus } from "../hooks/use-device-pack-send";

import "./DeviceSendPanel.css";

export interface DeviceSendPanelProps {
  /** Settled/in-flight status of the single tracked send. */
  status: DevicePackSendStatus;
  /** Open the native `.zip` picker then send. The route only wires this
   *  when the authoritative capability matrix allows `sendArchive` — the
   *  panel itself is not rendered otherwise. */
  onSend: () => void;
  /** Dismiss a settled status (success and failure alike). */
  onDismissStatus: () => void;
}

/**
 * Device-level "Envoyer un pack (.zip)" affordance. Rendered ONLY when the
 * connected device's capability matrix opens the DEDICATED `sendArchive`
 * operation (Lunii V3) — Rust re-proves that gate before any byte anyway.
 *
 * The native picker is owned by Rust (no path crosses IPC); choosing a file
 * IS the confirmation, so the button fires immediately. The long write has
 * no byte-level progress from the backend — a single awaited IPC call — so
 * the honest affordance is an INDETERMINATE bar, never a fake percentage.
 */
export function DeviceSendPanel({
  status,
  onSend,
  onDismissStatus,
}: DeviceSendPanelProps): React.JSX.Element {
  const titleId = useId();
  const statusId = useId();

  const busy = status.kind === "sending";

  return (
    <SurfacePanel
      elevation={1}
      as="section"
      ariaLabelledBy={titleId}
      className="device-send-panel"
    >
      <h2 id={titleId} className="device-send-panel__title">
        Envoyer un pack
      </h2>

      <p className="device-send-panel__note">
        Choisis une archive de pack (.zip, format STUdio) : elle sera adaptée
        et chiffrée pour cet appareil.
      </p>

      <div
        id={statusId}
        className="device-send-panel__status"
        aria-live="polite"
      >
        {status.kind === "sending" ? (
          <ProgressIndicator
            mode="indeterminate"
            label="Envoi du pack vers l'appareil…"
          />
        ) : status.kind === "sent" ? (
          <div className="device-send-panel__result">
            <StateChip tone="success" label="Pack envoyé" />
            <p className="device-send-panel__result-text">
              {`Pack envoyé sur l'appareil (${status.imageCount} image${
                status.imageCount > 1 ? "s" : ""
              }, ${status.audioCount} audio${
                status.audioCount > 1 ? "s" : ""
              }).`}
            </p>
            <Button variant="quiet" onClick={onDismissStatus}>
              Fermer
            </Button>
          </div>
        ) : null}
      </div>

      <div className="device-send-panel__actions">
        <Button
          variant="secondary"
          aria-disabled={busy || undefined}
          aria-busy={busy || undefined}
          aria-describedby={statusId}
          onClick={() => {
            if (!busy) onSend();
          }}
        >
          Envoyer un pack (.zip)…
        </Button>
      </div>

      {status.kind === "failed" ? (
        <div className="device-send-panel__error" role="alert">
          <StateChip tone="error" label="Envoi impossible" />
          <p className="device-send-panel__error-text">
            {status.error.message}
            {status.error.userAction ? ` ${status.error.userAction}` : ""}
          </p>
          <Button variant="quiet" onClick={onDismissStatus}>
            Fermer
          </Button>
        </div>
      ) : null}
    </SurfacePanel>
  );
}
