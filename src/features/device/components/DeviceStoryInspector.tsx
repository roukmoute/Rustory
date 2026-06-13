import type React from "react";
import { useId } from "react";

import type { SupportedOperationsDto } from "../../../shared/ipc-contracts/device";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";
import { Button, StateChip, SurfacePanel } from "../../../shared/ui";

import { DeviceImportStatusSurface } from "./DeviceImportStatusSurface";
import type { DeviceStoryImportStatus } from "../hooks/use-device-story-import";

import "./DeviceStoryInspector.css";

const IDLE_IMPORT_STATUS: DeviceStoryImportStatus = { kind: "idle" };

export interface DeviceStoryInspectorProps {
  /** The device story currently selected for inspection, or null when none
   *  is. When null the inspector renders nothing. */
  story: DeviceStoryDto | null;
  /** Authoritative per-profile operation matrix of the connected device,
   *  used to gate and phrase the copy affordance honestly. */
  supportedOperations?: SupportedOperationsDto;
  /** Current state of the copy flow (owned by `useDeviceStoryImport` at
   *  the route level). Defaults to idle when the route does not wire the
   *  import (listing/inspection-only contexts). */
  importState?: DeviceStoryImportStatus;
  /** Start the copy of the inspected story. Wired by the route ONLY when
   *  the capability gate allows it (`importStory === true`); when absent
   *  the CTA stays soft-disabled with a standardized reason. */
  onImport?: (story: DeviceStoryDto) => void;
  /** Re-fire the copy from a failed state (the alert's `Réessayer`). */
  onRetryImport?: () => void;
  /** Dismiss the import status surface (success `Fermer`). */
  onDismissImportStatus?: () => void;
}

/**
 * Right-column contextual inspector for the selected device story. Shows
 * only the verified facts already carried by the inventory snapshot (no
 * title, no cover, no asserted content quality — the device stores none
 * and the offline MVP consults no catalog). It makes the provenance
 * explicit ("lives on the device, not yet local") and surfaces
 * ambiguities before any copy.
 *
 * The `Copier dans ma bibliothèque` affordance (device → local library)
 * is ACTIVE when the authoritative matrix allows the copy, the payload is
 * present on the device and no local copy exists yet; otherwise it stays
 * soft-disabled with a standardized, fail-closed reason. The verb is
 * `Copier`, not `Importer`: Importer/Exporter are reserved for local file
 * artifacts (see product-language.md). The internal capability flag stays
 * `importStory`.
 *
 * All copy feedback renders in-context below the CTA (polite success,
 * alert failure with retry) — never a toast, never a modal.
 */
export function DeviceStoryInspector({
  story,
  supportedOperations,
  importState,
  onImport,
  onRetryImport,
  onDismissImportStatus,
}: DeviceStoryInspectorProps): React.JSX.Element | null {
  const titleId = useId();
  const copyReasonId = useId();

  if (!story) {
    return null;
  }

  const status = importState ?? IDLE_IMPORT_STATUS;
  const isImporting = status.kind === "importing";
  // A just-succeeded copy keeps the CTA soft-disabled until the device
  // re-read lands `alreadyImported=true`: in that window the snapshot
  // still says `alreadyImported=false` and `inFlightRef` is already
  // cleared, so a re-click would relaunch the copy and Rust would turn
  // the success surface into an `already_imported` alert.
  const isImported = status.kind === "imported";
  const canImport =
    supportedOperations?.importStory === true &&
    story.contentPresent &&
    !story.alreadyImported &&
    onImport !== undefined;
  const isSoftDisabled = !canImport || isImporting || isImported;
  const copyReason = canImport ? null : formatCopyReason(supportedOperations, story);

  const handleImportClick = (): void => {
    if (isSoftDisabled) return;
    onImport?.(story);
  };

  return (
    <SurfacePanel
      elevation={1}
      as="section"
      ariaLabelledBy={titleId}
      className="device-inspector"
    >
      <h2 id={titleId} className="device-inspector__title">
        Histoire sélectionnée
      </h2>

      <div className="device-inspector__provenance">
        <StateChip tone="info" label="Sur l'appareil" />
        <p className="device-inspector__provenance-note">
          {story.alreadyImported
            ? "Cette histoire vit sur l'appareil et une copie existe déjà dans ta bibliothèque locale."
            : "Cette histoire vit sur l'appareil, pas encore dans ta bibliothèque locale."}
        </p>
      </div>

      <h3 className="device-inspector__name">Histoire non reconnue</h3>

      <dl className="device-inspector__facts">
        <div className="device-inspector__fact">
          <dt className="device-inspector__fact-label">Identifiant</dt>
          <dd className="device-inspector__fact-value">
            <code>{story.shortId}</code>
          </dd>
        </div>
        <div className="device-inspector__fact">
          <dt className="device-inspector__fact-label">UUID</dt>
          <dd className="device-inspector__fact-value">
            <code>{story.uuid}</code>
          </dd>
        </div>
      </dl>

      {story.hidden || !story.contentPresent || story.alreadyImported ? (
        <div className="device-inspector__flags">
          {story.alreadyImported ? (
            <StateChip tone="success" label="Dans ta bibliothèque" />
          ) : null}
          {story.hidden ? <StateChip tone="neutral" label="Masquée" /> : null}
          {!story.contentPresent ? (
            <StateChip tone="warning" label="Contenu incomplet" />
          ) : null}
        </div>
      ) : null}

      {!story.contentPresent ? (
        <p className="device-inspector__note">
          Le dossier de contenu de cette histoire est introuvable sur
          l'appareil. Vérifie l'appareil avant de la copier.
        </p>
      ) : null}
      {story.hidden ? (
        <p className="device-inspector__note">
          Cette histoire est marquée comme masquée sur l'appareil.
        </p>
      ) : null}

      <Button
        aria-disabled={isSoftDisabled || undefined}
        aria-busy={isImporting || undefined}
        aria-describedby={copyReason ? copyReasonId : undefined}
        onClick={handleImportClick}
      >
        Copier dans ma bibliothèque
      </Button>
      {copyReason ? (
        <p id={copyReasonId} className="device-inspector__reason">
          {copyReason}
        </p>
      ) : null}

      <DeviceImportStatusSurface
        status={status}
        onRetry={() => onRetryImport?.()}
        onDismiss={() => onDismissImportStatus?.()}
      />
    </SurfacePanel>
  );
}

/**
 * Standardized, fail-closed disabled reason, in the priority order locked
 * by ui-states.md#Device Story Inspection Contract:
 * 1. a local copy already exists — the most useful fact: no copy needed;
 * 2. the profile does not POSITIVELY allow the copy (ops absent or
 *    `importStory !== true`, V3 included) — the fail-closed default;
 * 3. the payload folder is missing on the device.
 */
function formatCopyReason(
  ops: SupportedOperationsDto | undefined,
  story: DeviceStoryDto,
): string {
  if (story.alreadyImported) {
    return "Copie indisponible: déjà dans ta bibliothèque";
  }
  if (ops?.importStory !== true) {
    return "Copie indisponible: profil non supporté";
  }
  if (!story.contentPresent) {
    return "Copie indisponible: contenu incomplet sur l'appareil";
  }
  // `canImport` was false only because the route did not wire `onImport`
  // (inspection-only context). The honest cause is the profile gate not
  // being engaged — fail closed rather than invent a new wording.
  return "Copie indisponible: profil non supporté";
}
