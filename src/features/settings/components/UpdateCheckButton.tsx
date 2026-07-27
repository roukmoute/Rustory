import type React from "react";
import { useRef, useState } from "react";

import { refreshUpdateAvailability } from "../../../ipc/commands/settings";
import { Button } from "../../../shared/ui";
import { useUpdateShell } from "../../../shell/state/update-shell-store";

import "./UpdateCheckButton.css";

// Frozen frontend copies of the manual re-check gesture (product-language):
// the action label + its accessible name, and the calm neutral lines shown
// while a check runs / when it could not reach the server. The POSITIVE
// verdict copy always renders Rust-carried (headline/notice) via the status
// line, never re-composed here.
const CHECK_LABEL = "Rechercher une mise à jour";
const CHECK_ARIA_LABEL = "Rechercher une mise à jour maintenant";
const CHECKING_LABEL = "Recherche en cours…";
const UNREACHABLE_NOTE =
  "Vérification impossible pour le moment. Réessaie plus tard.";

/**
 * On-demand "Rechercher une mise à jour" gesture, next to the installed
 * version in the settings header. The launch check runs once per launch and
 * is deliberately silent; this button lets the user RE-CHECK now, without
 * relaunching. On success the fresh verdict pours into the shared update
 * shell store — so the settings status line AND the library banner update
 * from the single Rust-owned truth. In-flight is component-local (only this
 * button shows the "en cours" state); a failure is the calm neutral note,
 * never a scary error (absence of information is not an error).
 */
export function UpdateCheckButton(): React.JSX.Element {
  const setAvailability = useUpdateShell((s) => s.setAvailability);
  const [checking, setChecking] = useState(false);
  const [unreachable, setUnreachable] = useState(false);
  const inFlightRef = useRef(false);

  const check = async (): Promise<void> => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    setChecking(true);
    setUnreachable(false);
    try {
      const availability = await refreshUpdateAvailability();
      // The command is infallible: even "unreachable" arrives as a verdict
      // (status `checkUnavailable`). Pour it into the store so every surface
      // reflects the fresh truth; flag the neutral note for that state.
      setAvailability(availability);
      setUnreachable(availability.status === "checkUnavailable");
    } catch {
      // A rejection here is an IPC/contract drift, not a transport story —
      // keep it calm: the neutral "réessaie plus tard" note, no error surface.
      setUnreachable(true);
    } finally {
      inFlightRef.current = false;
      setChecking(false);
    }
  };

  return (
    <div className="update-check">
      <Button
        variant="quiet"
        aria-label={CHECK_ARIA_LABEL}
        aria-busy={checking || undefined}
        aria-disabled={checking || undefined}
        onClick={() => {
          if (!checking) void check();
        }}
      >
        {checking ? CHECKING_LABEL : CHECK_LABEL}
      </Button>
      {unreachable ? (
        <span className="update-check__note" role="status">
          {UNREACHABLE_NOTE}
        </span>
      ) : null}
    </div>
  );
}
