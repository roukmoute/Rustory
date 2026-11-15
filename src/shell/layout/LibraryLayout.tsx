import type React from "react";
import { useLayoutEffect, useRef } from "react";

import "./LibraryLayout.css";

export interface LibraryLayoutProps {
  leftNav: React.ReactNode;
  center: React.ReactNode;
  bottomBar?: React.ReactNode;
  mainLabel?: string;
}

/** Navigation and collection keep the same two columns when selection changes.
 * Floating actions belong to the collection and never become a side panel. */
export function LibraryLayout({
  leftNav,
  center,
  bottomBar,
  mainLabel = "Collection d'histoires",
}: LibraryLayoutProps): React.JSX.Element {
  const mainRef = useRef<HTMLElement>(null);
  const bottomBarRef = useRef<HTMLDivElement>(null);
  const hasBottomBar =
    bottomBar !== undefined && bottomBar !== null && bottomBar !== false;

  // Reserve the real bar height, including wrapped controls at larger zoom.
  // The final cards can then scroll fully above the floating actions.
  useLayoutEffect(() => {
    const main = mainRef.current;
    const bar = bottomBarRef.current;
    if (!main || !bar) return;
    const measure = (): void => {
      main.style.setProperty(
        "--library-bottom-bar-height",
        `${Math.ceil(bar.getBoundingClientRect().height)}px`,
      );
    };
    measure();
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(measure);
    observer?.observe(bar);
    window.addEventListener("resize", measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
      main.style.removeProperty("--library-bottom-bar-height");
    };
  }, [hasBottomBar]);

  return (
    <div
      className={`library-layout${hasBottomBar ? " library-layout--bottom-bar" : ""}`}
    >
      <nav className="library-layout__nav" aria-label="Navigation bibliothèque">
        {leftNav}
      </nav>
      <main
        tabIndex={-1}
        ref={mainRef}
        className="library-layout__main"
        aria-label={mainLabel}
      >
        {center}
        {hasBottomBar ? (
          <div className="library-layout__bottom-bar">
            <div
              ref={bottomBarRef}
              className="library-layout__bottom-bar-content"
            >
              {bottomBar}
            </div>
          </div>
        ) : null}
      </main>
    </div>
  );
}
