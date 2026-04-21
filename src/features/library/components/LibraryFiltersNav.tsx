import type React from "react";
import { useId } from "react";

import { Button, SurfacePanel } from "../../../shared/ui";

import "./LibraryFiltersNav.css";

interface FilterEntry {
  id: string;
  label: string;
}

const FILTER_ENTRIES: FilterEntry[] = [
  { id: "all", label: "Toutes les histoires" },
  { id: "drafts", label: "Brouillons locaux" },
  { id: "transferred", label: "Histoires transférées" },
];

/**
 * Left column of the library context. Renders the entry points for future
 * global filters. Everything is disabled today with a single canonical
 * reason — the purpose here is structural readability, not filter logic.
 */
export function LibraryFiltersNav(): React.JSX.Element {
  const titleId = useId();
  const reasonId = useId();

  return (
    <SurfacePanel
      elevation={0}
      ariaLabelledBy={titleId}
      className="library-filters-nav"
    >
      <h2 id={titleId} className="library-filters-nav__title">
        Filtres
      </h2>
      <ul className="library-filters-nav__list">
        {FILTER_ENTRIES.map((entry) => (
          <li key={entry.id}>
            <Button
              variant="quiet"
              aria-disabled="true"
              aria-describedby={reasonId}
            >
              {entry.label}
            </Button>
          </li>
        ))}
      </ul>
      <p id={reasonId} className="library-filters-nav__reason">
        Filtres avancés à venir
      </p>
    </SurfacePanel>
  );
}
