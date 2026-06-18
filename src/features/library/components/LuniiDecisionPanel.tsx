import type React from "react";
import { useId } from "react";

import type { AppError } from "../../../shared/errors/app-error";
import type { SupportedOperationsDto } from "../../../shared/ipc-contracts/device";
import {
  Button,
  ProgressIndicator,
  StateChip,
  SurfacePanel,
} from "../../../shared/ui";

import "./LuniiDecisionPanel.css";

export type LuniiDeviceState =
  | "absent"
  | "idle"
  | "unsupported"
  | "ambiguous"
  | "scanning"
  | "error";

/**
 * Read-only pre-transfer comparison, composed by Rust and only PRESENTED
 * here. `none` is the sober "nothing to compare yet" state (no single local
 * selection, or no readable device); `ready` carries the device membership
 * (`onDevice` ⇒ a send would replace) and how many other device stories stay
 * untouched. No size metric — there is no decisional volume before media
 * preparation.
 */
/** Why no comparison can be shown — each maps to a distinct, actionable
 *  hint so the user knows exactly what to do next (select a story, narrow to
 *  one, or plug a readable Lunii). */
export type NoComparisonReason = "no-selection" | "multi-selection" | "no-device";

export type TransferComparisonView =
  | { kind: "none"; reason: NoComparisonReason }
  | { kind: "loading" }
  | { kind: "ready"; onDevice: boolean; unchangedCount: number }
  | { kind: "error"; error: AppError };

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
  /** Read-only pre-transfer comparison. When omitted, the comparison
   *  section is not rendered at all (used by tests/storybook that do not
   *  exercise it). When provided, it renders between the selection and the
   *  device regions — never as the panel's visual center. */
  comparison?: TransferComparisonView;
  /** Retry trigger for a failed comparison — wired by the route to
   *  `useTransferPreview.refresh`. Makes the "Réessaie la comparaison" copy
   *  actionable. When omitted, the error shows its text without a button. */
  onRetryComparison?: () => void;
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
  comparison,
  onRetryComparison,
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

      {comparison && (
        <section
          className="lunii-panel__comparison"
          aria-label="Comparaison avant envoi"
          aria-live="polite"
        >
          {renderComparison(comparison, onRetryComparison)}
        </section>
      )}

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

function renderComparison(
  view: TransferComparisonView,
  onRetryComparison?: () => void,
): React.JSX.Element {
  switch (view.kind) {
    case "none":
      // Distinct hint per cause so the next gesture is unambiguous.
      return (
        <p className="lunii-panel__reason">
          {formatNoComparisonHint(view.reason)}
        </p>
      );
    case "loading":
      return (
        <ProgressIndicator mode="indeterminate" label="Comparaison en cours…" />
      );
    case "ready":
      return (
        <>
          <StateChip
            tone={view.onDevice ? "warning" : "info"}
            label={
              view.onDevice
                ? "Déjà présente sur l'appareil"
                : "Nouvelle sur l'appareil"
            }
          />
          <p className="lunii-panel__comparison-verdict">
            {view.onDevice
              ? "Déjà présente sur l'appareil — un envoi la remplacerait."
              : "Cette histoire serait ajoutée à l'appareil."}
          </p>
          <p className="lunii-panel__reason">
            {formatUnchanged(view.unchangedCount)}
          </p>
        </>
      );
    case "error":
      // Critical feedback IN CONTEXT (role="alert"), never a toast (UX-DR15).
      // The "Réessaie la comparaison" copy is made actionable by a retry CTA.
      return (
        <div role="alert" className="lunii-panel__comparison-error">
          <p>{view.error.message}</p>
          {view.error.userAction && <p>{view.error.userAction}</p>}
          {onRetryComparison && (
            <Button
              variant="quiet"
              onClick={onRetryComparison}
              aria-label="Réessayer la comparaison"
            >
              Réessayer
            </Button>
          )}
        </div>
      );
  }
}

function formatNoComparisonHint(reason: NoComparisonReason): string {
  switch (reason) {
    case "no-selection":
      return "Sélectionne une histoire locale pour comparer avant l'envoi.";
    case "multi-selection":
      return "Sélectionne une seule histoire locale pour comparer (le transfert multiple n'est pas encore disponible).";
    case "no-device":
      return "Branche une Lunii lisible pour comparer l'histoire sélectionnée avant l'envoi.";
  }
}

function formatUnchanged(count: number): string {
  if (count <= 0) return "Aucune autre histoire de l'appareil ne sera modifiée.";
  if (count === 1) return "1 autre histoire de l'appareil restera inchangée.";
  return `${count} autres histoires de l'appareil resteront inchangées.`;
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
    ["importStory", "Copie dans la bibliothèque locale"],
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
