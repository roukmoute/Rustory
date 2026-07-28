import type React from "react";
import { useEffect, useState } from "react";

import { ProgressIndicator } from "../../../shared/ui";

import "./DropOverlay.css";

/**
 * Delay before the analyzing overlay appears. A fast (small-file) analysis
 * settles well under this, so it never flashes; only a slow (big-pack)
 * analysis — the one that actually leaves the user wondering what's happening
 * — stays in flight long enough to reveal it.
 */
const ANALYZING_REVEAL_DELAY_MS = 200;

/**
 * Post-drop feedback overlay (`Drop Intent Contract`). Once the mouse is
 * released the drop is ANALYZED (a recognition review, never a direct import);
 * for a big pack that read + media probe takes a moment during which the
 * decorative hover overlay is already gone — a gap where nothing signals that
 * work is happening. This fills it with an ANNOUNCED (`role="status"`, unlike
 * the aria-hidden hover overlay) "Analyse en cours…". Revealed only after a
 * short delay so an instant small-file analysis never flashes it.
 */
export function DropAnalyzingOverlay({
  active,
}: {
  active: boolean;
}): React.JSX.Element | null {
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (!active) {
      setVisible(false);
      return;
    }
    const timer = setTimeout(() => {
      setVisible(true);
    }, ANALYZING_REVEAL_DELAY_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [active]);

  if (!visible) return null;
  return (
    <div className="drop-overlay drop-overlay--analyzing" role="status">
      <div className="drop-overlay__panel">
        <p className="drop-overlay__title">Analyse en cours…</p>
        <ProgressIndicator
          mode="indeterminate"
          label="Reconnaissance de l'histoire déposée"
        />
      </div>
    </div>
  );
}
