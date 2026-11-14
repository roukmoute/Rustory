import type React from "react";
import { Icon } from "../../../shared/ui";
import "./LibraryFiltersNav.css";

interface LibraryFiltersNavProps {
  activeSection?: "library" | "device";
  onLibrary?: () => void;
  onDevice?: () => void;
  onCatalog?: () => void;
}

/** Navigation to real collections. Device content stays distinct from local stories. */
export function LibraryFiltersNav({
  activeSection = "library",
  onLibrary,
  onDevice,
  onCatalog,
}: LibraryFiltersNavProps): React.JSX.Element {
  return (
    <div className="library-filters-nav">
      <div className="library-filters-nav__brand">
        <span className="library-filters-nav__brand-icon">
          <Icon name="book" size={27} />
        </span>
        Rustory<span className="library-filters-nav__brand-dot">•</span>
      </div>
      <p className="library-filters-nav__tagline">Des histoires à emporter.</p>
      <div className="library-filters-nav__list">
        <button
          type="button"
          className="library-filters-nav__entry"
          aria-current={activeSection === "library" ? "page" : undefined}
          onClick={onLibrary}
        >
          <Icon name="grid" />
          Ma bibliothèque
        </button>
        <button
          type="button"
          className="library-filters-nav__entry"
          aria-current={activeSection === "device" ? "page" : undefined}
          onClick={onDevice}
        >
          <Icon name="device" />
          Sur l'appareil
        </button>
        <button
          type="button"
          className="library-filters-nav__entry"
          onClick={onCatalog}
        >
          <Icon name="book" />
          Catalogue officiel
        </button>
      </div>
    </div>
  );
}
