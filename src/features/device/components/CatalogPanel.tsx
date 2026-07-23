import type React from "react";
import { useId } from "react";

import {
  Button,
  ProgressIndicator,
  StateChip,
  SurfacePanel,
} from "../../../shared/ui";
import type { UseOfficialCatalog } from "../hooks/use-official-catalog";

import "./CatalogPanel.css";

export interface CatalogPanelProps {
  catalog: UseOfficialCatalog;
}

/**
 * Official-catalog management (story 2-6, Phase C). Lets the user RECOGNIZE
 * commercial packs by caching Lunii's official `UUID → titre/cover` index.
 *
 * Offline-first is explicit here: the count read on mount is the only
 * automatic call; the network fetch happens ONLY when the user clicks
 * "Récupérer", and a 100%-offline file import is offered alongside. The note
 * states plainly that no connection happens without a deliberate action.
 */
export function CatalogPanel({ catalog }: CatalogPanelProps): React.JSX.Element {
  const titleId = useId();
  const statusId = useId();
  const errorId = useId();

  const { state, action, actionError } = catalog;
  const busy = action !== "idle";

  // The long-running actions (a network refresh, a file import) have no
  // byte-level progress from the backend — a single awaited IPC call — so the
  // honest affordance is an INDETERMINATE bar, never a fake percentage.
  const busyLabel =
    action === "refreshing"
      ? "Récupération du catalogue officiel…"
      : action === "importing"
        ? "Import du fichier de catalogue…"
        : null;

  const statusText =
    state.kind === "loading"
      ? "Lecture du catalogue local…"
      : state.kind === "error"
        ? "Catalogue local illisible."
        : state.count === 0
          ? "Aucun titre officiel en cache."
          : `${state.count} titre${state.count > 1 ? "s" : ""} officiel${
              state.count > 1 ? "s" : ""
            } en cache.`;

  return (
    <SurfacePanel
      elevation={1}
      as="section"
      ariaLabelledBy={titleId}
      className="catalog-panel"
    >
      <h2 id={titleId} className="catalog-panel__title">
        Catalogue officiel
      </h2>

      <div id={statusId} className="catalog-panel__status" aria-live="polite">
        {busyLabel !== null ? (
          <ProgressIndicator mode="indeterminate" label={busyLabel} />
        ) : (
          <p className="catalog-panel__status-text">{statusText}</p>
        )}
      </div>

      <p className="catalog-panel__note">
        Hors-ligne par défaut : Rustory ne contacte aucun serveur sans une
        action de ta part.
      </p>

      <div className="catalog-panel__actions">
        <Button
          variant="secondary"
          aria-disabled={busy || undefined}
          aria-busy={action === "refreshing" || undefined}
          aria-describedby={statusId}
          onClick={() => {
            if (!busy) void catalog.refresh();
          }}
        >
          Récupérer / mettre à jour
        </Button>
        <Button
          variant="quiet"
          aria-disabled={busy || undefined}
          aria-busy={action === "importing" || undefined}
          onClick={() => {
            if (!busy) void catalog.importFile();
          }}
        >
          Importer depuis un fichier
        </Button>
      </div>

      {actionError !== null ? (
        <div className="catalog-panel__error" role="alert">
          <StateChip tone="error" label="Catalogue indisponible" />
          <p id={errorId} className="catalog-panel__error-text">
            {actionError.message}
            {actionError.userAction ? ` ${actionError.userAction}` : ""}
          </p>
          <Button variant="quiet" onClick={catalog.dismissError}>
            Fermer
          </Button>
        </div>
      ) : null}
    </SurfacePanel>
  );
}
