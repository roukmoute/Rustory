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

type CreationStep = "choice" | "title" | "import" | "web";

export interface CreateStoryDialogProps {
  initialStep?: "choice" | "title" | "import";
  open: boolean;
  onClose: () => void;
  onCreated: (story: StoryCardDto) => void;
  /** Each import handover closes this dialog before opening its own surface. */
  onImportArtifactRequest?: () => void;
  isImportArtifactUnavailable?: boolean;
  onCreateFromFolderRequest?: () => void;
  isCreateFromFolderUnavailable?: boolean;
  onCreateFromArchiveRequest?: () => void;
  isCreateFromArchiveUnavailable?: boolean;
  onCreateFromRssRequest?: () => void;
  isCreateFromRssUnavailable?: boolean;
  /** Called with the trimmed address only after closing this dialog. */
  onCreateFromWebRequest?: (url: string) => void;
  isCreateFromWebUnavailable?: boolean;
  /** A sibling operation must not overlap the local draft creation. */
  isSubmitUnavailable?: boolean;
  onSubmittingChange?: (submitting: boolean) => void;
  /** Rust owns the source policy. Missing or unreadable policy fails closed;
   *  local title, artifact, folder and archive paths remain independent. */
  contentSourcePolicy?: ContentSourcePolicy | null;
}

const POLICY_FAIL_CLOSED_REASON =
  "Sources externes indisponibles pour l'instant.";
const BUSY_REASON =
  "Une opération est déjà en cours. Termine-la avant de continuer.";

