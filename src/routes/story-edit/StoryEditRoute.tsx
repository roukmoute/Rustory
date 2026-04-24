import type React from "react";
import { useMemo } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { LibraryErrorBanner } from "../../features/library/components/LibraryErrorBanner";
import { useStoryEditor, type SaveStatus } from "../../features/story-editor/hooks/use-story-editor";
import {
  Button,
  Field,
  ProgressIndicator,
  StateChip,
  SurfacePanel,
} from "../../shared/ui";
import type { StateChipProps } from "../../shared/ui";
import type { AppError } from "../../shared/errors/app-error";

import "./StoryEditRoute.css";

/**
 * Re-phrase a save-time `AppError` so the message uses the
 * "Enregistrement" vocabulary expected on the edit surface. The Rust
 * core reuses the canonical "Création impossible: …" strings from the
 * dialog flow to keep a single validation code path; on this surface
 * the user is updating an existing draft, not creating one, so the
 * prefix would be misleading.
 */
function saveErrorMessage(error: AppError): string {
  return error.message.replace(
    /^Création impossible\b/,
    "Enregistrement impossible",
  );
}

interface SaveStatusPresentation {
  tone: StateChipProps["tone"];
  label: string;
}

function presentSaveStatus(status: SaveStatus): SaveStatusPresentation {
  switch (status.kind) {
    case "idle":
      return { tone: "info", label: "Brouillon local" };
    case "pending":
      return { tone: "neutral", label: "Modifications en attente" };
    case "saving":
      return { tone: "neutral", label: "Enregistrement…" };
    case "saved":
      return { tone: "success", label: "Enregistré" };
    case "failed":
      return { tone: "error", label: "Enregistrement en échec" };
    default: {
      // Exhaustiveness check: a new SaveStatus variant must be handled
      // here explicitly, not fall through to a stale default.
      const exhaustive: never = status;
      return exhaustive;
    }
  }
}

export function StoryEditRoute(): React.JSX.Element {
  const { storyId: rawStoryId } = useParams<{ storyId: string }>();
  const navigate = useNavigate();

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

  const editor = useStoryEditor(storyId);
  const { state } = editor;

  const goBack = (): void => {
    // Commit a pending autosave before the route unmounts: clicking Retour
    // at millisecond 499 of the debounce must not lose the change.
    editor.flushAutoSave();
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
          onRetry={editor.retry}
          title={title}
        />
        <Button variant="secondary" onClick={goBack}>
          Retour à la bibliothèque
        </Button>
      </main>
    );
  }

  if (state.kind === "not-found") {
    return (
      <main
        className="story-edit-route story-edit-route--missing"
        aria-label="Histoire introuvable"
      >
        <SurfacePanel
          elevation={1}
          as="section"
          className="story-edit-route__card"
        >
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

  const presentation = presentSaveStatus(state.saveStatus);
  const saveStatusId = "story-edit-save-status";
  const saveAlertId = "story-edit-save-alert";

  return (
    <main
      className="story-edit-route"
      aria-label="Reprise d'un brouillon local"
    >
      <SurfacePanel
        elevation={1}
        as="section"
        className="story-edit-route__card"
      >
        {/* The H1 mirrors the PERSISTED title (source of truth), not
            the live draft — the latter would re-announce at every
            keystroke and misrepresent what is actually saved. The
            editable Field below carries the draft. */}
        <h1 className="story-edit-route__title">{state.detail.title}</h1>
        <p className="story-edit-route__message">
          Tu reprends le dernier brouillon local de cette histoire. L'appareil
          n'est pas consulté.
        </p>
        <div className="story-edit-route__editor">
          <Field
            id="story-title"
            label="Titre de l'histoire"
            value={state.draftTitle}
            onChange={editor.setDraftTitle}
            autoFocus
            aria-describedby={
              state.saveStatus.kind === "failed"
                ? `${saveStatusId} ${saveAlertId}`
                : saveStatusId
            }
          />
          {/* The chip itself is a passive visual surface — no
              `aria-live` to avoid announcing the fugitive `pending` and
              `saving` transitions that add no useful signal for AT
              users. The canonical contract only announces `saved` (via
              the sibling polite region below) and `failed` (via the
              `role="alert"` region that renders conditionally). */}
          <div
            id={saveStatusId}
            className="story-edit-route__save-status"
          >
            <StateChip tone={presentation.tone} label={presentation.label} />
          </div>
          <div
            className="story-edit-route__save-announce"
            aria-live="polite"
            aria-atomic="true"
          >
            {state.saveStatus.kind === "saved" ? "Enregistré" : ""}
          </div>
        </div>
        {state.saveStatus.kind === "failed" ? (
          <div
            id={saveAlertId}
            className="story-edit-route__save-alert"
            role="alert"
          >
            <p className="story-edit-route__save-alert-message">
              {saveErrorMessage(state.saveStatus.error)}
            </p>
            {state.saveStatus.error.userAction ? (
              <p className="story-edit-route__save-alert-action">
                {state.saveStatus.error.userAction}
              </p>
            ) : null}
            <Button variant="secondary" onClick={editor.retrySave}>
              Réessayer l'enregistrement
            </Button>
          </div>
        ) : null}
        <Button variant="quiet" onClick={goBack}>
          Retour à la bibliothèque
        </Button>
      </SurfacePanel>
    </main>
  );
}
