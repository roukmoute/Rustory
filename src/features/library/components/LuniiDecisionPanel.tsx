import type React from "react";
import { useId } from "react";

import { Button, StateChip, SurfacePanel } from "../../../shared/ui";

import "./LuniiDecisionPanel.css";

export type LuniiDeviceState = "absent" | "idle";

export interface LuniiDecisionPanelProps {
  deviceState?: LuniiDeviceState;
}

/**
 * Minimal decision surface shown in the library's right column.
 *
 * Scope right now: device presence badge + disabled send CTA with a canonical
 * reason from docs/architecture/ui-states.md. Selection size, compatibility
 * checks, preparation progress and retry traces are intentionally out —
 * they show up once selection and the transfer pipeline are wired.
 */
export function LuniiDecisionPanel({
  deviceState = "absent",
}: LuniiDecisionPanelProps): React.JSX.Element {
  const titleId = useId();
  const reasonId = useId();

  const chipLabel =
    deviceState === "absent" ? "Aucun appareil connecté" : "Appareil prêt";

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
      <StateChip tone="neutral" label={chipLabel} />
      <Button aria-disabled="true" aria-describedby={reasonId}>
        Envoyer vers la Lunii
      </Button>
      <p id={reasonId} className="lunii-panel__reason">
        Envoi indisponible: appareil non supporté
      </p>
    </SurfacePanel>
  );
}
