import type React from "react";
import { useEffect, useRef } from "react";

import { ExportStatusSurface } from "../../import-export/components/ExportStatusSurface";
import { ExportStoryButton } from "../../import-export/components/ExportStoryButton";
import type { UseStoryExport } from "../../import-export/hooks/use-story-export";
import { Button, Field, StateChip } from "../../../shared/ui";
import type { StateChipProps } from "../../../shared/ui";
import type { AppError } from "../../../shared/errors/app-error";
import type { StoryDetailDto } from "../../../shared/ipc-contracts/story";

import { RecoveryBanner } from "./RecoveryBanner";
import { RecoveryReadErrorBanner } from "./RecoveryReadErrorBanner";
import { StoryNodeEditorHost } from "./StoryNodeEditorHost";
import { StoryStructureNavigator } from "./StoryStructureNavigator";
import type { SaveStatus } from "../hooks/use-story-editor";
import type { UseNodeEditor } from "../hooks/use-node-editor";
import type { UseStoryRecovery } from "../hooks/use-story-recovery";

import "./StoryEditorShell.css";

/**
 * Re-phrase a save-time `AppError` so the message uses the "Enregistrement"
 * vocabulary expected on the edit surface. The Rust core reuses the canonical
 * "Création impossible: …" strings from the dialog flow to keep a single
 * validation code path; here the user is updating an existing draft, not
 * creating one, so the prefix would be misleading.
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
      // Exhaustiveness check: a new SaveStatus variant must be handled here
      // explicitly, not fall through to a stale default.
      const exhaustive: never = status;
      return exhaustive;
    }
  }
}

export interface StoryEditorShellProps {
  detail: StoryDetailDto;
  draftTitle: string;
  saveStatus: SaveStatus;
  recovery: UseStoryRecovery;
  exporter: UseStoryExport;
  /** Current-node editor (text/metadata autosave + media actions). */
  nodeEditor: UseNodeEditor;
  onSetDraftTitle: (next: string) => void;
  onRetrySave: () => void;
  /** Flush BOTH the title and node autosaves before the export boundary opens. */
  onFlushAutoSave: () => void;
  /** Leave the editor (the route owns the recovery guard + flush + navigate). */
  onBack: () => void;
}

/**
 * `Story Editor Shell` — the dedicated editing screen (UX-DR21), separate
 * from the library. It coexists three zones at once (AC1): the global
 * `Story Structure Navigator`, the current-node host, and the story state +
 * actions. The v1 canonical model is empty, so the two content zones render
 * NAMED empty states — the shell ships the frame, not the node model.
 *
 * The title autosave, draft recovery, and export behaviors are preserved
 * verbatim from the previous title-only surface (NFR6 / NFR8): the recovery
 * banner keeps priority over the editable field, the save failure stays a
 * `role="alert"` (never a toast), and `onFlushAutoSave` runs before export /
 * `Retour` so a mid-debounce keystroke is never lost.
 */
