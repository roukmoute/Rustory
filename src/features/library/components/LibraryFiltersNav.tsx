import type React from "react";
import { NavLink } from "react-router-dom";
import { Icon } from "../../../shared/ui";
import "./LibraryFiltersNav.css";

interface LibraryFiltersNavProps {
  onCatalog?: () => void;
}

/** Navigation to real collections. Device content stays distinct from local stories. */
export function LibraryFiltersNav({
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
        <NavLink to="/library" className="library-filters-nav__entry">
          <Icon name="grid" />
          Ma bibliothèque
        </NavLink>
        <NavLink to="/device" className="library-filters-nav__entry">
          <Icon name="device" />
          Sur l'appareil
        </NavLink>
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
