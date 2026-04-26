import type React from "react";

import { Button } from "../../../shared/ui";
import type { AppError } from "../../../shared/errors/app-error";
import type { RecoverableDraft } from "../../../shared/ipc-contracts/story";
import {
  formatRecoveryDisplay,
  type FormattedRecoveryDisplay,
} from "../lib/format-recovery-display";
import { formatRelativeTime } from "../lib/format-relative-time";

import "./RecoveryBanner.css";

/** Render a formatted display value as the JSX node the banner inserts
 *  inside its `<dd>` slots. Empty / whitespace fallbacks render as an
 *  italic phrase distinguishable from a real empty-quotes glyph. */
function renderDisplay(value: FormattedRecoveryDisplay): React.JSX.Element {
  if (value.kind === "empty") {
    return <em className="recovery-banner__empty">(vide)</em>;
  }
  if (value.kind === "whitespace") {
    return <em className="recovery-banner__empty">(espaces)</em>;
  }
  // `dir="auto"` lets the browser apply the first-strong heuristic on
  // each rendered string independently — a French label preceding a
  // Hebrew title will still render the title with its native direction
  // without flipping the surrounding layout.
  return (
    <strong dir="auto">{`"${value.text}"`}</strong>
  );
}

type RecoveredDraftPayload = Extract<RecoverableDraft, { kind: "recoverable" }>;

/**
 * Rephrase a recovery-time `AppError` so the user-facing copy uses
 * the "Restauration" vocabulary. The Rust core reuses the canonical
 * `Création impossible: …` strings to keep the validation rules
 * single-sourced; on the recovery surface that prefix would mislead
 * the user into thinking the error came from the create dialog. The
 * other AppError code wordings already reference the recovery
 * context and pass through unchanged.
 */
function recoveryErrorMessage(error: AppError): string {
  return error.message.replace(
    /^Création impossible\b/,
    "Restauration impossible",
  );
}

export interface RecoveryBannerProps {
  draft: RecoveredDraftPayload;
  /** When non-null, the banner is in flight: a Rust-side IPC is
   *  resolving. The discriminant tells the banner which copy to show
   *  on the primary action (`Restauration en cours…` for `apply`,
   *  `Suppression en cours…` for `discard`) — without it, a discard
   *  in flight would mislead the user with restore wording. */
  applyingIntent?: "apply" | "discard" | null;
  /** Optional error from a previous apply/discard attempt. When non-null,
   *  the banner renders a `role="alert"` block under the diff with a
   *  "Réessayer la récupération" button. */
  error?: AppError | null;
  onApply: () => void;
  onDiscard: () => void;
  /** Optional retry handler — only used when `error` is set. Without it,
   *  the alert still renders but the user has to use Apply / Discard
   *  to move on. */
  onRetry?: () => void;
}

/**
 * Inline, sober recovery banner that owns AC1's "two visible truths"
 * contract: the persisted title (last-saved value) and the buffered
 * draft (what the user typed before the interruption). Renders above
 * the editable Field; the Field is expected to be `disabled` while the
 * banner is on screen so the user commits a decision before resuming.
 */
export function RecoveryBanner({
  draft,
  applyingIntent,
  error,
  onApply,
  onDiscard,
  onRetry,
}: RecoveryBannerProps): React.JSX.Element {
  const isApplying = applyingIntent !== null && applyingIntent !== undefined;
  const primaryLabel =
    applyingIntent === "apply"
      ? "Restauration en cours…"
      : applyingIntent === "discard"
        ? "Suppression en cours…"
        : "Restaurer le brouillon";
  const secondaryLabel =
    applyingIntent === "discard"
      ? "Suppression en cours…"
      : "Conserver l'état enregistré";
  return (
    <section
      // P37/D2: stable id so the surrounding route can wire its
      // disabled `Field` `aria-describedby` to this region. Keeps
      // AT users informed that the field is locked because of a
      // pending recovery decision rather than for an opaque reason.
      id="story-edit-recovery-banner"
      role="region"
      aria-label="Brouillon récupéré"
      aria-busy={isApplying}
      className="ds-surface ds-surface--elevation-1 recovery-banner"
    >
      <h2 className="recovery-banner__title">Brouillon récupéré</h2>
      <p className="recovery-banner__intro">
        Choisis comment reprendre cette histoire.
      </p>
      <dl className="recovery-banner__diff">
        <div className="recovery-banner__diff-row">
          <dt>Tu avais tapé :</dt>
          <dd>{renderDisplay(formatRecoveryDisplay(draft.draftTitle))}</dd>
        </div>
        <div className="recovery-banner__diff-row">
          <dt>Dernier état enregistré :</dt>
          <dd>
            {renderDisplay(formatRecoveryDisplay(draft.persistedTitle))}
          </dd>
        </div>
      </dl>
      <p className="recovery-banner__when">
        Brouillon enregistré {formatRelativeTime(draft.draftAt)}.
      </p>
      <div className="recovery-banner__actions">
        <Button
          variant="primary"
          onClick={onApply}
          disabled={isApplying}
          aria-disabled={isApplying}
          aria-busy={applyingIntent === "apply"}
          // P30: move focus onto the primary action when the banner
          // mounts. Without an explicit focus move, the keyboard user
          // would lose context — the previously-focused Field is now
          // disabled, the focus falls back to <body>, and the user
          // has to tab in. autoFocus on the primary action also
          // implies the AT will read the action label first.
          autoFocus
        >
          {primaryLabel}
        </Button>
        <Button
          variant="secondary"
          onClick={onDiscard}
          disabled={isApplying}
          aria-disabled={isApplying}
          aria-busy={applyingIntent === "discard"}
        >
          {secondaryLabel}
        </Button>
      </div>
      {error ? (
        <div className="recovery-banner__alert" role="alert">
          <p className="recovery-banner__alert-message">
            {recoveryErrorMessage(error)}
          </p>
          {error.userAction ? (
            <p className="recovery-banner__alert-action">{error.userAction}</p>
          ) : null}
          {onRetry ? (
            <Button variant="secondary" onClick={onRetry} disabled={isApplying}>
              Réessayer la récupération
            </Button>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
