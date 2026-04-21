import type React from "react";
import { useId } from "react";

import "./ProgressIndicator.css";

export type ProgressMode = "indeterminate" | "determinate";

export interface ProgressIndicatorProps {
  mode: ProgressMode;
  label: string;
  value?: number;
}

/**
 * A role=progressbar that always carries a visible label (not just aria-label).
 * `determinate` requires `value` in [0, 100]; `indeterminate` animates gently
 * and respects prefers-reduced-motion via the global rule in tokens.css.
 */
export function ProgressIndicator({
  mode,
  label,
  value,
}: ProgressIndicatorProps): React.JSX.Element {
  const isDeterminate = mode === "determinate";
  const clamped =
    isDeterminate && typeof value === "number" && Number.isFinite(value)
      ? Math.max(0, Math.min(100, value))
      : undefined;
  const labelId = useId();

  return (
    <div className="ds-progress">
      <span id={labelId} className="ds-progress__label">
        {label}
      </span>
      <div
        role="progressbar"
        aria-labelledby={labelId}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={clamped}
        aria-valuetext={
          isDeterminate && clamped !== undefined ? `${clamped}%` : undefined
        }
        className={[
          "ds-progress__track",
          isDeterminate
            ? "ds-progress__track--determinate"
            : "ds-progress__track--indeterminate",
        ].join(" ")}
      >
        <span
          className="ds-progress__fill"
          style={
            isDeterminate && clamped !== undefined
              ? { width: `${clamped}%` }
              : undefined
          }
        />
      </div>
    </div>
  );
}
