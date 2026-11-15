import type React from "react";
import { useId, useMemo, useState } from "react";

import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import { Button, Field, Icon, ProgressIndicator } from "../../../shared/ui";
import {
  applyLibraryFilters,
  type LibrarySortKey,
} from "../hooks/use-library-collection";
import {
  StoryCard,
  type StoryCardSelectionMode,
  type StoryPreparationBadge,
} from "./StoryCard";

import "./StoryCollection.css";

export interface StoryCollectionProps {
  /** The route owns which stories belong to each collection. */
  variant?: "library" | "drafts";
  stories: StoryCardDto[];
  isLoading: boolean;
  query: string;
  sort: LibrarySortKey;
  onQueryChange: (query: string) => void;
  onSortChange: (sort: LibrarySortKey) => void;
  onResetFilters: () => void;
  selectedStoryIds?: ReadonlySet<string>;
  /** Per-story preparation badge (AC2). Keyed by story id; absent ⇒ no badge.
   *  Derived from the transfer flow, never a competing source of truth. */
  preparationBadges?: ReadonlyMap<string, StoryPreparationBadge>;
  /** Stories already on the connected device (their ids), with the
   *  family-correct stamp label. Absent ⇒ no stamps at all. */
  onDeviceStoryIds?: ReadonlySet<string>;
  deviceStampLabel?: string;
  onSelectStory?: (id: string, mode: StoryCardSelectionMode) => void;
  onOpenStory?: (id: string) => void;
  /** Right-click on a card: the parent opens a context menu at the cursor. */
  onStoryContextMenu?: (id: string, x: number, y: number) => void;
  /** Fires when the user requests to open the "Créer une histoire" flow.
   *  When omitted, the CTA in the empty state falls back to its disabled
   *  form with the canonical "indisponible" reason. */
  onCreateStoryRequest?: () => void;
  /** Fires when the user requests to import a local `.rustory` artifact.
   *  When omitted, the "Importer une histoire" CTA is hidden. */
  onImportArtifactRequest?: () => void;
  /** Optional broader import chooser used by the empty-library card. */
  onImportRequest?: () => void;
  /** Opens the existing official catalogue surface. */
  onCatalogRequest?: () => void;
  /** Authoritative catalogue metadata count, omitted until known. */
  catalogCount?: number;
  /** Disables the import CTA while an analysis / import is in flight, so a
   *  second pick cannot start a concurrent flow. */
  isImportBusy?: boolean;
  view?: "grid" | "list";
  onViewChange?: (view: "grid" | "list") => void;
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
  variant = "library",
  stories,
  isLoading,
  query,
  sort,
  onQueryChange,
  onSortChange,
  onResetFilters,
  selectedStoryIds = EMPTY_SELECTION,
  preparationBadges,
  onDeviceStoryIds,
  deviceStampLabel,
  onSelectStory,
  onOpenStory,
  onStoryContextMenu,
  onCreateStoryRequest,
  onImportArtifactRequest,
  onImportRequest,
  onCatalogRequest,
  catalogCount,
  isImportBusy = false,
  view: controlledView,
  onViewChange,
}: StoryCollectionProps): React.JSX.Element {
  const titleId = useId();
  const searchId = useId();
  const sortId = useId();
  const [localView, setLocalView] = useState<"grid" | "list">("grid");
  const view = controlledView ?? localView;
  const setView = onViewChange ?? setLocalView;
  const createDisabledId = useId();
  const catalogDisabledId = useId();
  const importDisabledId = useId();

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
  const isDrafts = variant === "drafts";
  const showLibraryWelcome = !isDrafts && state.kind === "loaded-empty";
  const requestImport = onImportRequest ?? onImportArtifactRequest;

  const handleSelect = (id: string, mode: StoryCardSelectionMode): void => {
    onSelectStory?.(id, mode);
  };

  const handleOpen = (id: string): void => {
    onOpenStory?.(id);
  };

  return (
    <section
      className={`story-collection story-collection--${variant}${showLibraryWelcome ? " story-collection--welcome" : ""}`}
      aria-labelledby={titleId}
    >
      <header className="story-collection__header">
        <div className="story-collection__heading-row">
          <div>
            {!isDrafts && !showLibraryWelcome ? (
              <p className="story-collection__eyebrow">
                À lire, à écouter, à imaginer
              </p>
            ) : null}
            <h1 id={titleId} className="story-collection__title">
              {isDrafts
                ? "Mes brouillons"
                : showLibraryWelcome
                  ? "Par quoi veux-tu commencer ?"
                  : "Ma bibliothèque"}
            </h1>
            {isDrafts ? (
              <p className="story-collection__description">
                Tes histoires créées ici, à retrouver pour continuer à écrire.
              </p>
            ) : null}
          </div>
          {state.kind !== "loaded-empty" ? (
            <div className="story-collection__actions">
              {!isDrafts && onImportArtifactRequest ? (
                <Button
                  variant="quiet"
                  onClick={onImportArtifactRequest}
                  disabled={isImportBusy}
                  aria-busy={isImportBusy}
                >
                  <Icon name="upload" size={17} />
                  Importer une histoire
                </Button>
              ) : null}
              {onCreateStoryRequest ? (
                <Button
                  variant={isDrafts ? "secondary" : "primary"}
                  onClick={onCreateStoryRequest}
                >
                  <Icon name="plus" size={18} />
                  Créer une histoire
                </Button>
              ) : null}
            </div>
          ) : null}
        </div>
        {!showLibraryWelcome ? (
          <div className="story-collection__controls">
            <div className="story-collection__search">
              <Icon name="search" size={20} />
              <Field
                id={searchId}
                label="Rechercher une histoire"
                type="search"
                value={query}
                onChange={onQueryChange}
                placeholder={isDrafts
                  ? "Chercher un brouillon…"
                  : "Chercher une histoire…"}
              />
            </div>
            <div className="story-collection__sort">
              <label className="story-collection__sr-only" htmlFor={sortId}>
                Trier par
              </label>
              <select
                id={sortId}
                className="story-collection__select"
                value={sort}
                onChange={(event) => {
                  const next = event.target.value;
                  if (isLibrarySortKey(next)) onSortChange(next);
                }}
              >
                <option value="titre-asc">Titre (A → Z)</option>
                <option value="titre-desc">Titre (Z → A)</option>
              </select>
            </div>
            <div
              className="story-collection__view"
              role="group"
              aria-label="Affichage"
            >
              <button
                type="button"
                aria-label="Affichage en cartes"
                aria-pressed={view === "grid"}
                onClick={() => setView("grid")}
              >
                <Icon name="grid" />
              </button>
              <button
                type="button"
                aria-label="Affichage en liste"
                aria-pressed={view === "list"}
                onClick={() => setView("list")}
              >
                <Icon name="list" />
              </button>
            </div>
          </div>
        ) : null}
        {/*
         * Counter is read by the empty / filtered-empty / pending regions via
         * their own role=status. Keeping aria-live here would double-announce
         * on every keystroke — rely on the status regions instead.
         */}
        {!showLibraryWelcome ? (
          <p className="story-collection__counter">
            {countersLabel(stories.length, state, selectedCount, variant)}
          </p>
        ) : null}
        {state.kind === "loaded" ? (
          <details className="story-collection__selection-help">
            <summary>Sélectionner plusieurs histoires</summary>
            <p>
              Clique une carte pour la sélectionner. Ctrl+clic (ou Cmd+clic sur
              macOS) pour en sélectionner plusieurs. Double-clic pour ouvrir
              l'édition.
            </p>
          </details>
        ) : null}
      </header>

      {state.kind === "pending" ? (
        <div
          className="story-collection__pending"
          role="status"
          aria-live="polite"
        >
          <ProgressIndicator
            mode="indeterminate"
            label={isDrafts
              ? "Chargement des brouillons…"
              : "Chargement de la bibliothèque…"}
          />
        </div>
      ) : null}

      {showLibraryWelcome ? (
        <div className="story-collection__welcome">
          <p
            className="story-collection__welcome-intro"
            role="status"
            aria-live="polite"
          >
            Ta bibliothèque est vide pour l'instant. Choisis comment la remplir.
          </p>
          <div className="story-collection__choices">
            <section className="story-collection__choice">
              <p className="story-collection__eyebrow">
                Des histoires à découvrir
              </p>
              <h2 className="story-collection__choice-title">
                Catalogue officiel
              </h2>
              <p className="story-collection__choice-description">
                Retrouve les titres et les images des histoires officielles.
              </p>
              {catalogCount !== undefined ? (
                <p className="story-collection__choice-count">
                  {`${catalogCount} titre${catalogCount === 1 ? "" : "s"} référencé${catalogCount === 1 ? "" : "s"}`}
                </p>
              ) : null}
              <Button
                variant="secondary"
                onClick={onCatalogRequest}
                aria-disabled={!onCatalogRequest || undefined}
                aria-describedby={
                  !onCatalogRequest ? catalogDisabledId : undefined
                }
              >
                Voir le catalogue
              </Button>
              {!onCatalogRequest ? (
                <p
                  id={catalogDisabledId}
                  className="story-collection__empty-reason"
                >
                  Catalogue indisponible pour l'instant.
                </p>
              ) : null}
            </section>
            <section className="story-collection__choice">
              <p className="story-collection__eyebrow">Déjà une histoire ?</p>
              <h2 className="story-collection__choice-title">Importer</h2>
              <p className="story-collection__choice-description">
                {onImportRequest
                  ? "Depuis un fichier, un dossier ou un lien."
                  : "Ajoute une histoire depuis un fichier .rustory."}
              </p>
              <Button
                variant="secondary"
                onClick={requestImport}
                aria-label="Importer une histoire"
                disabled={isImportBusy}
                aria-busy={isImportBusy}
                aria-disabled={!requestImport || undefined}
                aria-describedby={!requestImport ? importDisabledId : undefined}
              >
                Importer
              </Button>
              {!requestImport ? (
                <p
                  id={importDisabledId}
                  className="story-collection__empty-reason"
                >
                  Import indisponible pour l'instant.
                </p>
              ) : null}
            </section>
            <section className="story-collection__choice story-collection__choice--create">
              <p className="story-collection__eyebrow">Envie de créer ?</p>
              <h2 className="story-collection__choice-title">Écrire la mienne</h2>
              <p className="story-collection__choice-description">
                Un brouillon que tu peux finir plus tard.
              </p>
              <Button
                variant="secondary"
                onClick={onCreateStoryRequest}
                aria-disabled={!onCreateStoryRequest || undefined}
                aria-describedby={
                  !onCreateStoryRequest ? createDisabledId : undefined
                }
              >
                Créer une histoire
              </Button>
              {!onCreateStoryRequest ? (
                <p
                  id={createDisabledId}
                  className="story-collection__empty-reason"
                >
                  Création d'histoire indisponible pour l'instant.
                </p>
              ) : null}
            </section>
          </div>
        </div>
      ) : null}

      {isDrafts && state.kind === "loaded-empty" ? (
        <section
          className="story-collection__empty"
          role="status"
          aria-live="polite"
        >
          <div className="story-collection__empty-art" aria-hidden="true">
            <Icon name="book" size={70} />
            <span>✦</span>
          </div>
          <p className="story-collection__eyebrow">
            Une nouvelle histoire commence ici
          </p>
          <h2 className="story-collection__empty-title">
            Aucun brouillon pour l'instant
          </h2>
          <p className="story-collection__empty-hint">
            Commence une histoire et retrouve-la ici pour continuer à écrire.
          </p>
          {onCreateStoryRequest ? (
            <Button variant="primary" onClick={onCreateStoryRequest}>
              Créer une histoire
            </Button>
          ) : (
            <>
              <Button aria-disabled="true" aria-describedby={createDisabledId}>
                Créer une histoire
              </Button>
              <p
                id={createDisabledId}
                className="story-collection__empty-reason"
              >
                Création d'histoire indisponible pour l'instant.
              </p>
            </>
          )}
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
        <ul
          className={`story-collection__list${view === "list" ? " story-collection__list--rows" : ""}`}
        >
          {state.visible.map((story) => (
            <li key={story.id} className="story-collection__item">
              <StoryCard
                story={story}
                isDraft={isDrafts}
                isSelected={selectedStoryIds.has(story.id)}
                selectionSize={selectedCount}
                preparationBadge={preparationBadges?.get(story.id)}
                deviceStamp={
                  deviceStampLabel && onDeviceStoryIds?.has(story.id)
                    ? deviceStampLabel
                    : undefined
                }
                onSelect={handleSelect}
                onOpen={handleOpen}
                onContextMenu={onStoryContextMenu}
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
  variant: "library" | "drafts",
): string {
  if (state.kind === "pending") return "Chargement en cours";
  if (variant === "drafts") {
    const shown = state.kind === "loaded" ? state.visible.length : 0;
    const count = shown === total ? `${total}` : `${shown} sur ${total}`;
    const selected = selectedCount > 0
      ? ` — ${selectedCount} sélectionné${selectedCount === 1 ? "" : "s"}`
      : "";
    return `${count} brouillon${total === 1 ? "" : "s"} · enregistré${total === 1 ? "" : "s"} sur cet ordinateur${selected}`;
  }
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
