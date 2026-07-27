import type React from "react";
import { useNavigate } from "react-router-dom";

import { Button, StateChip } from "../../../shared/ui";
import { useUpdateShell } from "../../../shell/state/update-shell-store";

import "./UpdateAvailabilitySignal.css";

// The ONLY frontend-frozen literals of the update surfaces
// (product-language.md): the consultation gesture and its accessible
// name — headline/notice always render Rust-carried, verbatim.
const SEE_DETAILS_LABEL = "Voir les détails";
const SEE_DETAILS_ARIA_LABEL = "Consulter les détails de la mise à jour";

/**
 * The library's PROMINENT update banner (`Update Availability Contract`):
 * a full-width block at the TOP of the library content, rendered ONLY
 * when the launch's (or a manual re-check's) verdict is `updateAvailable`
 * — every other state (up-to-date, unreachable, check in flight) is
 * INVISIBLE here: silence is the rule, the positive is the exception. One
 * gesture: `Voir les détails` navigates IN-APP to `/settings` (the
 * consultation-gesture pattern — no external browser, no outbound link),
 * where the full notice + the install gesture live. Autonomous by design
 * (store + navigate): the route only provides the slot.
 */
export function UpdateAvailabilitySignal(): React.JSX.Element | null {
  const availability = useUpdateShell((s) => s.availability);
  const navigate = useNavigate();
  if (availability === null || availability.status !== "updateAvailable") {
    return null;
  }
  return (
    <div className="update-availability-signal" role="status">
      <div className="update-availability-signal__text">
        <StateChip tone="info" label={availability.headline} />
        <span className="update-availability-signal__notice">
          {availability.notice}
        </span>
      </div>
      <Button
        variant="secondary"
        aria-label={SEE_DETAILS_ARIA_LABEL}
        onClick={() => {
          navigate("/settings");
        }}
      >
        {SEE_DETAILS_LABEL}
      </Button>
    </div>
  );
}
