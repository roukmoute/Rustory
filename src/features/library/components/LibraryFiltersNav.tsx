import type React from "react";
import { NavLink } from "react-router-dom";
import { Icon } from "../../../shared/ui";
import "./LibraryFiltersNav.css";

/** Navigation to three distinct collections, matching the product's screens. */
export function LibraryFiltersNav(): React.JSX.Element {
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
        <NavLink to="/drafts" className="library-filters-nav__entry">
          <Icon name="edit" />
          Mes brouillons
        </NavLink>
        <NavLink to="/device" className="library-filters-nav__entry">
          <Icon name="device" />
          Sur l'appareil
        </NavLink>
      </div>
    </div>
  );
}
