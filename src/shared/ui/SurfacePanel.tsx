import type React from "react";

import "./SurfacePanel.css";

export interface SurfacePanelProps {
  elevation?: 0 | 1 | 2;
  as?: "section" | "aside" | "article" | "div";
  ariaLabelledBy?: string;
  className?: string;
  children?: React.ReactNode;
}

/**
 * Neutral container surface. No business logic — consumers provide content
 * and the semantic tag. Elevation controls visual separation only.
 */
export function SurfacePanel({
  elevation = 0,
  as: Tag = "section",
  ariaLabelledBy,
  className,
  children,
}: SurfacePanelProps): React.JSX.Element {
  return (
    <Tag
      className={[
        "ds-surface",
        `ds-surface--elevation-${elevation}`,
        className,
      ]
        .filter(Boolean)
        .join(" ")}
      aria-labelledby={ariaLabelledBy}
    >
      {children}
    </Tag>
  );
}
