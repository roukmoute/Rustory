import type React from "react";

import type { DeviceLibraryState } from "../../device/hooks/use-device-library";
import type { AppError } from "../../../shared/errors/app-error";
import type { SupportedFamilyDto } from "../../../shared/ipc-contracts/device";
import { Button, ProgressIndicator } from "../../../shared/ui";
import { OnboardingDeviceIllustration, OnboardingLayout } from "./OnboardingLayout";

import "./DeviceDiscoveryScreen.css";

export interface DeviceDiscoveryScreenProps {
  state: DeviceLibraryState;
  deviceLabel?: string;
  deviceFamily?: SupportedFamilyDto;
  /** The real capability refusal, supplied by the route. A readable V3
   * still shows its inventory count, without offering a copy. */
  importUnavailableReason?: string;
  onRetry: () => void;
  onRecover: () => void;
  onContinue: () => void;
  busy?: boolean;
  error?: AppError | null;
  onRetryPreferences?: () => void;
  onOpenLibrary?: () => void;
}

export function DeviceDiscoveryScreen({
  state,
  deviceLabel,
  deviceFamily,
  importUnavailableReason,
  onRetry,
  onRecover,
  onContinue,
  busy = false,
  error = null,
  onRetryPreferences,
  onOpenLibrary,
}: DeviceDiscoveryScreenProps): React.JSX.Element {
  const loading = state.kind === "loading";
  const found = state.kind === "ready" && state.stories.length > 0;
  const empty = state.kind === "ready" && state.stories.length === 0;
  const readError = state.kind === "error" ? state.error : null;
  const count = state.kind === "ready" ? state.stories.length : 0;
  const eligibleCount = state.kind === "ready"
    ? state.stories.filter((story) => story.contentPresent && !story.alreadyImported).length
    : 0;
  const allAlreadyImported = state.kind === "ready" && state.stories.every((story) => story.alreadyImported);
  const canRecover = !importUnavailableReason && eligibleCount > 0;
  const unavailable = state.kind === "idle";
  const isFlam = deviceFamily === "flam" || deviceLabel?.toLowerCase().includes("flam");
  const isLunii = deviceFamily === "lunii" || (!deviceFamily && (!deviceLabel || deviceLabel.toLowerCase().includes("lunii")));
  const deviceNoun = isFlam ? "ton FLAM" : isLunii ? "ta Lunii" : "ton appareil";
  const deviceSubject = isFlam ? "Ton FLAM" : isLunii ? "Ta Lunii" : "Ton appareil";

  const title = loading
    ? `Je regarde ce qu'il y a sur ${deviceNoun}`
    : found
      ? `${count} histoire${count > 1 ? "s trouvées" : " trouvée"} sur ${deviceNoun}`
      : empty
        ? `${deviceSubject} ne contient aucune histoire`
        : readError
          ? `Je n'ai pas pu lire ${deviceNoun}`
          : "La récupération n'est pas disponible";

  return (
    <OnboardingLayout step={loading || readError ? 2 : 3} deviceStepLabel={deviceSubject}>
      <div className="onboarding-discovery">
        <p className="onboarding-discovery__device">
          <span aria-hidden="true" />{deviceLabel ?? (isFlam ? "FLAM connecté" : isLunii ? "Lunii connectée" : "Appareil connecté")}
        </p>
        <div className="onboarding-discovery__body">
          <div className="onboarding-discovery__content">
            <h1 className="onboarding__title">{title}</h1>
            {loading && (
              <>
                <p className="onboarding__lead">Ça prend un petit moment. Ne {isLunii ? "la" : "le"} débranche pas.</p>
                <div className="onboarding-discovery__progress" role="status">
                  <ProgressIndicator mode="indeterminate" label={`Lecture de la bibliothèque de ${deviceNoun}…`} />
                </div>
                <p className="onboarding__note">Rien n'est modifié sur {deviceNoun}.</p>
              </>
            )}
            {found && (
              <>
                <p className="onboarding__lead">
                  {importUnavailableReason ?? (canRecover
                    ? `Veux-tu les ajouter à ta bibliothèque Rustory ? Elles restent sur ${deviceNoun}, rien n'est supprimé.`
                    : allAlreadyImported
                      ? "Toutes ces histoires sont déjà dans ta bibliothèque Rustory."
                      : "Aucune de ces histoires ne peut être récupérée pour l'instant : leur contenu est absent ou elles sont déjà dans ta bibliothèque.")}
                </p>
                <div className="onboarding__actions">
                  {canRecover ? (
                    <>
                      <Button variant="primary" onClick={onRecover} aria-disabled={busy || undefined}>
                        Récupérer mes histoires
                      </Button>
                      <Button variant="secondary" onClick={onContinue} aria-disabled={busy || undefined}>
                        Pas maintenant
                      </Button>
                    </>
                  ) : (
                    <Button variant="primary" onClick={onContinue} aria-disabled={busy || undefined}>
                      Continuer vers ma bibliothèque
                    </Button>
                  )}
                </div>
                {canRecover && (
                  <p className="onboarding__note">
                    {eligibleCount < count && `${eligibleCount} histoire${eligibleCount > 1 ? "s" : ""} à récupérer ; les histoires déjà copiées ou sans contenu sont laissées de côté. `}
                    Tu pourras les récupérer plus tard depuis Sur l'appareil.
                  </p>
                )}
              </>
            )}
            {empty && (
              <>
                <p className="onboarding__lead">
                  {importUnavailableReason ?? "Crée ta première histoire dans Rustory pour préparer de nouvelles aventures."}
                </p>
                <div className="onboarding__actions">
                  <Button variant="primary" onClick={onContinue} aria-disabled={busy || undefined}>Continuer</Button>
                </div>
              </>
            )}
            {readError && (
              <>
                <p className="onboarding__error" role="alert">
                  {readError.message}{readError.userAction ? ` ${readError.userAction}` : ""}
                </p>
                <div className="onboarding__actions">
                  <Button variant="primary" onClick={onRetry} aria-disabled={busy || undefined}>Réessayer</Button>
                  <Button variant="secondary" onClick={onContinue} aria-disabled={busy || undefined}>Pas maintenant</Button>
                </div>
              </>
            )}
            {unavailable && (
              <>
                <p className="onboarding__lead">
                  {importUnavailableReason ?? "La bibliothèque de cet appareil n'est pas disponible. Réessaie la lecture ou continue vers ta bibliothèque Rustory."}
                </p>
                <div className="onboarding__actions">
                  <Button variant="primary" onClick={onContinue} aria-disabled={busy || undefined}>Continuer</Button>
                  {!importUnavailableReason && (
                    <Button variant="secondary" onClick={onRetry} aria-disabled={busy || undefined}>Réessayer</Button>
                  )}
                </div>
              </>
            )}
            {error && (
              <div className="onboarding__error" role="alert">
                <p>{error.message}{error.userAction ? ` ${error.userAction}` : ""}</p>
                {onRetryPreferences && <Button variant="secondary" onClick={onRetryPreferences} aria-disabled={busy || undefined}>Réessayer l'accueil</Button>}
                {onOpenLibrary && <Button variant="quiet" onClick={onOpenLibrary}>Ouvrir ma bibliothèque</Button>}
              </div>
            )}
          </div>
          <OnboardingDeviceIllustration />
        </div>
      </div>
    </OnboardingLayout>
  );
}
