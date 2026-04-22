import type React from "react";
import { useId } from "react";

import { Button, StateChip, SurfacePanel } from "../../../shared/ui";

import "./LuniiDecisionPanel.css";

export type LuniiDeviceState = "absent" | "idle";

export interface LuniiDecisionPanelProps {
  deviceState?: LuniiDeviceState;
  selectedCount?: number;
  /** Required when the panel may expose an active Éditer CTA. Defaulting to
   *  `undefined` would risk rendering a silent no-op button; pass a handler
   *  that routes to the edit surface (or that throws if it should never be
   *  called in a given context). */
  onEdit: () => void;
}

/**
 * Decision surface shown in the library's right column.
 *
 * Layer 1 (selection feedback): a state chip that summarizes how many stories
 * are selected, and an `Éditer` CTA that activates exactly when the selection
 * is a singleton. Both are required so the user can always see why a resume
 * is allowed or prevented.
 *
 * Layer 2 (device readiness): the send CTA stays visible but disabled with a
 * canonical reason from `docs/architecture/ui-states.md`. Real device
 * detection and the full transfer pipeline are wired in a later context.
 */
export function LuniiDecisionPanel({
  deviceState = "absent",
  selectedCount = 0,
  onEdit,
}: LuniiDecisionPanelProps): React.JSX.Element {
  const titleId = useId();
  const deviceReasonId = useId();
  const editReasonId = useId();

  const deviceChipLabel =
    deviceState === "absent" ? "Aucun appareil connecté" : "Appareil prêt";

  const selectionChipLabel = formatSelectionLabel(selectedCount);
  const selectionChipTone = selectedCount > 0 ? "info" : "neutral";

  const editReason = formatEditReason(selectedCount);
  const editIsActive = selectedCount === 1;

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
        <StateChip tone="neutral" label={deviceChipLabel} />
        <Button aria-disabled="true" aria-describedby={deviceReasonId}>
          Envoyer vers la Lunii
        </Button>
        <p id={deviceReasonId} className="lunii-panel__reason">
          Envoi indisponible: appareil non supporté
        </p>
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
