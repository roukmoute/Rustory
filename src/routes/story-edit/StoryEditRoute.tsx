import type React from "react";
import { useMemo } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { LibraryErrorBanner } from "../../features/library/components/LibraryErrorBanner";
import { useLibraryOverview } from "../../features/library/hooks/use-library-overview";
import { Button, ProgressIndicator, StateChip, SurfacePanel } from "../../shared/ui";

import "./StoryEditRoute.css";

export function StoryEditRoute(): React.JSX.Element {
  const { storyId: rawStoryId } = useParams<{ storyId: string }>();
  const navigate = useNavigate();
  const { state, retry } = useLibraryOverview();

  // The library encodes ids with encodeURIComponent before pushing them into
  // the URL — decode here before comparing against canonical ids. A malformed
  // encoding (rare) falls back to the raw value; the "introuvable" branch
  // still catches it cleanly.
  const storyId = useMemo(() => {
    if (!rawStoryId) return undefined;
    try {
      return decodeURIComponent(rawStoryId);
    } catch {
      return rawStoryId;
    }
  }, [rawStoryId]);

  const goBack = (): void => {
    // `replace` keeps the browser history a single in/out transition for
    // the library ↔ edit context — back button behavior stays predictable.
    navigate("/library", { replace: true });
  };

  if (state.kind === "loading") {
    return (
      <main
        className="story-edit-route story-edit-route--loading"
        aria-label="Chargement du brouillon"
      >
        <div
          className="story-edit-route__status"
          role="status"
          aria-live="polite"
        >
          <ProgressIndicator
            mode="indeterminate"
            label="Chargement du brouillon local…"
          />
        </div>
      </main>
    );
  }

  if (state.kind === "error") {
    const title =
      state.error.code === "LIBRARY_INCONSISTENT"
        ? "Bibliothèque incohérente, recharge nécessaire"
        : "Reprise indisponible";
    return (
      <main className="story-edit-route" aria-label="Erreur de chargement">
        <LibraryErrorBanner
          error={state.error}
          onRetry={retry}
          title={title}
        />
        <Button variant="secondary" onClick={goBack}>
          Retour à la bibliothèque
        </Button>
      </main>
    );
  }

  const story = state.overview.stories.find((s) => s.id === storyId);

  if (!story) {
    return (
      <main
        className="story-edit-route story-edit-route--missing"
        aria-label="Histoire introuvable"
      >
        <SurfacePanel elevation={1} as="section" className="story-edit-route__card">
          <h1 className="story-edit-route__title">Histoire introuvable</h1>
          <p className="story-edit-route__message">
            Cette histoire n'est plus dans ta bibliothèque locale.
          </p>
          <Button variant="secondary" onClick={goBack}>
            Retour à la bibliothèque
          </Button>
        </SurfacePanel>
      </main>
    );
  }

  return (
    <main
      className="story-edit-route"
      aria-label="Reprise d'un brouillon local"
    >
      <SurfacePanel elevation={1} as="section" className="story-edit-route__card">
        <h1 className="story-edit-route__title">{story.title}</h1>
        <StateChip tone="info" label="Brouillon local" />
        <p className="story-edit-route__message">
          Tu reprends le dernier brouillon local de cette histoire. L'appareil
          n'est pas consulté.
        </p>
        <Button variant="secondary" onClick={goBack}>
          Retour à la bibliothèque
        </Button>
      </SurfacePanel>
    </main>
  );
}
