import type React from "react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import { createStory } from "../../../ipc/commands/story";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { ContentSourcePolicy } from "../../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import { Button, Dialog, Field, ProgressIndicator } from "../../../shared/ui";
import {
  MAX_STORY_TITLE_CHARS,
  normalizeStoryTitle,
  reasonFor,
  validateStoryTitle,
  type StoryTitleIssue,
} from "../validation/story-title";

import "./CreateStoryDialog.css";

export interface CreateStoryDialogProps {
  open: boolean;
  onClose: () => void;
  onCreated: (story: StoryCardDto) => void;
  /** Start the structured-folder creation path (`Choisir un dossier…`):
   *  closes this dialog and hands over to the folder flow. Optional — when
   *  absent, the secondary entry is not rendered (the interactive path
   *  stays the whole dialog). */
  onCreateFromFolderRequest?: () => void;
  /** Cross-flow exclusivity: while ANOTHER import/creation flow is busy
   *  (a `.rustory` analysis in flight, a folder analysis/creation, an RSS
   *  fetch/creation), the folder entry is disabled — two native dialogs /
   *  review surfaces must never stack. Mirrors the library bar's import
   *  CTA gating. */
  isCreateFromFolderUnavailable?: boolean;
  /** Start the external-source creation path (`Démarrer depuis une source
   *  externe (RSS)`): closes this dialog and hands over to the RSS flow.
   *  Optional — when absent, the content-source section is not rendered. */
  onCreateFromRssRequest?: () => void;
  /** Same cross-flow exclusivity for the RSS entry. */
  isCreateFromRssUnavailable?: boolean;
  /** The distribution's content-source policy, read by the route when the
   *  dialog opens (`read_content_source_policy` — Rust alone decides; the
   *  dialog renders what it declares, never a hardcoded list). `null` /
   *  absent = the read failed or has not landed: FAIL-CLOSED — every
   *  external-source entry renders disabled with the frozen reason, never
   *  active-by-default. The title path and the folder entry are NEVER
   *  policy-gated. */
  contentSourcePolicy?: ContentSourcePolicy | null;
}

/** The frozen entry-level activation marker (`product-language.md`) —
 *  same copy family as the surface mention, distinct literal (no final
 *  period). */
const ACTIVATION_MARKER = "Activée par la distribution officielle";

/** The frozen fail-closed reason when the policy read failed or has not
 *  landed — the only content-source copy rendered WITHOUT a successful
 *  policy read (the activation marker above is a frontend literal too,
 *  but it only accompanies a successfully read `enabled` line). */
const POLICY_FAIL_CLOSED_REASON = "Sources externes indisponibles pour l'instant.";

/**
 * Modal used to collect the minimal input required to create a new local
 * story draft — the CREATION CHOICE of the library: the interactive path
 * (title → `Créer`) stays primary, and a secondary entry hands over to the
 * structured-folder flow (`Ou démarre depuis un dossier préparé hors de
 * Rustory`). Title validation runs in two redundant places: this component
 * computes a `StoryTitleIssue` for responsive feedback, and the Rust core
 * re-validates authoritatively before any SQL is executed.
 */
