import type React from "react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import { createStory } from "../../../ipc/commands/story";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type { ContentSourcePolicy } from "../../../shared/ipc-contracts/import-export";
import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import { Button, Dialog, Field, Icon, ProgressIndicator } from "../../../shared/ui";
import {
  MAX_STORY_TITLE_CHARS,
  normalizeStoryTitle,
  reasonFor,
  validateStoryTitle,
  type StoryTitleIssue,
} from "../validation/story-title";

import "./CreateStoryDialog.css";

export interface CreateStoryDialogProps {
  /** The library uses the guided choice; classic keeps the compact all-in-one surface. */
  flow?: "classic" | "guided";
  initialStep?: "choice" | "title" | "import";
  open: boolean;
  onClose: () => void;
  onCreated: (story: StoryCardDto) => void;
  onImportArtifactRequest?: () => void;
  isImportArtifactUnavailable?: boolean;
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
  /** Start the structured-ARCHIVE creation path (`Choisir une archive de
   *  pack (.zip)…`): closes this dialog and hands over to the archive
   *  flow. Optional — when absent, the archive entry is not rendered. */
  onCreateFromArchiveRequest?: () => void;
  /** Same cross-flow exclusivity for the archive entry. */
  isCreateFromArchiveUnavailable?: boolean;
  /** Start the external-source creation path (`Démarrer depuis une source
   *  externe (RSS)`): closes this dialog and hands over to the RSS flow.
   *  Optional — when absent, the content-source section is not rendered. */
  onCreateFromRssRequest?: () => void;
  /** Same cross-flow exclusivity for the RSS entry. */
  isCreateFromRssUnavailable?: boolean;
  /** Start the web-page creation path (`Démarrer depuis une page web`):
   *  closes this dialog and hands over to the web flow. Optional — when
   *  absent, the web entry is not rendered. */
  onCreateFromWebRequest?: () => void;
  /** Same cross-flow exclusivity for the web entry. */
  isCreateFromWebUnavailable?: boolean;
  /** Cross-flow exclusivity for the PRIMARY title path: while a sibling
   *  operation that must not overlap a creation is in flight (an OS-open
   *  analysis settling), the `Créer` submission is refused — mirrors the
   *  folder/RSS entry gating. */
  isSubmitUnavailable?: boolean;
  /** Mirror of the internal `isSubmitting` state, reported upward so the
   *  route can include the PRIMARY creation in the flows' mutual
   *  exclusion (an OS-open intent arriving mid-submission must be
   *  declined calmly, never interleaved). */
  onSubmittingChange?: (submitting: boolean) => void;
  /** The distribution's content-source policy, read by the route when the
   *  dialog opens (`read_content_source_policy` — Rust alone decides; the
   *  dialog renders what it declares, never a hardcoded list). `null` /
   *  absent = the read failed or has not landed: FAIL-CLOSED — every
   *  external-source entry renders disabled with the frozen reason, never
   *  active-by-default. The title path and the folder entry are NEVER
   *  policy-gated. */
  contentSourcePolicy?: ContentSourcePolicy | null;
}

/** The frozen fail-closed reason when the policy read failed or has not
 *  landed — the only content-source copy rendered WITHOUT a successful
 *  policy read (the entry-level activation marker is Rust-carried by
 *  the policy DTO and rendered verbatim, never re-typed here). */
