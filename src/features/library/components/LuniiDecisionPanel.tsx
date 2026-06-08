import type React from "react";
import { useId } from "react";

import type { SupportedOperationsDto } from "../../../shared/ipc-contracts/device";
import { Button, StateChip, SurfacePanel } from "../../../shared/ui";

import "./LuniiDecisionPanel.css";

export type LuniiDeviceState =
  | "absent"
  | "idle"
  | "unsupported"
  | "ambiguous"
  | "scanning"
  | "error";

export interface LuniiDecisionPanelProps {
  /** Authoritative device state derived from `useConnectedLunii`. */
  deviceState?: LuniiDeviceState;
  /** Friendly label shown when `deviceState === "idle"` (e.g.
   *  "Lunii Origine 2.x"). Optional; falls back to "Appareil prêt". */
  deviceLabel?: string;
  /** Standardized reason copy shown when an action is disabled.
   *  Sourced from `docs/architecture/ui-states.md#Disabled Actions and
   *  Reasons` — never invented at the call site. */
  deviceReason?: string;
  /** Authoritative per-profile operation matrix. Rendered as a small
   *  list under the device chip when the device is `idle` so AC1's
   *  "affiche les opérations officiellement supportées" requirement
   *  is satisfied. Omitted on non-idle states. */
  supportedOperations?: SupportedOperationsDto;
  /** Number of selected stories in the library. Drives the Éditer
   *  CTA's enabled state. */
  selectedCount?: number;
  /** Required when the panel may expose an active Éditer CTA. */
  onEdit: () => void;
  /** Optional refresh trigger — wired by the route to
   *  `useConnectedLunii.refresh`. When omitted, the refresh button is
   *  hidden (used by tests/storybook that do not need the affordance). */
  onRefreshDevice?: () => void;
  /** Optional fallback for the unsupported / ambiguous / error
   *  states. Wired by the route to open
   *  `docs/architecture/device-support-profile.md`. When omitted, the
   *  link is hidden — used by tests that do not need the affordance. */
  onConsultSupportProfile?: () => void;
}

/**
 * Decision surface shown in the library's right column.
 *
 * Layer 1 (selection feedback): a state chip that summarizes how many
 * stories are selected, and an `Éditer` CTA that activates exactly when
 * the selection is a singleton.
 *
 * Layer 2 (device readiness): a state chip + the canonical send CTA
 * (always visible, always disabled in MVP Phase 1 with a typed reason),
 * plus a `Réessayer la détection` action when the route wires
 * `onRefreshDevice`.
 */
