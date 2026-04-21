import type React from "react";

import "./StateChip.css";

export type StateChipTone =
  | "neutral"
  | "info"
  | "success"
  | "warning"
  | "error";

export interface StateChipProps {
  tone: StateChipTone;
  label: string;
  className?: string;
}

/**
 * A small tone-coded status pill. Color alone never carries meaning — every
 * tone ships with an ASCII glyph prefix so the distinction survives grayscale
 * and color-blindness (NFR21, UX-DR21).
 */
const GLYPH: Record<StateChipTone, string> = {
  neutral: "•",
  info: "i",
  success: "✓",
  warning: "!",
  error: "×",
};

export function StateChip({
  tone,
  label,
  className,
}: StateChipProps): React.JSX.Element {
  return (
    <span
      className={["ds-chip", `ds-chip--${tone}`, className]
        .filter(Boolean)
        .join(" ")}
    >
      <span className="ds-chip__glyph" aria-hidden="true">
        {GLYPH[tone]}
      </span>
      <span className="ds-chip__label">{label}</span>
    </span>
  );
}
