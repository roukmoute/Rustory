import type React from "react";
import { useEffect, useId, useRef, useState } from "react";

import { readNodeMedia } from "../../../ipc/commands/story";
import { Button, Field, StateChip } from "../../../shared/ui";
import type { StateChipProps } from "../../../shared/ui";
import type { AppError } from "../../../shared/errors/app-error";
import type {
  NodeMediaSlot,
  NodeMediaSlotKind,
} from "../../../shared/ipc-contracts/story";

import type { NodeSaveStatus, UseNodeEditor } from "../hooks/use-node-editor";

import "./StoryNodeEditorHost.css";

function presentNodeSaveStatus(status: NodeSaveStatus): {
  tone: StateChipProps["tone"];
  label: string;
} {
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
      const exhaustive: never = status;
      return exhaustive;
    }
  }
}

/** Humanize a byte count into a parent-friendly size (o / Ko / Mo). */
function humanizeBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} o`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} Ko`;
  return `${(bytes / (1024 * 1024)).toFixed(1).replace(".", ",")} Mo`;
}

interface MediaSlotProps {
  storyId: string;
  kind: NodeMediaSlotKind;
  heading: string;
  emptyLabel: string;
  slot: NodeMediaSlot | null;
  error: AppError | null;
  busy: boolean;
  editable: boolean;
  onAttach: () => void;
  onRemove: () => void;
}

/** One media slot (image or audio): its current state, the add/preview/replace/
 *  remove actions, and an inline `role="alert"` for a blocking issue. */
function MediaSlot({
  storyId,
  kind,
  heading,
  emptyLabel,
  slot,
  error,
  busy,
  editable,
  onAttach,
  onRemove,
}: MediaSlotProps): React.JSX.Element {
  const headingId = useId();
  const alertId = useId();
  const [preview, setPreview] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<AppError | null>(null);

  const currentAssetId = slot?.assetId ?? null;
  const latestAssetIdRef = useRef<string | null>(currentAssetId);
  latestAssetIdRef.current = currentAssetId;

  // Reset the preview whenever the slot's asset changes (a replacement /
  // removal) so an old data URL is never painted under a new slot (F10).
  useEffect(() => {
    setPreview(null);
    setPreviewError(null);
  }, [currentAssetId]);

  const showPreview = (): void => {
    if (!slot) return;
    const requestedAssetId = slot.assetId;
    setPreviewError(null);
    void readNodeMedia({ storyId, assetId: requestedAssetId })
      .then((p) => {
        // Ignore a late response whose asset is no longer the current one.
        if (latestAssetIdRef.current !== requestedAssetId) return;
        setPreview(p.dataUrl);
      })
      .catch((err: unknown) => {
        if (latestAssetIdRef.current !== requestedAssetId) return;
        setPreviewError(err as AppError);
      });
  };

  return (
    <div
      className="story-node-editor-host__media"
      role="group"
      aria-labelledby={headingId}
      aria-busy={busy}
    >
      <h3 id={headingId} className="story-node-editor-host__media-heading">
        {heading}
      </h3>
      {/* NFR3: a visible + announced acknowledgement that the media action is
          being processed (a large file can take a moment to hash/promote). */}
      <div
        className="story-node-editor-host__media-status"
        role="status"
        aria-live="polite"
      >
        {busy
          ? slot
            ? "Mise à jour du média en cours…"
            : "Ajout du média en cours…"
          : ""}
      </div>
      {slot ? (
        <div className="story-node-editor-host__media-present">
          <StateChip
            tone={slot.state === "attention" ? "warning" : "success"}
            label={
              slot.state === "attention"
                ? "Média à corriger"
                : `Média ajouté · ${humanizeBytes(slot.byteSize ?? 0)}`
            }
          />
          {slot.state === "attention" ? (
            <p className="story-node-editor-host__media-note">
              Le fichier associé n'est plus accessible. Ré-associe le média ou
              retire-le ; le reste du nœud reste modifiable.
            </p>
          ) : null}
          <div className="story-node-editor-host__media-actions">
            {slot.state === "ready" ? (
              <Button variant="quiet" onClick={showPreview}>
                Aperçu
              </Button>
            ) : null}
            {editable ? (
              <>
                <Button
                  variant="secondary"
                  onClick={onAttach}
                  disabled={busy}
                  aria-describedby={error ? alertId : undefined}
                >
                  Remplacer
                </Button>
                <Button variant="quiet" onClick={onRemove} disabled={busy}>
                  Retirer
                </Button>
              </>
            ) : null}
          </div>
          {preview && slot.state === "ready" ? (
            kind === "image" ? (
              <img
                className="story-node-editor-host__preview-image"
                src={preview}
                alt={`Aperçu de l'${heading.toLowerCase()} du nœud`}
              />
            ) : (
              <audio
                className="story-node-editor-host__preview-audio"
                src={preview}
                aria-label={`Aperçu de l'${heading.toLowerCase()} du nœud`}
                controls
              />
            )
          ) : null}
          {previewError ? (
            <p className="story-node-editor-host__media-alert" role="alert">
              Aperçu indisponible pour l'instant.
            </p>
          ) : null}
        </div>
      ) : (
        <div className="story-node-editor-host__media-empty">
          <StateChip tone="info" label={emptyLabel} />
          {editable ? (
            <Button
              variant="secondary"
              onClick={onAttach}
              disabled={busy}
              aria-describedby={error ? alertId : undefined}
            >
              Ajouter
            </Button>
          ) : null}
        </div>
      )}
      {error ? (
        <div
          id={alertId}
          className="story-node-editor-host__media-alert"
          role="alert"
        >
          <span className="story-node-editor-host__media-block">Média bloqué</span>
          <p>{error.message}</p>
          {error.userAction ? <p>{error.userAction}</p> : null}
        </div>
      ) : null}
    </div>
  );
}

