import type React from "react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import { createStory } from "../../../ipc/commands/story";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
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
}

/**
 * Modal used to collect the minimal input required to create a new local
 * story draft. Title validation runs in two redundant places: this component
 * computes a `StoryTitleIssue` for responsive feedback, and the Rust core
 * re-validates authoritatively before any SQL is executed.
 */
export function CreateStoryDialog({
  open,
  onClose,
  onCreated,
}: CreateStoryDialogProps): React.JSX.Element {
  const descriptionId = useId();
  const titleFieldId = useId();
  const reasonId = useId();
  const counterId = useId();
  const serverErrorId = useId();
  const progressId = useId();
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
          {reasonFor(issue)}
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
    </Dialog>
  );
}
