import type React from "react";

import "./LibraryLayout.css";

export interface LibraryLayoutProps {
  leftNav: React.ReactNode;
  center: React.ReactNode;
  rightPanel: React.ReactNode;
}

/**
 * Desktop 3-column grid for the library context.
 *
 * Standard mode  (≥ 1280px): minmax(240px, 280px) | 1fr | minmax(320px, 360px)
 * Reduced mode   (1024–1279px, via @media): 200px | 1fr | 300px
 * Under 1024px: handled by the Tauri minWidth contract — the window refuses
 * to shrink further, so no additional responsive fallback is needed.
 *
 * Purely presentational: receives 3 slots, applies semantic regions, no
 * fetching or business logic.
 */
export function LibraryLayout({
  leftNav,
  center,
  rightPanel,
}: LibraryLayoutProps): React.JSX.Element {
  return (
    <div className="library-layout">
      <nav className="library-layout__nav" aria-label="Filtres bibliothèque">
        {leftNav}
      </nav>
      <main
        className="library-layout__main"
        aria-label="Collection d'histoires"
      >
        {center}
      </main>
      <aside
        className="library-layout__panel"
        aria-label="Panneau de décision"
      >
        {rightPanel}
      </aside>
    </div>
  );
}
