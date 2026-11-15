import type React from "react";
import { useId } from "react";

import type { LuniiDeviceState } from "../../library/components/LuniiDecisionPanel";
import { Button, Icon, ProgressIndicator } from "../../../shared/ui";
import {
  DeviceStoryCollection,
  type DeviceStoryCollectionProps,
} from "./DeviceStoryCollection";

import "./DeviceScreen.css";

export interface DeviceScreenProps {
  /** Existing route projection of the authoritative detection result. */
  deviceState: LuniiDeviceState;
  deviceLabel?: string;
  deviceReason?: string;
  onRetryDetection: () => void;
  onConsultSupportProfile: () => void;
  /** Keep inventory, selection and device actions owned by the route. */
  collectionProps: DeviceStoryCollectionProps;
}

function unavailableTitle(state: LuniiDeviceState): string {
  switch (state) {
    case "absent":
      return "Aucun appareil connecté";
    case "unsupported":
      return "Profil non supporté";
    case "ambiguous":
      return "Profil ambigu";
    case "error":
      return "Détection indisponible";
    case "scanning":
      return "Détection en cours…";
    case "idle":
      return "Bibliothèque de l'appareil indisponible";
  }
}

function unavailableHint(state: LuniiDeviceState): string {
  switch (state) {
    case "absent":
      return "Branche ton appareil à cet ordinateur pour consulter les histoires qu'il contient.";
    case "unsupported":
      return "Consulte le profil de support pour connaître les appareils et les opérations pris en charge.";
    case "ambiguous":
      return "Plusieurs appareils ont été détectés. Débranche les autres puis réessaie la détection.";
    case "error":
      return "Vérifie que l'appareil est branché puis réessaie la détection.";
    case "idle":
      // A detected device with no inventory is not an empty inventory:
      // its read capability may be closed, or a live re-read saw a change.
      return "La bibliothèque de cet appareil n'est pas disponible. Réessaie la détection ou consulte le profil de support.";
    case "scanning":
      return "";
  }
}

/** Device-only screen. Detection and inventory remain separate truths:
 * a refresh can retain an inventory, and an absent device is never shown
 * as an empty collection. No fetching or mutation is introduced here. */
export function DeviceScreen({
  deviceState,
  deviceLabel,
  deviceReason,
  onRetryDetection,
  onConsultSupportProfile,
  collectionProps,
}: DeviceScreenProps): React.JSX.Element {
  const titleId = useId();
  const statusId = useId();
  const hasInventoryState = collectionProps.state.kind !== "idle";
  const scanning = deviceState === "scanning";
  const showDetection = deviceState !== "idle" || !hasInventoryState;
  const showSupport = !scanning && deviceState !== "absent";

  return (
    <section className="device-screen" aria-labelledby={titleId}>
      <header className="device-screen__header">
        <div className="device-screen__heading">
          <p className="device-screen__eyebrow">
            {deviceLabel ?? "Les histoires de ton appareil"}
          </p>
          <h1 id={titleId} className="device-screen__title">Sur l'appareil</h1>
        </div>
        <Button
          variant="secondary"
          onClick={onRetryDetection}
          aria-label="Réessayer la détection de l'appareil"
          aria-disabled={scanning || undefined}
          aria-describedby={scanning ? statusId : undefined}
        >
          <Icon name="device" />
          Réessayer la détection
        </Button>
      </header>

      {showDetection && (
        <div
          id={statusId}
          className={`device-screen__status${hasInventoryState ? " device-screen__status--compact" : ""}`}
          role={deviceState === "error" ? "alert" : "status"}
        >
          <span className="device-screen__illustration" aria-hidden="true">
            <Icon name="device" />
          </span>
          <div className="device-screen__status-content">
            {scanning ? (
              <ProgressIndicator mode="indeterminate" label="Détection en cours…" />
            ) : (
              <>
                <h2 className="device-screen__status-title">
                  {unavailableTitle(deviceState)}
                </h2>
                <p className="device-screen__hint">
                  {deviceReason ?? unavailableHint(deviceState)}
                </p>
                {showSupport && (
                  <Button variant="quiet" onClick={onConsultSupportProfile}>
                    <Icon name="settings" />
                    Consulter le profil de support
                  </Button>
                )}
              </>
            )}
          </div>
        </div>
      )}

      <DeviceStoryCollection
        {...collectionProps}
        deviceLabel={collectionProps.deviceLabel ?? deviceLabel}
      />
    </section>
  );
}
