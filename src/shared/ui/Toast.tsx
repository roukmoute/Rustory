import type React from "react";
import { useEffect } from "react";

import "./Toast.css";
import type { StateChipTone } from "./StateChip";

export interface ToastProps {
  tone: Exclude<StateChipTone, "error">;
  message: string;
  onDismiss: () => void;
  durationMs?: number;
}

/**
 * Lightweight, auto-dismissing confirmation affordance.
 *
 * Deliberate restriction: a `Toast` must never carry a critical error on its
 * own (UX-DR15). The `tone` type excludes `error` so the compiler enforces
 * the rule — critical failures live in-context (alerts, banners), not in a
 * toast that disappears.
 */
export function Toast({
  tone,
  message,
  onDismiss,
  durationMs = 4000,
}: ToastProps): React.JSX.Element {
  useEffect(() => {
    const id = setTimeout(onDismiss, durationMs);
    return () => clearTimeout(id);
  }, [onDismiss, durationMs]);

  return (
    <div
      className={["ds-toast", `ds-toast--${tone}`].join(" ")}
      role="status"
      aria-live="polite"
    >
      {message}
    </div>
  );
}
