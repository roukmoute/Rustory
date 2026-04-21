import type React from "react";
import { useCallback, useEffect, useRef, useState } from "react";

import { LibraryFiltersNav } from "../../features/library/components/LibraryFiltersNav";
import { LuniiDecisionPanel } from "../../features/library/components/LuniiDecisionPanel";
import { StoryCollection } from "../../features/library/components/StoryCollection";
import { getLibraryOverview } from "../../ipc/commands/library";
import { LibraryLayout } from "../../shell/layout/LibraryLayout";
import { toAppError, type AppError } from "../../shared/errors/app-error";
import {
  isLibraryOverviewDto,
  type LibraryOverviewDto,
} from "../../shared/ipc-contracts/library";
import { Button } from "../../shared/ui";
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

interface LibraryErrorBannerProps {
  error: AppError;
  onRetry: () => void;
}

function LibraryErrorBanner({
  error,
  onRetry,
}: LibraryErrorBannerProps): React.JSX.Element {
  // `role="alert"` already implies `aria-live="assertive"` — mixing it with
  // `polite` produces contradictory behavior on several screen readers.
  return (
    <section className="library-route__error" role="alert">
      <p className="library-route__error-badge" aria-hidden="true">
        !
      </p>
      <div className="library-route__error-body">
        <h1 className="library-route__error-title">
          Bibliothèque indisponible
        </h1>
        <p className="library-route__error-message">{error.message}</p>
        {error.userAction ? (
          <p className="library-route__error-action">{error.userAction}</p>
        ) : null}
        <Button variant="secondary" onClick={onRetry}>
          Réessayer
        </Button>
      </div>
    </section>
  );
}

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

  const center =
    state.kind === "error" ? (
      <LibraryErrorBanner error={state.error} onRetry={load} />
    ) : (
      <StoryCollection
        stories={state.kind === "ready" ? state.overview.stories : []}
        isLoading={state.kind === "loading"}
      />
    );

  return (
    <LibraryLayout
      leftNav={<LibraryFiltersNav />}
      center={center}
      rightPanel={<LuniiDecisionPanel deviceState="absent" />}
    />
  );
}
