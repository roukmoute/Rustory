import type React from "react";
import { useMemo } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { ExportStatusSurface } from "../../features/import-export/components/ExportStatusSurface";
import { ExportStoryButton } from "../../features/import-export/components/ExportStoryButton";
import { useStoryExport } from "../../features/import-export/hooks/use-story-export";
import { LibraryErrorBanner } from "../../features/library/components/LibraryErrorBanner";
import { RecoveryBanner } from "../../features/story-editor/components/RecoveryBanner";
import { RecoveryReadErrorBanner } from "../../features/story-editor/components/RecoveryReadErrorBanner";
import { useStoryEditor, type SaveStatus } from "../../features/story-editor/hooks/use-story-editor";
import { useStoryRecovery } from "../../features/story-editor/hooks/use-story-recovery";
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
  const exporter = useStoryExport();
  const recovery = useStoryRecovery(storyId, {
    onApplied: (output) => {
      // The Rust core already persisted the recovered title. Patch
      // the in-memory editor snapshot in place — no follow-up
      // get_story_detail round-trip needed.
      editor.reloadDetailFromOutput(output);
    },
  });
  const { state } = editor;

  const goBack = (): void => {
    // Block the navigation while a recovery apply / discard is in
    // flight: navigating mid-transaction would unmount the hook,
    // strand the IPC, and either commit the recovered title without
    // its UI ack or drop a row that the user just confirmed should be
    // dropped — both states are confusing. The button surface should
    // already be disabled by `recoveryActive`, but a programmatic
    // call (keyboard shortcut, browser back) must also no-op here.
    if (recovery.state.kind === "applying") return;
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

  // The recovery banner takes precedence over the editable Field: the
  // user must commit a decision (Apply / Discard / Retry / Dismiss)
  // before resuming typing. While ANY recovery surface — INCLUDING
  // the initial-load probe — is on screen, the Field is disabled, the
  // ExportStoryButton is disabled, and the autosave chip is paused.
  //
  // The `loading` branch matters: a keystroke between Field mount and
  // `readRecoverableDraft` resolution would schedule a `recordDraft`
  // 150 ms debounce, which can land on the row before the banner
  // mounts and silently overwrite the recoverable buffer.
  const recoveryActive =
    recovery.state.kind === "loading" ||
    recovery.state.kind === "recoverable" ||
    recovery.state.kind === "applying" ||
    recovery.state.kind === "error";
  const recoveryDraft =
    recovery.state.kind === "recoverable"
      ? recovery.state.draft
      : recovery.state.kind === "applying"
        ? recovery.state.draft
        : recovery.state.kind === "error"
          ? recovery.state.draft
          : null;
  // An initial-read error has no draft attached; render the dedicated
  // read-error banner instead of the diff banner. Discriminating on
  // `error + draft === null` keeps the diff banner pure (two truths
  // visible, never empty).
  const recoveryReadError =
    recovery.state.kind === "error" && recovery.state.draft === null
      ? recovery.state.error
      : null;

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
        {recoveryDraft ? (
          <RecoveryBanner
            draft={recoveryDraft}
            applyingIntent={
              recovery.state.kind === "applying"
                ? recovery.state.intent
                : null
            }
            error={
              recovery.state.kind === "error" ? recovery.state.error : null
            }
            onApply={recovery.apply}
            onDiscard={recovery.discard}
            onRetry={recovery.retry}
          />
        ) : recoveryReadError ? (
          <RecoveryReadErrorBanner
            error={recoveryReadError}
            onRetry={recovery.retry}
            onDismiss={recovery.dismissReadError}
          />
        ) : recovery.state.kind === "loading" ? (
          // P17 placeholder: the Field is disabled while we probe for
          // a recoverable draft. Showing nothing would leave the user
          // wondering why the Field will not accept input. A small
          // status line explains the wait without mounting a full
          // banner — the probe usually settles in <100 ms.
          <p
            className="story-edit-route__recovery-loading"
            role="status"
            aria-live="polite"
          >
            Vérification d'un brouillon récupérable…
          </p>
        ) : null}
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
            disabled={recoveryActive}
            autoFocus={!recoveryActive}
            // P37/D2: when a recovery surface is on screen, point
            // `aria-describedby` at the banner so AT users hear the
            // reason the Field is locked. Otherwise keep the existing
            // wiring (save status + alert).
            aria-describedby={
              recoveryActive
                ? "story-edit-recovery-banner"
                : state.saveStatus.kind === "failed"
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
        <ExportStatusSurface
          status={exporter.status}
          onRetry={() => {
            void exporter.retryExport();
          }}
          onDismiss={exporter.dismissStatus}
        />
        <div className="story-edit-route__actions">
          <ExportStoryButton
            storyId={state.detail.id}
            // Pass the LIVE draft title rather than the persisted one
            // so the save-dialog suggestion reflects what the user has
            // actually typed. The trim/NFC normalization matches the
            // form that Rust will have persisted once `flushAutoSave`
            // settles below.
            storyTitle={state.draftTitle.trim().normalize("NFC")}
            exporter={exporter}
            onBeforeTrigger={editor.flushAutoSave}
            disabled={recoveryActive}
          />
          <Button variant="quiet" onClick={goBack}>
            Retour à la bibliothèque
          </Button>
        </div>
      </SurfacePanel>
    </main>
  );
}
