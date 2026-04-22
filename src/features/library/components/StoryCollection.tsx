import type React from "react";
import { useId, useMemo } from "react";

import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import { Button, Field, ProgressIndicator } from "../../../shared/ui";
import {
  applyLibraryFilters,
  type LibrarySortKey,
} from "../hooks/use-library-collection";
import { StoryCard, type StoryCardSelectionMode } from "./StoryCard";

import "./StoryCollection.css";

export interface StoryCollectionProps {
  stories: StoryCardDto[];
  isLoading: boolean;
  query: string;
  sort: LibrarySortKey;
  onQueryChange: (query: string) => void;
  onSortChange: (sort: LibrarySortKey) => void;
  onResetFilters: () => void;
  selectedStoryIds?: ReadonlySet<string>;
  onSelectStory?: (id: string, mode: StoryCardSelectionMode) => void;
  onOpenStory?: (id: string) => void;
}

type CollectionState =
  | { kind: "pending" }
  | { kind: "loaded-empty" }
  | { kind: "filtered-empty" }
  | { kind: "loaded"; visible: StoryCardDto[] };

const SORT_VALUES = new Set<LibrarySortKey>(["titre-asc", "titre-desc"]);
const EMPTY_SELECTION: ReadonlySet<string> = new Set();

function isLibrarySortKey(value: string): value is LibrarySortKey {
  return (SORT_VALUES as Set<string>).has(value);
}

export function StoryCollection({
  stories,
  isLoading,
  query,
  sort,
  onQueryChange,
  onSortChange,
  onResetFilters,
  selectedStoryIds = EMPTY_SELECTION,
  onSelectStory,
  onOpenStory,
}: StoryCollectionProps): React.JSX.Element {
  const titleId = useId();
  const searchId = useId();
  const sortId = useId();
  const filterLabelId = useId();
  const filterButtonId = useId();
  const filterReasonId = useId();
  const createDisabledId = useId();

  const visible = useMemo(
    () => applyLibraryFilters({ stories, query, sort }),
    [stories, query, sort],
  );

  const state: CollectionState = isLoading
    ? { kind: "pending" }
    : stories.length === 0
      ? { kind: "loaded-empty" }
      : visible.length === 0
        ? { kind: "filtered-empty" }
        : { kind: "loaded", visible };

  const selectedCount = selectedStoryIds.size;

  const handleSelect = (id: string, mode: StoryCardSelectionMode): void => {
    onSelectStory?.(id, mode);
  };

  const handleOpen = (id: string): void => {
    onOpenStory?.(id);
  };

  return (
    <section className="story-collection" aria-labelledby={titleId}>
      <header className="story-collection__header">
        <h1 id={titleId} className="story-collection__title">
          Bibliothèque
        </h1>
        <div className="story-collection__controls">
          <Field
            id={searchId}
            label="Rechercher une histoire"
            type="search"
            value={query}
            onChange={onQueryChange}
            placeholder="Titre…"
          />
          <div className="story-collection__control">
            <label className="ds-field__label" htmlFor={sortId}>
              Trier par
            </label>
            <select
              id={sortId}
              className="story-collection__select"
              value={sort}
              onChange={(event) => {
                const next = event.target.value;
                if (isLibrarySortKey(next)) {
                  onSortChange(next);
                }
              }}
            >
              <option value="titre-asc">Titre (A → Z)</option>
              <option value="titre-desc">Titre (Z → A)</option>
            </select>
          </div>
          <div className="story-collection__control">
            <span id={filterLabelId} className="ds-field__label">
              Filtre
            </span>
            <Button
              id={filterButtonId}
              variant="quiet"
              aria-disabled="true"
              aria-labelledby={`${filterLabelId} ${filterButtonId}`}
              aria-describedby={filterReasonId}
              className="story-collection__filter"
            >
              Toutes les histoires
            </Button>
            <p
              id={filterReasonId}
              className="story-collection__filter-reason"
            >
              Filtres avancés à venir
            </p>
          </div>
        </div>
        {/*
         * Counter is read by the empty / filtered-empty / pending regions via
         * their own role=status. Keeping aria-live here would double-announce
         * on every keystroke — rely on the status regions instead.
         */}
        <p className="story-collection__counter">
          {countersLabel(stories.length, state, selectedCount)}
        </p>
      </header>

      {state.kind === "pending" ? (
        <div
          className="story-collection__pending"
          role="status"
          aria-live="polite"
        >
          <ProgressIndicator
            mode="indeterminate"
            label="Chargement de la bibliothèque…"
          />
        </div>
      ) : null}

      {state.kind === "loaded-empty" ? (
        <section
          className="story-collection__empty"
          role="status"
          aria-live="polite"
        >
          <h2 className="story-collection__empty-title">
            Ta bibliothèque est vide
          </h2>
          <p className="story-collection__empty-hint">
            Crée ta première histoire pour la retrouver ici à chaque ouverture.
          </p>
          <Button aria-disabled="true" aria-describedby={createDisabledId}>
            Créer une histoire
          </Button>
          <p
            id={createDisabledId}
            className="story-collection__empty-reason"
          >
            Création d'histoire indisponible pour l'instant.
          </p>
        </section>
      ) : null}

      {state.kind === "filtered-empty" ? (
        <section
          className="story-collection__filtered-empty"
          role="status"
          aria-live="polite"
        >
          <h2 className="story-collection__empty-title">Aucun résultat</h2>
          <p className="story-collection__empty-hint">
            Aucune histoire ne correspond à ta recherche ou à tes filtres.
          </p>
          <Button variant="secondary" onClick={onResetFilters}>
            Réinitialiser les filtres
          </Button>
        </section>
      ) : null}

      {state.kind === "loaded" ? (
        <ul className="story-collection__list">
          {state.visible.map((story) => (
            <li key={story.id} className="story-collection__item">
              <StoryCard
                story={story}
                isSelected={selectedStoryIds.has(story.id)}
                onSelect={handleSelect}
                onOpen={handleOpen}
              />
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

function countersLabel(
  total: number,
  state: CollectionState,
  selectedCount: number,
): string {
  if (state.kind === "pending") return "Chargement en cours";
  if (state.kind === "loaded-empty") return "0 histoire";
  const selectedClause =
    selectedCount === 0
      ? ""
      : selectedCount === 1
        ? " — 1 sélectionnée"
        : ` — ${selectedCount} sélectionnées`;
  if (state.kind === "filtered-empty") return `0 sur ${total}${selectedClause}`;
  const shown = state.visible.length;
  const base =
    shown === total
      ? `${total} histoire${total > 1 ? "s" : ""}`
      : `${shown} sur ${total}`;
  return `${base}${selectedClause}`;
}
