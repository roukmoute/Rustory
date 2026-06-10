import type React from "react";
import { useId } from "react";

import type { SupportedOperationsDto } from "../../../shared/ipc-contracts/device";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";
import { Button, StateChip, SurfacePanel } from "../../../shared/ui";

import "./DeviceStoryInspector.css";

export interface DeviceStoryInspectorProps {
  /** The device story currently selected for inspection, or null when none
   *  is. When null the inspector renders nothing. */
  story: DeviceStoryDto | null;
  /** Authoritative per-profile operation matrix of the connected device,
   *  used to phrase the (still disabled) copy affordance honestly. */
  supportedOperations?: SupportedOperationsDto;
}

/**
 * Right-column contextual inspector for the selected device story. Read-only:
 * it shows only the verified facts already carried by the inventory snapshot
 * (no title, no cover, no asserted content quality — the device stores none
 * and the offline MVP consults no catalog). It makes the provenance explicit
 * ("lives on the device, not yet local" — AC1) and surfaces ambiguities
 * before any copy (AC2). The "Copier dans ma bibliothèque" affordance
 * (device → local library) is present but disabled here; the copy flow lands
 * in a later story. The verb is `Copier`, not `Importer`: Importer/Exporter
 * are reserved for local file artifacts (see product-language.md). The
 * internal capability flag stays `importStory`.
 */
export function DeviceStoryInspector({
  story,
  supportedOperations,
}: DeviceStoryInspectorProps): React.JSX.Element | null {
  const titleId = useId();
  const copyReasonId = useId();

  if (!story) {
    return null;
  }

  const copyReason = formatCopyReason(supportedOperations);

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
          Cette histoire vit sur l'appareil, pas encore dans ta bibliothèque
          locale.
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

      {story.hidden || !story.contentPresent ? (
        <div className="device-inspector__flags">
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

      <Button aria-disabled="true" aria-describedby={copyReasonId}>
        Copier dans ma bibliothèque
      </Button>
      <p id={copyReasonId} className="device-inspector__reason">
        {copyReason}
      </p>
    </SurfacePanel>
  );
}

function formatCopyReason(ops?: SupportedOperationsDto): string {
  // Fail-closed and capability-aware, drawn from the closed reason set in
  // ui-states.md#Disabled Actions and Reasons. Only claim "not wired yet"
  // when we POSITIVELY know the profile allows the copy; an unknown matrix
  // (ops absent) or a disallowed operation (V3) must read as "profil non
  // supporté" rather than optimistically implying support.
  if (ops?.importStory === true) {
    return "Copie indisponible: pas encore activée (MVP Phase 1)";
  }
  return "Copie indisponible: profil non supporté";
}
