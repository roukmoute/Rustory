import type React from "react";

import "./LibraryLayout.css";

export interface LibraryLayoutProps {
  leftNav: React.ReactNode;
  center: React.ReactNode;
  rightPanel: React.ReactNode;
  panelOpen?: boolean;
  mainLabel?: string;
}

/** Two-column library with an optional, permanently mounted decision panel.
 * Keeping the slots mounted preserves in-flight operations and live regions. */
export function LibraryLayout({
  leftNav,
  center,
  rightPanel,
  panelOpen = true,
  mainLabel = "Collection d'histoires",
}: LibraryLayoutProps): React.JSX.Element {
  return (
    <div
      className={`library-layout${panelOpen ? " library-layout--details" : ""}`}
    >
      <nav className="library-layout__nav" aria-label="Navigation bibliothèque">
        {leftNav}
      </nav>
      <main className="library-layout__main" aria-label={mainLabel}>
        {center}
      </main>
      <aside
        className="library-layout__panel"
        aria-label="Panneau de décision"
        hidden={!panelOpen}
      >
        {rightPanel}
      </aside>
    </div>
  );
}