const POLICY_FAIL_CLOSED_REASON =
  "Sources externes indisponibles pour l'instant.";

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
  flow = "classic",
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
  const reasonId = useId();
  const counterId = useId();
  const serverErrorId = useId();
  const progressId = useId();
  const sourcesId = useId();
  const localNoteId = useId();
  const webChoiceReasonId = useId();
  const fieldRef = useRef<HTMLInputElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);

  const [step, setStep] = useState(initialStep);
  const [title, setTitle] = useState<string>("");
  const [isSubmitting, setIsSubmitting] = useState<boolean>(false);
  const [serverError, setServerError] = useState<AppError | null>(null);

  const normalized = useMemo(() => normalizeStoryTitle(title), [title]);
  const issue = useMemo<StoryTitleIssue | null>(
    () => validateStoryTitle(normalized),
    [normalized],
  );
  const charCount = useMemo(() => Array.from(normalized).length, [normalized]);
  const guided = flow === "guided";
  const webPolicy = contentSourcePolicy?.sources.find((source) => source.kind === "web");
  const webAvailable = webPolicy?.activation === "enabled" && Boolean(onCreateFromWebRequest);

  useEffect(() => {
    setStep(initialStep);
  }, [open, initialStep]);

  useEffect(() => {
    if (!open || !guided) return;
    if (step === "title") fieldRef.current?.focus();
    else contentRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [open, guided, step]);

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

  // Report the submission state upward (the reset above flows through this
  // same effect, so the route's mirror can never stick on a closed dialog).
  useEffect(() => {
    onSubmittingChange?.(isSubmitting);
  }, [isSubmitting, onSubmittingChange]);

  const canSubmit = issue === null && !isSubmitting && !isSubmitUnavailable;

  const describedBy = [
    descriptionId,
    counterId,
    guided && step === "title" ? localNoteId : null,
    issue !== null ? reasonId : null,
    serverError !== null ? serverErrorId : null,
    isSubmitting ? progressId : null,
  ]
    .filter(Boolean)
    .join(" ");

  const handleSubmit = async (): Promise<void> => {
    if (isSubmitting || isSubmitUnavailable) return;
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

  const handleCreateFromArchive = (): void => {
    // Same cross-flow exclusivity as the folder entry.
    if (isSubmitting || isCreateFromArchiveUnavailable) return;
    // Hand over to the archive flow: close this dialog first so the
    // native file picker is never stacked under a modal.
    onClose();
    onCreateFromArchiveRequest?.();
  };

  const handleCreateFromRss = (): void => {
    // Same cross-flow exclusivity as the folder entry.
    if (isSubmitting || isCreateFromRssUnavailable) return;
    // Hand over to the external-source flow: close this dialog first so
    // the in-context surface never sits under a modal.
    onClose();
    onCreateFromRssRequest?.();
  };

  const handleCreateFromWeb = (): void => {
    // Same cross-flow exclusivity as the folder entry.
    if (isSubmitting || isCreateFromWebUnavailable) return;
    // Hand over to the web-page flow: close this dialog first so
    // the in-context surface never sits under a modal.
    onClose();
    onCreateFromWebRequest?.();
  };

  const handleImportArtifact = (): void => {
    if (isSubmitting || isImportArtifactUnavailable) return;
    onClose();
    onImportArtifactRequest?.();
  };

  const dialogTitle = !guided ? "Créer une histoire"
    : step === "choice" ? "Que veux-tu faire ?"
      : step === "title" ? "Comment s'appelle ton histoire ?"
        : "Comment veux-tu importer ?";

  return (
    <Dialog
      open={open}
      onClose={handleCancel}
      title={dialogTitle}
      ariaDescribedBy={!guided || step === "title" ? descriptionId : undefined}
    >
      <div ref={contentRef} className={`create-story-dialog${guided ? " create-story-dialog--guided" : ""}`}>
        {guided && <p className="create-story-dialog__step">Étape {step === "choice" ? "1" : "2"} sur 2</p>}
        {guided && step === "choice" && (
          <>
            <div className="create-story-dialog__guided-choices">
              <Button variant="secondary" aria-label="Écrire une histoire" onClick={() => setStep("title")}>
                <strong>Écrire une histoire</strong>
                <span>Partir d'une page blanche</span>
              </Button>
              <Button variant="secondary" aria-label="Importer" onClick={() => setStep("import")}>
                <strong>Importer</strong>
                <span>Fichier, dossier ou lien</span>
              </Button>
              <Button
                variant="secondary"
                aria-label="Depuis une page web"
                aria-describedby={webChoiceReasonId}
                aria-disabled={!webAvailable || isSubmitting || isCreateFromWebUnavailable || undefined}
                onClick={handleCreateFromWeb}
              >
                <strong>Depuis une page web</strong>
                <span>Coller l'adresse d'une page</span>
              </Button>
              <p id={webChoiceReasonId} className={webAvailable ? "create-story-dialog__sr-only" : "create-story-dialog__source-reason"}>
                {webAvailable ? webPolicy?.activationMarker : webPolicy?.reason ?? POLICY_FAIL_CLOSED_REASON}
              </p>
            </div>
            <Button variant="quiet" className="create-story-dialog__cancel-choice" onClick={handleCancel}>Annuler</Button>
          </>
        )}
        {(!guided || step === "title") && (
        <div className="create-story-dialog__new-story">
          {!guided && <span className="create-story-dialog__illustration" aria-hidden="true">
            <Icon name="wand" />
          </span>}
          <p id={descriptionId} className="create-story-dialog__description">
            {guided ? "Tu pourras la changer plus tard." : "Donne un titre à ta nouvelle histoire. Rustory crée un brouillon local que tu peux éditer plus tard."}
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
            {charCount} / {MAX_STORY_TITLE_CHARS}{guided ? "" : " caractères"}
          </p>
          {guided && <p id={localNoteId} className="create-story-dialog__local-note">On crée un brouillon sur ton ordinateur. Rien n'est envoyé.</p>}
          {issue !== null && !isSubmitting ? (
            <p id={reasonId} className={`create-story-dialog__reason${guided && issue === "empty" ? " create-story-dialog__sr-only" : ""}`}>
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
              <ProgressIndicator mode="indeterminate" label="Création en cours…" />
            </div>
          ) : null}
          <div className="create-story-dialog__actions">
            <Button
              variant={guided ? "quiet" : "secondary"}
              onClick={guided ? () => setStep("choice") : handleCancel}
              aria-disabled={isSubmitting || undefined}
            >
              {guided ? "Retour" : "Annuler"}
            </Button>
            {canSubmit ? (
              <Button variant="primary" onClick={() => void handleSubmit()}>
                {!guided && <Icon name="plus" />}
                Créer
              </Button>
            ) : (
              <Button
                variant="primary"
                aria-disabled="true"
                aria-describedby={
                  issue !== null ? reasonId : isSubmitting ? progressId : undefined
                }
              >
                {!guided && <Icon name="plus" />}
                Créer
              </Button>
            )}
          </div>
        </div>
        )}
        {(!guided || step === "import") && (
        <div className="create-story-dialog__choices">
          {guided && onImportArtifactRequest && (
            <div className="create-story-dialog__folder-entry">
              <span className="create-story-dialog__choice-icon" aria-hidden="true"><Icon name="book" /></span>
              <p className="create-story-dialog__folder-hint">Une histoire exportée depuis Rustory</p>
              <Button variant="quiet" onClick={handleImportArtifact} aria-disabled={isSubmitting || isImportArtifactUnavailable || undefined}>
                Choisir un fichier .rustory…
              </Button>
            </div>
          )}
          {onCreateFromFolderRequest ? (
            <div className="create-story-dialog__folder-entry">
              <span className="create-story-dialog__choice-icon" aria-hidden="true">
                <Icon name="folder" />
              </span>
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
          {onCreateFromArchiveRequest ? (
            <div className="create-story-dialog__folder-entry create-story-dialog__folder-entry--archive">
              <span className="create-story-dialog__choice-icon" aria-hidden="true">
                <Icon name="upload" />
              </span>
              <p className="create-story-dialog__folder-hint">
                Ou importe une archive de pack au format communautaire
              </p>
              <Button
                variant="quiet"
                onClick={handleCreateFromArchive}
                aria-disabled={
                  isSubmitting || isCreateFromArchiveUnavailable || undefined
                }
              >
                Choisir une archive de pack (.zip)…
              </Button>
            </div>
          ) : null}
          {onCreateFromRssRequest || onCreateFromWebRequest ? (
            <div
              className="create-story-dialog__sources"
              role="group"
              aria-label="Sources de contenu"
            >
              {contentSourcePolicy !== null &&
              contentSourcePolicy.sources.some(
                (s) => s.kind === "rss" || s.kind === "web",
              ) ? (
                contentSourcePolicy.sources.map((entry) => {
                  const isRssEntry = entry.kind === "rss";
                  const isWebEntry = entry.kind === "web";
                  const isActionable =
                    entry.activation === "enabled" &&
                    ((isRssEntry && onCreateFromRssRequest) ||
                      (isWebEntry && onCreateFromWebRequest));
                  const subTextId = `${sourcesId}-${entry.kind}`;
                  return (
                    <div key={entry.kind} className={`create-story-dialog__source${isActionable ? " create-story-dialog__source--active" : ""}`}>
                      <span className="create-story-dialog__choice-icon" aria-hidden="true">
                        <Icon name={isWebEntry ? "link" : "headphones"} />
                      </span>
                      {isActionable ? (
                        <>
                          <Button
                            variant="quiet"
                            onClick={
                              isWebEntry
                                ? handleCreateFromWeb
                                : handleCreateFromRss
                            }
                            aria-disabled={
                              isSubmitting ||
                              (isWebEntry
                                ? isCreateFromWebUnavailable
                                : isCreateFromRssUnavailable) ||
                              undefined
                            }
                            aria-describedby={subTextId}
                          >
                            {isWebEntry
                              ? "Démarrer depuis une page web"
                              : "Démarrer depuis une source externe (RSS)"}
                          </Button>
                          <p
                            id={subTextId}
                            className="create-story-dialog__source-note"
                          >
                            <span className="create-story-dialog__source-label">
                              {entry.label}
                            </span>
                            <span className="create-story-dialog__source-marker">
                              {entry.activationMarker}
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
                              : isWebEntry
                                ? "Démarrer depuis une page web"
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
                  <span className="create-story-dialog__choice-icon" aria-hidden="true">
                    <Icon name="headphones" />
                  </span>
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
        </div>
        )}
        {guided && step === "import" && (
          <div className="create-story-dialog__actions">
            <Button variant="quiet" onClick={() => setStep("choice")}>Retour</Button>
          </div>
        )}
      </div>
    </Dialog>
  );
}