export function CreateStoryDialog({
  open,
  onClose,
  onCreated,
  onCreateFromFolderRequest,
  isCreateFromFolderUnavailable = false,
  onCreateFromRssRequest,
  isCreateFromRssUnavailable = false,
  contentSourcePolicy = null,
}: CreateStoryDialogProps): React.JSX.Element {
  const descriptionId = useId();
  const titleFieldId = useId();
  const reasonId = useId();
  const counterId = useId();
  const serverErrorId = useId();
  const progressId = useId();
  const sourcesId = useId();
  const fieldRef = useRef<HTMLInputElement | null>(null);

  const [title, setTitle] = useState<string>("");
  const [isSubmitting, setIsSubmitting] = useState<boolean>(false);
  const [serverError, setServerError] = useState<AppError | null>(null);

  const normalized = useMemo(() => normalizeStoryTitle(title), [title]);
  const issue = useMemo<StoryTitleIssue | null>(
    () => validateStoryTitle(normalized),
    [normalized],
  );
  const charCount = useMemo(() => Array.from(normalized).length, [normalized]);

  // Reset the typing state whenever the dialog transitions from open→closed
  // so a subsequent re-open starts from a clean slate without clobbering
  // the content the user is actively typing inside an open dialog.
  useEffect(() => {
    if (!open) {
      setTitle("");
      setServerError(null);
      setIsSubmitting(false);
    }
  }, [open]);

  const canSubmit = issue === null && !isSubmitting;

  const describedBy = [
    descriptionId,
    counterId,
    issue !== null ? reasonId : null,
    serverError !== null ? serverErrorId : null,
    isSubmitting ? progressId : null,
  ]
    .filter(Boolean)
    .join(" ");

  const handleSubmit = async (): Promise<void> => {
    if (isSubmitting) return;
    // Read the live value straight from the input instead of reusing the
    // memoized `normalized`: a keystroke that lands between React's last
    // render and this handler (Enter race) would otherwise submit a stale
    // value. We still pass the submission through to Rust even when the
    // local mirror thinks the title is invalid — Rust remains authoritative
    // and will answer with a typed `INVALID_STORY_TITLE` that the server
    // error branch below surfaces with the canonical reason.
    const liveValue = fieldRef.current?.value ?? title;
    const liveNormalized = normalizeStoryTitle(liveValue);
    setIsSubmitting(true);
    setServerError(null);
    try {
      const created = await createStory({ title: liveNormalized });
      onCreated(created);
      onClose();
    } catch (err) {
      setServerError(toAppError(err));
      // Bring the focus back onto the field so the user can correct without
      // hunting the next focus target via keyboard.
      fieldRef.current?.focus();
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleTitleChange = (next: string): void => {
    setTitle(next);
    // Any edit after a rejection means the user is correcting their input:
    // clear the stale server-side error so the role=alert banner does not
    // keep shouting about a value that no longer exists.
    if (serverError !== null) {
      setServerError(null);
    }
  };

  const handleFieldKeyDown = (
    event: React.KeyboardEvent<HTMLInputElement>,
  ): void => {
    // Submit on Enter even when the local validator currently flags an
    // issue: if the value in the DOM is actually valid (race with React's
    // render cycle), Rust accepts it; if the DOM value is invalid too,
    // Rust rejects it with a typed error. The UI never silently drops a
    // keystroke.
    if (event.key === "Enter" && !isSubmitting) {
      event.preventDefault();
      void handleSubmit();
    }
  };

  const handleCancel = (): void => {
    if (isSubmitting) return;
    onClose();
  };

  const handleCreateFromFolder = (): void => {
    // The folder handover is refused while the title submission OR any
    // sibling import/creation flow is busy (cross-flow exclusivity).
    if (isSubmitting || isCreateFromFolderUnavailable) return;
    // Hand over to the structured-folder flow: close this dialog first so
    // the native folder picker is never stacked under a modal.
    onClose();
    onCreateFromFolderRequest?.();
  };

  const handleCreateFromRss = (): void => {
    // Same cross-flow exclusivity as the folder entry.
    if (isSubmitting || isCreateFromRssUnavailable) return;
    // Hand over to the external-source flow: close this dialog first so
    // the in-context surface never sits under a modal.
    onClose();
    onCreateFromRssRequest?.();
  };

  return (
    <Dialog
      open={open}
      onClose={handleCancel}
      title="Créer une histoire"
      ariaDescribedBy={descriptionId}
    >
      <p id={descriptionId} className="create-story-dialog__description">
        Donne un titre à ta nouvelle histoire. Rustory crée un brouillon local
        que tu peux éditer plus tard.
      </p>
      <Field
        id={titleFieldId}
        label="Titre"
        value={title}
        onChange={handleTitleChange}
        placeholder="Le soleil couchant…"
        autoFocus
        aria-describedby={describedBy || undefined}
        onKeyDown={handleFieldKeyDown}
        inputRef={fieldRef}
      />
      <p
        id={counterId}
        className={[
          "create-story-dialog__counter",
          charCount > MAX_STORY_TITLE_CHARS
            ? "create-story-dialog__counter--over"
            : null,
        ]
          .filter(Boolean)
          .join(" ")}
        aria-live="polite"
      >
        {charCount} / {MAX_STORY_TITLE_CHARS} caractères
      </p>
      {issue !== null && !isSubmitting ? (
        <p id={reasonId} className="create-story-dialog__reason">
          {reasonFor(issue, { charCount })}
        </p>
      ) : null}
      {serverError !== null ? (
        <p
          id={serverErrorId}
          className="create-story-dialog__server-error"
          role="alert"
        >
          {serverError.message}
          {serverError.userAction ? ` ${serverError.userAction}` : ""}
        </p>
      ) : null}
      {isSubmitting ? (
        <div id={progressId} className="create-story-dialog__progress">
          <ProgressIndicator
            mode="indeterminate"
            label="Création en cours…"
          />
        </div>
      ) : null}
      <div className="create-story-dialog__actions">
        <Button
          variant="secondary"
          onClick={handleCancel}
          aria-disabled={isSubmitting || undefined}
        >
          Annuler
        </Button>
        {canSubmit ? (
          <Button variant="primary" onClick={() => void handleSubmit()}>
            Créer
          </Button>
        ) : (
          <Button
            variant="primary"
            aria-disabled="true"
            aria-describedby={
              issue !== null
                ? reasonId
                : isSubmitting
                  ? progressId
                  : undefined
            }
          >
            Créer
          </Button>
        )}
      </div>
      {onCreateFromFolderRequest ? (
        <div className="create-story-dialog__folder-entry">
          <p className="create-story-dialog__folder-hint">
            Ou démarre depuis un dossier préparé hors de Rustory
          </p>
          <Button
            variant="quiet"
            onClick={handleCreateFromFolder}
            aria-disabled={
              isSubmitting || isCreateFromFolderUnavailable || undefined
            }
          >
            Choisir un dossier…
          </Button>
        </div>
      ) : null}
      {onCreateFromRssRequest ? (
        <div
          className="create-story-dialog__sources"
          role="group"
          aria-label="Sources de contenu"
        >
          {contentSourcePolicy !== null &&
          contentSourcePolicy.sources.some((s) => s.kind === "rss") ? (
            contentSourcePolicy.sources.map((entry) => {
              const isRssEntry = entry.kind === "rss";
              const isActionable = isRssEntry && entry.activation === "enabled";
              const subTextId = `${sourcesId}-${entry.kind}`;
              return (
                <div key={entry.kind} className="create-story-dialog__source">
                  {isActionable ? (
                    <>
                      <Button
                        variant="quiet"
                        onClick={handleCreateFromRss}
                        aria-disabled={
                          isSubmitting || isCreateFromRssUnavailable || undefined
                        }
                        aria-describedby={subTextId}
                      >
                        Démarrer depuis une source externe (RSS)
                      </Button>
                      <p id={subTextId} className="create-story-dialog__source-note">
                        <span className="create-story-dialog__source-label">
                          {entry.label}
                        </span>
                        <span className="create-story-dialog__source-marker">
                          {ACTIVATION_MARKER}
                        </span>
                      </p>
                    </>
                  ) : (
                    <>
                      {/* A non-enabled kind (or a non-RSS kind — no
                          ingestion flow exists for it) renders VISIBLE but
                          DISABLED, its Rust-carried reason reachable from
                          the keyboard (Disabled Actions pattern). The
                          reason-less fallback (an enabled non-RSS kind —
                          refused upstream by the policy guard) stays
                          honest: the fail-closed reason, NEVER the
                          activation marker on a disabled entry. */}
                      <Button
                        variant="quiet"
                        aria-disabled="true"
                        aria-describedby={subTextId}
                      >
                        {isRssEntry
                          ? "Démarrer depuis une source externe (RSS)"
                          : entry.label}
                      </Button>
                      <p
                        id={subTextId}
                        className="create-story-dialog__source-reason"
                      >
                        {entry.reason ?? POLICY_FAIL_CLOSED_REASON}
                      </p>
                    </>
                  )}
                </div>
              );
            })
          ) : (
            <div className="create-story-dialog__source">
              {/* FAIL-CLOSED: no readable policy (absent, failed read, or a
                  policy without the rss line) — the external-source entry
                  renders disabled with the frozen frontend-owned reason,
                  never active-by-default. The title path above is intact. */}
              <Button
                variant="quiet"
                aria-disabled="true"
                aria-describedby={`${sourcesId}-fail-closed`}
              >
                Démarrer depuis une source externe (RSS)
              </Button>
              <p
                id={`${sourcesId}-fail-closed`}
                className="create-story-dialog__source-reason"
              >
                {POLICY_FAIL_CLOSED_REASON}
              </p>
            </div>
          )}
        </div>
      ) : null}
    </Dialog>
  );
}