export function LuniiDecisionPanel({
  deviceState = "absent",
  deviceLabel,
  deviceReason,
  supportedOperations,
  selectedCount = 0,
  onEdit,
  onRefreshDevice,
  onConsultSupportProfile,
}: LuniiDecisionPanelProps): React.JSX.Element {
  const showSupportProfile =
    onConsultSupportProfile !== undefined &&
    (deviceState === "unsupported" ||
      deviceState === "ambiguous" ||
      deviceState === "error");
  const titleId = useId();
  const deviceReasonId = useId();
  const editReasonId = useId();

  const deviceChipLabel = formatDeviceChipLabel(deviceState, deviceLabel);
  const deviceChipTone = formatDeviceChipTone(deviceState);

  const selectionChipLabel = formatSelectionLabel(selectedCount);
  const selectionChipTone = selectedCount > 0 ? "info" : "neutral";

  const editReason = formatEditReason(selectedCount);
  const editIsActive = selectedCount === 1;

  const sendDisabledReason =
    deviceReason ?? formatSendReason(deviceState);

  const isScanning = deviceState === "scanning";

  return (
    <SurfacePanel
      elevation={1}
      as="div"
      ariaLabelledBy={titleId}
      className="lunii-panel"
    >
      <h2 id={titleId} className="lunii-panel__title">
        Panneau de décision
      </h2>

      <section className="lunii-panel__selection" aria-label="Sélection courante">
        <StateChip tone={selectionChipTone} label={selectionChipLabel} />
        {editIsActive ? (
          <Button variant="secondary" onClick={onEdit}>
            Éditer
          </Button>
        ) : (
          <>
            <Button
              variant="secondary"
              aria-disabled="true"
              aria-describedby={editReasonId}
            >
              Éditer
            </Button>
            <p id={editReasonId} className="lunii-panel__reason">
              {editReason}
            </p>
          </>
        )}
      </section>

      <section className="lunii-panel__device" aria-label="État de l'appareil">
        <StateChip tone={deviceChipTone} label={deviceChipLabel} />
        {deviceState === "idle" && supportedOperations && (
          <ul
            className="lunii-panel__operations"
            aria-label="Opérations supportées par l'appareil détecté"
          >
            {formatSupportedOperationLabels(supportedOperations).map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        )}
        <Button aria-disabled="true" aria-describedby={deviceReasonId}>
          Envoyer vers la Lunii
        </Button>
        <p id={deviceReasonId} className="lunii-panel__reason">
          {sendDisabledReason}
        </p>
        {onRefreshDevice && !isScanning && (
          <Button
            variant="quiet"
            onClick={onRefreshDevice}
            aria-label="Réessayer la détection de l'appareil"
          >
            Réessayer la détection
          </Button>
        )}
        {showSupportProfile && (
          <Button
            variant="quiet"
            onClick={onConsultSupportProfile}
            aria-label="Consulter le profil de support officiel"
          >
            Consulter le profil de support
          </Button>
        )}
      </section>
    </SurfacePanel>
  );
}

function formatSelectionLabel(count: number): string {
  if (count <= 0) return "Aucune histoire sélectionnée";
  if (count === 1) return "1 histoire sélectionnée";
  return `${count} histoires sélectionnées`;
}

function formatEditReason(count: number): string {
  if (count <= 0) return "Reprise indisponible: aucune histoire sélectionnée";
  return "Reprise indisponible: sélection multiple";
}

function formatDeviceChipLabel(
  state: LuniiDeviceState,
  deviceLabel?: string,
): string {
  switch (state) {
    case "absent":
      return "Aucun appareil connecté";
    case "idle":
      return deviceLabel ? `Appareil prêt — ${deviceLabel}` : "Appareil prêt";
    case "unsupported":
      return "Profil non supporté";
    case "ambiguous":
      return "Profil ambigu";
    case "scanning":
      return "Détection en cours…";
    case "error":
      return "Détection indisponible";
  }
}

function formatDeviceChipTone(
  state: LuniiDeviceState,
): "neutral" | "info" | "warning" | "error" {
  switch (state) {
    case "idle":
      return "info";
    case "unsupported":
    case "ambiguous":
      return "warning";
    case "error":
      return "error";
    case "scanning":
    case "absent":
      return "neutral";
  }
}

function formatSupportedOperationLabels(
  ops: SupportedOperationsDto,
): string[] {
  // Stable, parent-friendly French copy mirroring the canonical
  // labels in docs/architecture/device-support-profile.md.
  const matrix: Array<[keyof SupportedOperationsDto, string]> = [
    ["readLibrary", "Lecture bibliothèque appareil"],
    ["inspectStory", "Inspection d'histoire"],
    ["importStory", "Import vers la bibliothèque locale"],
    ["writeStory", "Transfert vers la Lunii"],
  ];
  return matrix.map(([k, label]) => `${ops[k] ? "✓" : "—"} ${label}`);
}

function formatSendReason(state: LuniiDeviceState): string {
  switch (state) {
    case "absent":
      // Distinct from "appareil non supporté" so the user knows
      // whether to plug something in (absent) or check the profile
      // (unsupported). The canonical phrasing lives in
      // docs/architecture/ui-states.md.
      return "Envoi indisponible: aucun appareil connecté";
    case "idle":
      // MVP Phase 1: even a supported device cannot accept a transfer
      // yet — Epic 3 wires the gate. Distinct copy from "appareil
      // non supporté" so the user sees a positive "supported device,
      // transfer not wired yet" message instead of a contradiction
      // with the `Appareil prêt — Lunii …` chip just above.
      return "Envoi indisponible: transfert pas encore activé (MVP Phase 1)";
    case "unsupported":
      return "Envoi indisponible: profil non supporté";
    case "ambiguous":
      return "Envoi indisponible: profil ambigu";
    case "scanning":
      return "Envoi indisponible: détection en cours";
    case "error":
      return "Envoi indisponible: détection en échec";
  }
}