/** One creation choice followed by the input needed for the chosen path. */
export function CreateStoryDialog({
  initialStep = "choice",
  open,
  onClose,
  onCreated,
  onImportArtifactRequest,
  isImportArtifactUnavailable = false,
  onCreateFromFolderRequest,
  isCreateFromFolderUnavailable = false,
  onCreateFromArchiveRequest,
  isCreateFromArchiveUnavailable = false,
  onCreateFromRssRequest,
  isCreateFromRssUnavailable = false,
  onCreateFromWebRequest,
  isCreateFromWebUnavailable = false,
  isSubmitUnavailable = false,
  onSubmittingChange,
  contentSourcePolicy = null,
}: CreateStoryDialogProps): React.JSX.Element {
  const descriptionId = useId();
  const titleFieldId = useId();
  const titleReasonId = useId();
  const counterId = useId();
  const serverErrorId = useId();
  const progressId = useId();
  const localNoteId = useId();
  const busyReasonId = useId();
  const webFieldId = useId();
  const webPolicyId = useId();
  const webEmptyReasonId = useId();
  const rssPolicyId = useId();
  const artifactReasonId = useId();
  const folderReasonId = useId();
  const archiveReasonId = useId();
  const fieldRef = useRef<HTMLInputElement | null>(null);
  const webFieldRef = useRef<HTMLInputElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);

  const [step, setStep] = useState<CreationStep>(initialStep);
  const [title, setTitle] = useState("");
  const [pageUrl, setPageUrl] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [serverError, setServerError] = useState<AppError | null>(null);

  const normalized = useMemo(() => normalizeStoryTitle(title), [title]);
  const issue = useMemo<StoryTitleIssue | null>(
    () => validateStoryTitle(normalized),
    [normalized],
  );
  const charCount = useMemo(() => Array.from(normalized).length, [normalized]);
  const webPolicy = contentSourcePolicy?.sources.find(
    (source) => source.kind === "web",
  );
  const rssPolicy = contentSourcePolicy?.sources.find(
    (source) => source.kind === "rss",
  );
  const webAvailable =
    webPolicy?.activation === "enabled" && Boolean(onCreateFromWebRequest);
  const rssAvailable =
    rssPolicy?.activation === "enabled" && Boolean(onCreateFromRssRequest);
  const webBusy = isSubmitting || isCreateFromWebUnavailable;
  const rssBusy = isSubmitting || isCreateFromRssUnavailable;
  const webReason = !webAvailable
    ? (webPolicy?.reason ?? POLICY_FAIL_CLOSED_REASON)
    : webBusy
      ? BUSY_REASON
      : undefined;
  const rssReason = !rssAvailable
    ? (rssPolicy?.reason ?? POLICY_FAIL_CLOSED_REASON)
    : rssBusy
      ? BUSY_REASON
      : undefined;

  useEffect(() => {
    setStep(initialStep);
  }, [open, initialStep]);

  useEffect(() => {
    if (!open) return;
    if (step === "title") fieldRef.current?.focus();
    else if (step === "web") webFieldRef.current?.focus();
    else
      contentRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [open, step]);

  // Back keeps the entered values; closing starts the next creation afresh.
  useEffect(() => {
    if (!open) {
      setTitle("");
      setPageUrl("");
      setServerError(null);
      setIsSubmitting(false);
    }
  }, [open]);

  useEffect(() => {
    onSubmittingChange?.(isSubmitting);
  }, [isSubmitting, onSubmittingChange]);

  const canSubmit = issue === null && !isSubmitting && !isSubmitUnavailable;
  const describedBy = [
    descriptionId,
    localNoteId,
    counterId,
    issue !== null && !isSubmitting ? titleReasonId : null,
    serverError !== null ? serverErrorId : null,
    isSubmitting ? progressId : null,
    isSubmitUnavailable ? busyReasonId : null,
  ]
    .filter(Boolean)
    .join(" ");

  const handleSubmit = async (): Promise<void> => {
    if (isSubmitting || isSubmitUnavailable) return;
    // Read the live input for the Enter race. Rust remains authoritative
    // even when React's ergonomic validation has not caught up yet.
    const liveNormalized = normalizeStoryTitle(
      fieldRef.current?.value ?? title,
    );
    setIsSubmitting(true);
    setServerError(null);
    try {
      const created = await createStory({ title: liveNormalized });
      onCreated(created);
      onClose();
    } catch (err) {
      setServerError(toAppError(err));
      fieldRef.current?.focus();
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleTitleChange = (next: string): void => {
    setTitle(next);
    if (serverError !== null) setServerError(null);
  };

  const handleFieldKeyDown = (
    event: React.KeyboardEvent<HTMLInputElement>,
  ): void => {
    if (event.key === "Enter" && !isSubmitting) {
      event.preventDefault();
      void handleSubmit();
    }
  };

  const handleCancel = (): void => {
    if (!isSubmitting) onClose();
  };
  const handleBack = (): void => {
    if (!isSubmitting) setStep("choice");
  };
  const handOver = (
    request: (() => void) | undefined,
    unavailable: boolean,
  ): void => {
    if (!request || isSubmitting || unavailable) return;
    onClose();
    request();
  };
  const handleWebSubmit = (): void => {
    const url = (webFieldRef.current?.value ?? pageUrl).trim();
    if (!url || !webAvailable || webBusy || !onCreateFromWebRequest) return;
    onClose();
    onCreateFromWebRequest(url);
  };

  const dialogTitle =
    step === "choice"
      ? "Que veux-tu faire ?"
      : step === "title"
        ? "Comment s'appelle ton histoire ?"
        : step === "import"
          ? "Comment veux-tu importer ?"
          : "Depuis une page web";

  return (
    <Dialog
      open={open}
      onClose={handleCancel}
      dismissible={!isSubmitting}
      eyebrow={`Étape ${step === "choice" ? "1" : "2"} sur 2`}
      title={dialogTitle}
      ariaDescribedBy={step === "title" ? descriptionId : undefined}
    >
      <div ref={contentRef} className="create-story-dialog">
        {step === "choice" && (
          <>
            <div className="create-story-dialog__choices">
              <Button
                variant="secondary"
                className="create-story-dialog__card"
                aria-label="Écrire une histoire"
                onClick={() => setStep("title")}
              >
                <strong>Écrire une histoire</strong>
                <span>Partir d'une page blanche</span>
              </Button>
              <Button
                variant="secondary"
                className="create-story-dialog__card"
                aria-label="Importer"
                onClick={() => setStep("import")}
              >
                <strong>Importer</strong>
                <span>Fichier, dossier ou lien</span>
              </Button>
              <div className="create-story-dialog__source">
                <Button
                  variant="secondary"
                  className="create-story-dialog__card"
                  aria-label="Depuis une page web"
                  aria-describedby={webPolicyId}
                  aria-disabled={!webAvailable || webBusy || undefined}
                  onClick={() => setStep("web")}
                >
                  <strong>Depuis une page web</strong>
                  <span>Coller l'adresse d'une page</span>
                </Button>
                <p
                  id={webPolicyId}
                  className={
                    webReason
                      ? "create-story-dialog__reason"
                      : "create-story-dialog__sr-only"
                  }
                >
                  {webReason ?? webPolicy?.activationMarker}
                </p>
              </div>
            </div>
            <Button
              variant="quiet"
              className="create-story-dialog__cancel"
              onClick={handleCancel}
            >
              Annuler
            </Button>
          </>
        )}
        {step === "title" && (
          <div className="create-story-dialog__field-step">
            <p id={descriptionId} className="create-story-dialog__description">
              Tu pourras le changer plus tard.
            </p>
            <Field
              id={titleFieldId}
              label="Titre"
              value={title}
              onChange={handleTitleChange}
              placeholder="Le soleil couchant…"
              autoFocus
              aria-describedby={describedBy}
              onKeyDown={handleFieldKeyDown}
              inputRef={fieldRef}
            />
            <div className="create-story-dialog__field-notes">
              <p id={localNoteId}>
                On crée un brouillon sur ton ordinateur. Rien n'est envoyé.
              </p>
              <p
                id={counterId}
                className={`create-story-dialog__counter${charCount > MAX_STORY_TITLE_CHARS ? " create-story-dialog__counter--over" : ""}`}
                aria-live="polite"
              >
                {charCount} / {MAX_STORY_TITLE_CHARS}
              </p>
            </div>
            {issue !== null && !isSubmitting && (
              <p
                id={titleReasonId}
                className={
                  issue === "empty"
                    ? "create-story-dialog__sr-only"
                    : "create-story-dialog__reason"
                }
              >
                {reasonFor(issue, { charCount })}
              </p>
            )}
            {isSubmitUnavailable && (
              <p id={busyReasonId} className="create-story-dialog__reason">
                {BUSY_REASON}
              </p>
            )}
            {serverError !== null && (
              <p
                id={serverErrorId}
                className="create-story-dialog__error"
                role="alert"
              >
                {serverError.message}
                {serverError.userAction ? ` ${serverError.userAction}` : ""}
              </p>
            )}
            {isSubmitting && (
              <div id={progressId} className="create-story-dialog__progress">
                <ProgressIndicator
                  mode="indeterminate"
                  label="Création en cours…"
                />
              </div>
            )}
            <div className="create-story-dialog__actions">
              <Button
                variant="quiet"
                onClick={handleBack}
                aria-disabled={isSubmitting || undefined}
              >
                Retour
              </Button>
              <Button
                variant="primary"
                onClick={() => void handleSubmit()}
                aria-disabled={!canSubmit || undefined}
                aria-describedby={
                  !canSubmit
                    ? isSubmitting
                      ? progressId
                      : issue !== null
                        ? titleReasonId
                        : busyReasonId
                    : undefined
                }
              >
                Créer
              </Button>
            </div>
          </div>
        )}
        {step === "import" && (
          <>
            <div className="create-story-dialog__choices">
              {onImportArtifactRequest && (
                <div className="create-story-dialog__source">
                  <Button
                    variant="secondary"
                    className="create-story-dialog__card"
                    aria-label="Choisir un fichier .rustory…"
                    onClick={() =>
                      handOver(
                        onImportArtifactRequest,
                        isImportArtifactUnavailable,
                      )
                    }
                    aria-disabled={
                      isSubmitting || isImportArtifactUnavailable || undefined
                    }
                    aria-describedby={
                      isImportArtifactUnavailable ? artifactReasonId : undefined
                    }
                  >
                    <strong>Choisir un fichier .rustory…</strong>
                    <span>Une histoire exportée depuis Rustory</span>
                  </Button>
                  {isImportArtifactUnavailable && (
                    <p
                      id={artifactReasonId}
                      className="create-story-dialog__reason"
                    >
                      {BUSY_REASON}
                    </p>
                  )}
                </div>
              )}
              {onCreateFromFolderRequest && (
                <div className="create-story-dialog__source">
                  <Button
                    variant="secondary"
                    className="create-story-dialog__card"
                    aria-label="Choisir un dossier…"
                    onClick={() =>
                      handOver(
                        onCreateFromFolderRequest,
                        isCreateFromFolderUnavailable,
                      )
                    }
                    aria-disabled={
                      isSubmitting || isCreateFromFolderUnavailable || undefined
                    }
                    aria-describedby={
                      isCreateFromFolderUnavailable ? folderReasonId : undefined
                    }
                  >
                    <strong>Choisir un dossier…</strong>
                    <span>Un dossier préparé hors de Rustory</span>
                  </Button>
                  {isCreateFromFolderUnavailable && (
                    <p
                      id={folderReasonId}
                      className="create-story-dialog__reason"
                    >
                      {BUSY_REASON}
                    </p>
                  )}
                </div>
              )}
              {onCreateFromArchiveRequest && (
                <div className="create-story-dialog__source">
                  <Button
                    variant="secondary"
                    className="create-story-dialog__card"
                    aria-label="Choisir une archive de pack (.zip)…"
                    onClick={() =>
                      handOver(
                        onCreateFromArchiveRequest,
                        isCreateFromArchiveUnavailable,
                      )
                    }
                    aria-disabled={
                      isSubmitting ||
                      isCreateFromArchiveUnavailable ||
                      undefined
                    }
                    aria-describedby={
                      isCreateFromArchiveUnavailable
                        ? archiveReasonId
                        : undefined
                    }
                  >
                    <strong>Choisir une archive de pack (.zip)…</strong>
                    <span>Une archive de pack au format communautaire</span>
                  </Button>
                  {isCreateFromArchiveUnavailable && (
                    <p
                      id={archiveReasonId}
                      className="create-story-dialog__reason"
                    >
                      {BUSY_REASON}
                    </p>
                  )}
                </div>
              )}
              {onCreateFromRssRequest && (
                <div className="create-story-dialog__source">
                  <Button
                    variant="secondary"
                    className="create-story-dialog__card"
                    aria-label="Démarrer depuis une source externe (RSS)"
                    onClick={() =>
                      handOver(onCreateFromRssRequest, !rssAvailable || rssBusy)
                    }
                    aria-disabled={!rssAvailable || rssBusy || undefined}
                    aria-describedby={rssPolicyId}
                  >
                    <strong>Démarrer depuis une source externe (RSS)</strong>
                    <span>Un flux de podcast ou d'épisodes audio</span>
                  </Button>
                  <p
                    id={rssPolicyId}
                    className={
                      rssReason
                        ? "create-story-dialog__reason"
                        : "create-story-dialog__sr-only"
                    }
                  >
                    {rssReason ?? rssPolicy?.activationMarker}
                  </p>
                </div>
              )}
            </div>
            <div className="create-story-dialog__actions">
              <Button variant="quiet" onClick={handleBack}>
                Retour
              </Button>
            </div>
          </>
        )}
        {step === "web" && (
          <div className="create-story-dialog__field-step">
            <Field
              id={webFieldId}
              label="Adresse de la page web"
              type="url"
              value={pageUrl}
              onChange={setPageUrl}
              placeholder="https://…"
              autoFocus
              inputRef={webFieldRef}
              aria-describedby={[
                webPolicyId,
                !pageUrl.trim() ? webEmptyReasonId : null,
              ]
                .filter(Boolean)
                .join(" ")}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  handleWebSubmit();
                }
              }}
            />
            <p
              id={webPolicyId}
              className={
                webReason
                  ? "create-story-dialog__reason"
                  : "create-story-dialog__sr-only"
              }
            >
              {webReason ?? webPolicy?.activationMarker}
            </p>
            {!pageUrl.trim() && (
              <p id={webEmptyReasonId} className="create-story-dialog__sr-only">
                Renseigne l'adresse de la page web.
              </p>
            )}
            <div className="create-story-dialog__actions">
              <Button variant="quiet" onClick={handleBack}>
                Retour
              </Button>
              <Button
                variant="primary"
                onClick={handleWebSubmit}
                aria-disabled={
                  !pageUrl.trim() || !webAvailable || webBusy || undefined
                }
                aria-describedby={
                  webReason
                    ? webPolicyId
                    : !pageUrl.trim()
                      ? webEmptyReasonId
                      : undefined
                }
              >
                Récupérer la page
              </Button>
            </div>
          </div>
        )}
      </div>
    </Dialog>
  );
}
