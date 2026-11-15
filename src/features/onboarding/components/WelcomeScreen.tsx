import type React from "react";

import type { LuniiDeviceState } from "../../library/components/LuniiDecisionPanel";
import { AnnouncementVoiceSettings } from "../../settings/components/AnnouncementVoiceSettings";
import type { AppError } from "../../../shared/errors/app-error";
import type { SupportedFamilyDto } from "../../../shared/ipc-contracts/device";
import { Button } from "../../../shared/ui";
import { OnboardingLayout } from "./OnboardingLayout";

import "./WelcomeScreen.css";

export interface WelcomeScreenProps {
  deviceState: LuniiDeviceState;
  deviceLabel?: string;
  deviceFamily?: SupportedFamilyDto;
  deviceReason?: string;
  onContinue: () => void;
  onRetryDetection: () => void;
  busy?: boolean;
  error?: AppError | null;
  onRetryPreferences?: () => void;
  onOpenLibrary?: () => void;
}

function deviceStatus(state: LuniiDeviceState): string {
  switch (state) {
    case "idle": return "Ta Lunii est reconnue";
    case "absent": return "En attente de ta Lunii…";
    case "scanning": return "Recherche de ta Lunii…";
    case "unsupported": return "Profil non supporté";
    case "ambiguous": return "Profil ambigu";
    case "error": return "Détection indisponible";
  }
}

/** Device state and navigation remain owned by the route. Voice selection
 * reuses the existing settings component and its authoritative Rust values. */
export function WelcomeScreen({
  deviceState,
  deviceLabel,
  deviceFamily,
  deviceReason,
  onContinue,
  onRetryDetection,
  busy = false,
  error = null,
  onRetryPreferences,
  onOpenLibrary,
}: WelcomeScreenProps): React.JSX.Element {
  const connected = deviceState === "idle";
  const scanning = deviceState === "scanning";
  const isFlam = deviceFamily === "flam" || deviceLabel?.toLowerCase().includes("flam");
  const deviceNoun = isFlam ? "ton FLAM" : "ta Lunii";
  const status = isFlam && (connected || deviceState === "absent" || scanning)
    ? connected ? "Ton FLAM est reconnu" : scanning ? "Recherche de ton FLAM…" : "En attente de ton FLAM…"
    : connected && deviceLabel && !deviceLabel.toLowerCase().includes("lunii")
      ? `${deviceLabel} reconnu`
      : deviceStatus(deviceState);

  return (
    <OnboardingLayout step={1} deviceStepLabel={isFlam ? "Ton FLAM" : "Ta Lunii"}>
      <div className="onboarding-welcome">
        <div className="onboarding-welcome__intro">
          <h1 className="onboarding__title">Bienvenue sur Rustory</h1>
          <p className="onboarding__lead">
            Mets tes histoires préférées sur {deviceNoun} en quelques minutes.
            Branche {deviceNoun} et choisis une voix pour commencer.
          </p>
        </div>
        <div className="onboarding-welcome__setup">
          <AnnouncementVoiceSettings variant="welcome" />
          <div className="onboarding-welcome__connection">
            <p
              className={`onboarding-welcome__device${connected ? " onboarding-welcome__device--connected" : ""}`}
              role={deviceState === "error" ? "alert" : "status"}
            >
              <span aria-hidden="true" />{status}
            </p>
            <p className="onboarding__note">
              {deviceReason ?? (connected
                ? "Rustory va regarder ce qu'il y a dessus."
                : isFlam ? "Allume-le et branche le câble USB." : "Allume-la et branche le câble USB.")}
            </p>
            {!connected && (
              <Button
                variant="quiet"
                onClick={onRetryDetection}
                aria-disabled={scanning || busy || undefined}
              >
                Réessayer la détection
              </Button>
            )}
          </div>
          <div className="onboarding__actions onboarding-welcome__continue">
            <Button variant="primary" onClick={onContinue} aria-disabled={busy || undefined}>
              {connected ? "Continuer" : isFlam ? "Continuer sans FLAM" : "Continuer sans Lunii"}
            </Button>
            <p className="onboarding__note">Tu pourras changer de voix dans Aide et réglages.</p>
          </div>
          {error && (
            <div className="onboarding__error" role="alert">
              <p>{error.message}{error.userAction ? ` ${error.userAction}` : ""}</p>
              {onRetryPreferences && <Button variant="secondary" onClick={onRetryPreferences} aria-disabled={busy || undefined}>Réessayer l'accueil</Button>}
              {onOpenLibrary && <Button variant="quiet" onClick={onOpenLibrary}>Ouvrir ma bibliothèque</Button>}
            </div>
          )}
        </div>
      </div>
    </OnboardingLayout>
  );
}