export interface StoryNodeEditorHostProps {
  storyId: string;
  editor: UseNodeEditor;
  /** When true, a TITLE recovery decision is pending: the node is gated (its
   *  fields + media actions are locked and its own recovery banner is held
   *  back) so the two recovery surfaces never compete. */
  gated?: boolean;
}

/**
 * `Story Node Editor` — the current-node zone of the editor shell. Edits the
 * node text + metadata label (autosaved) and hosts the image / audio media
 * slots. For an imported story the same projection renders read-only with a
 * named reason; the editor never shows a control that cannot be saved.
 *
 * When no node is projected (a degraded structure) the zone renders a NAMED
 * empty state, never a fabricated node.
 */
export function StoryNodeEditorHost({
  storyId,
  editor,
  gated = false,
}: StoryNodeEditorHostProps): React.JSX.Element {
  const headingId = useId();
  const textId = useId();
  const recoveryId = useId();
  const presentation = presentNodeSaveStatus(editor.saveStatus);
  // The node's OWN recovery offer is surfaced only when no title recovery is
  // pending (the two never compete); while gated, editing is locked too. The
  // narrowed value keeps the banner type-safe.
  const nodeRecovery =
    !gated && editor.recovery.kind === "recoverable" ? editor.recovery : null;
  const locked = !editor.editable || gated || nodeRecovery !== null;

  if (editor.nodeId === null) {
    return (
      <section
        className="story-node-editor-host"
        aria-labelledby={headingId}
      >
        <h2 id={headingId} className="story-node-editor-host__heading">
          Nœud courant
        </h2>
        <div className="story-node-editor-host__empty" tabIndex={0}>
          Aucun nœud à éditer pour l'instant.
        </div>
      </section>
    );
  }

  return (
    <section className="story-node-editor-host" aria-labelledby={headingId}>
      <h2 id={headingId} className="story-node-editor-host__heading">
        Nœud courant
      </h2>

      {!editor.editable ? (
        <p className="story-node-editor-host__readonly" role="note">
          Histoire importée (lecture seule)
        </p>
      ) : null}

      {nodeRecovery ? (
        <div
          id={recoveryId}
          className="story-node-editor-host__recovery"
          role="alert"
        >
          <p className="story-node-editor-host__recovery-title">
            Brouillon récupéré
          </p>
          <p>Tu avais tapé : « {nodeRecovery.draftText || "(vide)"} »</p>
          <p>
            Dernier état enregistré : « {nodeRecovery.persistedText || "(vide)"} »
          </p>
          <div className="story-node-editor-host__recovery-actions">
            <Button variant="secondary" onClick={editor.applyRecovery}>
              Reprendre ce brouillon
            </Button>
            <Button variant="quiet" onClick={editor.discardRecovery}>
              Conserver l'état enregistré
            </Button>
          </div>
        </div>
      ) : null}

      <div className="story-node-editor-host__field">
        <label htmlFor={textId} className="story-node-editor-host__label">
          Texte du nœud
        </label>
        <textarea
          id={textId}
          className="story-node-editor-host__textarea"
          value={editor.text}
          placeholder="Écris le texte de ce nœud…"
          onChange={(e) => editor.setText(e.target.value)}
          disabled={locked}
          rows={4}
        />
      </div>

      <Field
        id="story-node-label"
        label="Libellé du nœud"
        value={editor.label}
        onChange={editor.setLabel}
        disabled={locked}
      />

      <div className="story-node-editor-host__save-status">
        <StateChip tone={presentation.tone} label={presentation.label} />
      </div>
      <div
        className="story-node-editor-host__save-announce"
        aria-live="polite"
        aria-atomic="true"
      >
        {editor.saveStatus.kind === "saved" ? "Enregistré" : ""}
      </div>
      {editor.saveStatus.kind === "failed" ? (
        <div className="story-node-editor-host__save-alert" role="alert">
          <p>{editor.saveStatus.error.message}</p>
          {editor.saveStatus.error.userAction ? (
            <p>{editor.saveStatus.error.userAction}</p>
          ) : null}
        </div>
      ) : null}

      <div className="story-node-editor-host__media-slots">
        <MediaSlot
          storyId={storyId}
          kind="image"
          heading="Image"
          emptyLabel="Aucune image"
          slot={editor.image}
          error={editor.imageError}
          busy={editor.imageBusy}
          editable={!locked}
          onAttach={() => editor.attachMedia("image")}
          onRemove={() => editor.removeMedia("image")}
        />
        <MediaSlot
          storyId={storyId}
          kind="audio"
          heading="Audio"
          emptyLabel="Aucun audio"
          slot={editor.audio}
          error={editor.audioError}
          busy={editor.audioBusy}
          editable={!locked}
          onAttach={() => editor.attachMedia("audio")}
          onRemove={() => editor.removeMedia("audio")}
        />
      </div>
    </section>
  );
}