export function StoryEditorShell({
  detail,
  draftTitle,
  saveStatus,
  recovery,
  exporter,
  nodeEditor,
  onSetDraftTitle,
  onRetrySave,
  onFlushAutoSave,
  onBack,
}: StoryEditorShellProps): React.JSX.Element {
  const presentation = presentSaveStatus(saveStatus);
  const saveStatusId = "story-edit-save-status";
  const saveAlertId = "story-edit-save-alert";

  // Title field focus management. `autoFocus` only fires at mount, so a field
  // that was disabled behind a recovery surface never regained focus once the
  // surface dismissed (previously deferred). A `ref` + effect focuses the field
  // at mount AND on every transition back to an editable state.
  const titleFieldRef = useRef<HTMLInputElement>(null);

  // The recovery banner takes precedence over the editable Field: the user
  // must commit a decision (Apply / Discard / Retry / Dismiss) before resuming
  // typing. While ANY recovery surface — INCLUDING the initial-load probe — is
  // on screen, the Field is disabled, the ExportStoryButton is disabled, and
  // the autosave chip is paused.
  //
  // The `loading` branch matters: a keystroke between Field mount and
  // `readRecoverableDraft` resolution would schedule a `recordDraft` 150 ms
  // debounce, which can land on the row before the banner mounts and silently
  // overwrite the recoverable buffer.
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
  // `error + draft === null` keeps the diff banner pure (two truths visible,
  // never empty).
  const recoveryReadError =
    recovery.state.kind === "error" && recovery.state.draft === null
      ? recovery.state.error
      : null;

  useEffect(() => {
    // Focus the title field when it is (re)enabled — at mount with no recovery
    // surface, and when a recovery surface dismisses. Focusing a disabled input
    // is a no-op, so the guard keeps focus off the field while a surface is up.
    if (!recoveryActive) {
      titleFieldRef.current?.focus();
    }
  }, [recoveryActive]);

  return (
    <main className="story-editor-shell" aria-label="Éditeur d'histoire">
      <section className="story-editor-shell__state">
        {recoveryDraft ? (
          <RecoveryBanner
            draft={recoveryDraft}
            applyingIntent={
              recovery.state.kind === "applying" ? recovery.state.intent : null
            }
            error={recovery.state.kind === "error" ? recovery.state.error : null}
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
          // P17 placeholder: the Field is disabled while we probe for a
          // recoverable draft. A small status line explains the wait without
          // mounting a full banner — the probe usually settles in <100 ms.
          // It carries the SAME id the recovery banners use so the Field's
          // `aria-describedby` (set to "story-edit-recovery-banner" whenever a
          // recovery surface is up) resolves to a real element during the
          // initial probe too — AT keeps the reason the Field is locked.
          <p
            id="story-edit-recovery-banner"
            className="story-editor-shell__recovery-loading"
            role="status"
            aria-live="polite"
          >
            Vérification d'un brouillon récupérable…
          </p>
        ) : null}
        {/* The H1 mirrors the PERSISTED title (source of truth), not the live
            draft — the latter would re-announce at every keystroke and
            misrepresent what is actually saved. The editable Field below
            carries the draft. */}
        <h1 className="story-editor-shell__title">{detail.title}</h1>
        <p className="story-editor-shell__message">
          Tu reprends le dernier brouillon local de cette histoire. L'appareil
          n'est pas consulté.
        </p>
        <div className="story-editor-shell__editor">
          <Field
            id="story-title"
            label="Titre de l'histoire"
            value={draftTitle}
            onChange={onSetDraftTitle}
            disabled={recoveryActive}
            inputRef={titleFieldRef}
            // P37/D2: when a recovery surface is on screen, point
            // `aria-describedby` at the banner so AT users hear the reason
            // the Field is locked. Otherwise keep the existing wiring (save
            // status + alert).
            aria-describedby={
              recoveryActive
                ? "story-edit-recovery-banner"
                : saveStatus.kind === "failed"
                  ? `${saveStatusId} ${saveAlertId}`
                  : saveStatusId
            }
          />
          {/* The chip itself is a passive visual surface — no `aria-live` to
              avoid announcing the fugitive `pending` and `saving` transitions
              that add no useful signal for AT users. */}
          <div id={saveStatusId} className="story-editor-shell__save-status">
            <StateChip tone={presentation.tone} label={presentation.label} />
          </div>
          <div
            className="story-editor-shell__save-announce"
            aria-live="polite"
            aria-atomic="true"
          >
            {saveStatus.kind === "saved" ? "Enregistré" : ""}
          </div>
        </div>
        {saveStatus.kind === "failed" ? (
          <div
            id={saveAlertId}
            className="story-editor-shell__save-alert"
            role="alert"
          >
            <p className="story-editor-shell__save-alert-message">
              {saveErrorMessage(saveStatus.error)}
            </p>
            {saveStatus.error.userAction ? (
              <p className="story-editor-shell__save-alert-action">
                {saveStatus.error.userAction}
              </p>
            ) : null}
            <Button variant="secondary" onClick={onRetrySave}>
              Réessayer l'enregistrement
            </Button>
          </div>
        ) : null}
      </section>

      {/* The two content zones coexist with the state bandeau (AC1). The
          navigator and the node editor both consume the node PROJECTED by Rust
          (`detail.node`), never re-parsing `structureJson`. */}
      <div className="story-editor-shell__zones">
        <StoryStructureNavigator
          title={detail.title}
          node={detail.node}
          currentNodeId={nodeEditor.nodeId}
        />
        <StoryNodeEditorHost
          storyId={detail.id}
          editor={nodeEditor}
          gated={recoveryActive}
        />
      </div>

      <section className="story-editor-shell__actions-region">
        <ExportStatusSurface
          status={exporter.status}
          onRetry={() => {
            void exporter.retryExport();
          }}
          onDismiss={exporter.dismissStatus}
        />
        <div className="story-editor-shell__actions">
          <ExportStoryButton
            storyId={detail.id}
            // Pass the LIVE draft title rather than the persisted one so the
            // save-dialog suggestion reflects what the user has actually
            // typed. The trim/NFC normalization matches the form that Rust
            // will have persisted once `flushAutoSave` settles.
            storyTitle={draftTitle.trim().normalize("NFC")}
            exporter={exporter}
            onBeforeTrigger={onFlushAutoSave}
            disabled={recoveryActive}
          />
          <Button variant="quiet" onClick={onBack}>
            Retour à la bibliothèque
          </Button>
        </div>
      </section>
    </main>
  );
}
