import type React from "react";

import { useDropShell } from "../../../shell/state/drop-shell-store";

import "./DropOverlay.css";

/**
 * Frozen hover copy (`product-language.md`) — honest: the drop triggers an
 * ANALYSIS (a recognition review), never a direct import. A frontend-owned
 * literal, typed exactly once.
 */
const DROP_OVERLAY_COPY = "Dépose ton fichier ou ton dossier pour l'analyser";

/**
 * Global hover overlay of the drop channel (`Drop Intent Contract`) —
 * mounted APP-LEVEL by the shell above the routed outlet (the whole window
 * is the drop target, no route owns it). PURELY DECORATIVE feedback of a
 * mouse gesture in progress: `aria-hidden` (the VERDICTS announce through
 * the live regions, a drag has nothing to say to AT), `pointer-events:
 * none` (the drop is captured natively by the webview), no focus, no
 * interaction. Renders on `hoverActive` and closes on `drop:hover-ended`
 * AND on `drop:requested` (both clear the flag — `Leave` is not
 * guaranteed after a `Drop` on every platform, belt and braces).
 */
export function DropOverlay(): React.JSX.Element | null {
  const hoverActive = useDropShell((s) => s.hoverActive);
  if (!hoverActive) return null;

  return (
    <div className="drop-overlay" aria-hidden="true">
      <p className="drop-overlay__copy">{DROP_OVERLAY_COPY}</p>
    </div>
  );
}
