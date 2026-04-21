import type React from "react";
import { useCallback, useEffect, useRef, useState } from "react";

import { getLibraryOverview } from "../../ipc/commands/library";
import { toAppError, type AppError } from "../../shared/errors/app-error";
import {
  isLibraryOverviewDto,
  type LibraryOverviewDto,
} from "../../shared/ipc-contracts/library";
import "./LibraryRoute.css";

type LibraryState =
  | { kind: "loading" }
  | { kind: "ready"; overview: LibraryOverviewDto }
  | { kind: "error"; error: AppError };

const MALFORMED_OVERVIEW_ERROR: AppError = {
  code: "UNKNOWN",
  message:
    "Rustory a reçu une réponse inattendue pour la bibliothèque locale.",
  userAction:
    "Relance l'application. Si le problème persiste, signale-le avec les traces locales.",
  details: null,
};

export function LibraryRoute(): React.JSX.Element {
  const [state, setState] = useState<LibraryState>({ kind: "loading" });
  // Guards against late IPC responses landing after unmount / re-mount
  // (StrictMode double-invokes effects in dev, and the user may press
  // Réessayer before a previous call resolves).
  const activeCallRef = useRef(0);
  const mountedRef = useRef(true);

  const load = useCallback(() => {
    const callId = ++activeCallRef.current;
    setState({ kind: "loading" });

    getLibraryOverview()
      .then((overview) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        if (!isLibraryOverviewDto(overview)) {
          setState({
            kind: "error",
            error: MALFORMED_OVERVIEW_ERROR,
          });
          return;
        }
        setState({ kind: "ready", overview });
      })
      .catch((err) => {
        if (!mountedRef.current || callId !== activeCallRef.current) return;
        setState({ kind: "error", error: toAppError(err) });
      });
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
    };
  }, [load]);

  if (state.kind === "loading") {
    return (
      <section className="library-route" aria-busy="true">
        <p className="library-route__status">Chargement de la bibliothèque…</p>
      </section>
    );
  }

  if (state.kind === "error") {
    // `role="alert"` already implies `aria-live="assertive"` — mixing it
    // with `polite` produces contradictory behavior on several screen
    // readers. Keep the role and drop the explicit live region.
    return (
      <section
        className="library-route library-route--error"
        role="alert"
      >
        <h1 className="library-route__title">
          Bibliothèque indisponible
        </h1>
        <p className="library-route__message">{state.error.message}</p>
        {state.error.userAction ? (
          <p className="library-route__action">{state.error.userAction}</p>
        ) : null}
        <button
          type="button"
          className="library-route__retry"
          onClick={load}
        >
          Réessayer
        </button>
      </section>
    );
  }

  const { overview } = state;

  if (overview.stories.length === 0) {
    // The button is kept focusable (no `disabled` attribute) so keyboard
    // users can reach it and read the disabled reason — `disabled` strips
    // the element from the tab order and hides `title` from assistive tech.
    // The disabled reason is visible inline, never only in a tooltip.
    const disabledReason =
      "Création d'histoire indisponible pour l'instant.";
    return (
      <section className="library-route library-route--empty">
        <h1 className="library-route__title">Ta bibliothèque est vide</h1>
        <p className="library-route__hint">
          Crée ta première histoire pour la retrouver ici à chaque ouverture.
        </p>
        <button
          type="button"
          className="library-route__primary"
          aria-disabled="true"
          aria-describedby="create-story-disabled-reason"
          onClick={(event) => event.preventDefault()}
        >
          Créer une histoire
        </button>
        <p
          id="create-story-disabled-reason"
          className="library-route__hint-secondary"
        >
          {disabledReason}
        </p>
      </section>
    );
  }

  return (
    <section className="library-route">
      <h1 className="library-route__title">Bibliothèque</h1>
      <ul className="library-route__list">
        {overview.stories.map((story) => (
          <li key={story.id} className="library-route__card">
            {story.title}
          </li>
        ))}
      </ul>
    </section>
  );
}
