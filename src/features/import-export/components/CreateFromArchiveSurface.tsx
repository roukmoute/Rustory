import type React from "react";

import { Button, ProgressIndicator, StateChip } from "../../../shared/ui";
import type {
  CreatableSummary,
  ImportFinding,
} from "../../../shared/ipc-contracts/import-export";
import {
  categoryLabel,
  categoryTone,
  qualityLabel,
  qualityTone,
} from "../lib/recognition-labels";
import type {
  AnalyzedArchiveVerdict,
  ArchiveCreationStatus,
} from "../hooks/use-archive-creation";

import "./CreateFromArchiveSurface.css";

export interface CreateFromArchiveSurfaceProps {
  status: ArchiveCreationStatus;
  /** Commit the analyzed archive (`Créer l'histoire`). */
  onAccept: () => void;
  /** Abandon the analyzed archive (pure frontend, no mutation). */
  onAbandon: () => void;
  /** Re-open the archive picker after a failure (`Réessayer`). */
  onRetry: () => void;
  /** Dismiss a terminal status back to idle (`Fermer`). */
  onDismiss: () => void;
}

/**
 * In-context surface for the structured-archive creation flow (`Création
 * depuis une archive de pack`), the CreateFromFolderSurface discipline:
 * never a toast for a problem, never a modal, `role="alert"` for a blocked
 * / failed state, `aria-live="polite"` for the report + success. Renders
 * nothing while idle. The absolute archive path NEVER renders — only its
 * basename does.
 */
export function CreateFromArchiveSurface({
  status,
  onAccept,
  onAbandon,
  onRetry,
  onDismiss,
}: CreateFromArchiveSurfaceProps): React.JSX.Element | null {
  if (status.kind === "idle") return null;

  return (
    <section
      className="create-from-archive"
      aria-label="Création depuis une archive de pack"
    >
      {/* Polite region mounted while the surface is shown so AT picks up
          the success announcement atomically. */}
      <div
        className="create-from-archive__live"
        aria-live="polite"
        aria-atomic="true"
      >
        {status.kind === "created"
          ? "Histoire créée dans ta bibliothèque"
          : ""}
      </div>

      {status.kind === "analyzing" ? (
        <div className="create-from-archive__pending">
          <ProgressIndicator
            mode="indeterminate"
            label="Analyse de l'archive…"
          />
        </div>
      ) : null}

      {status.kind === "review" ? (
        <ReviewReport
          verdict={status.verdict}
          onAccept={onAccept}
          onAbandon={onAbandon}
        />
      ) : null}

      {status.kind === "creating" ? (
        <div className="create-from-archive__pending">
          <ProgressIndicator mode="indeterminate" label="Création en cours…" />
        </div>
      ) : null}

      {status.kind === "created" ? (
        <div className="create-from-archive__success">
          <StateChip
            tone="success"
            label="Histoire créée dans ta bibliothèque"
          />
          <p className="create-from-archive__success-title">
            {status.story.title}
          </p>
          <Button variant="quiet" onClick={onDismiss}>
            Fermer
          </Button>
        </div>
      ) : null}

      {status.kind === "failed" ? (
        <div className="create-from-archive__alert" role="alert">
          <p className="create-from-archive__alert-title">
            Création impossible
          </p>
          <p className="create-from-archive__alert-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="create-from-archive__alert-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="create-from-archive__actions">
            <Button variant="secondary" onClick={onRetry}>
              Réessayer
            </Button>
            <Button variant="quiet" onClick={onDismiss}>
              Fermer
            </Button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

/** The recognition report — the folder surface's exact shape: a creatable
 *  verdict is a calm `aria-live` region with the UNIQUE CTA `Créer
 *  l'histoire` then `Abandonner`; a blocked one is a `role="alert"` with
 *  `Abandonner` only. */
function ReviewReport({
  verdict,
  onAccept,
  onAbandon,
}: {
  verdict: AnalyzedArchiveVerdict;
  onAccept: () => void;
  onAbandon: () => void;
}): React.JSX.Element {
  const creatable = verdict.creatableSummary !== undefined;
  const recognized = verdict.findings.filter(
    (f) => f.category === "recognized",
  );
  const attention = verdict.findings.filter(
    (f) => f.category !== "recognized",
  );

  return (
    <div
      className="create-from-archive__review"
      role={creatable ? undefined : "alert"}
      aria-live={creatable ? "polite" : undefined}
    >
      <StateChip
        tone={qualityTone(verdict.quality)}
        label={qualityLabel(verdict.quality)}
      />
      <p className="create-from-archive__archive-name">
        {verdict.archiveName}
      </p>

      {recognized.length > 0 ? (
        <section className="create-from-archive__group">
          <p className="create-from-archive__group-heading">
            Ce que Rustory a reconnu
          </p>
          <ul className="create-from-archive__findings">
            {recognized.map((finding) => (
              <FindingItem key={findingKey(finding)} finding={finding} />
            ))}
          </ul>
        </section>
      ) : null}

      {attention.length > 0 ? (
        <section className="create-from-archive__group">
          <p className="create-from-archive__group-heading">
            Points d'attention
          </p>
          <ul className="create-from-archive__findings">
            {attention.map((finding) => (
              <FindingItem key={findingKey(finding)} finding={finding} />
            ))}
          </ul>
        </section>
      ) : null}

      {verdict.creatableSummary ? (
        <CreatableSummarySection summary={verdict.creatableSummary} />
      ) : null}

      <div className="create-from-archive__actions">
        {creatable ? (
          <Button variant="primary" onClick={onAccept}>
            Créer l'histoire
          </Button>
        ) : null}
        <Button variant="quiet" onClick={onAbandon}>
          Abandonner
        </Button>
      </div>
    </div>
  );
}

/** Beyond this many discarded names, the list collapses to a count — a
 *  community archive names its files by content hash, so a long list of
 *  40-char basenames reads as noise, not information. */
const MAX_LISTED_DISCARDED_MEDIA = 8;

/** What an accepted archive WILL create — the normalized title, the node
 *  count, and the media as COUNTS: archive basenames are content hashes
 *  (never human-chosen names), so listing them verbatim would be noise.
 *  Discarded media keep their names while the list stays short (they are
 *  the actionable part of the report). */
function CreatableSummarySection({
  summary,
}: {
  summary: CreatableSummary;
}): React.JSX.Element {
  const retainedCount = summary.retainedMedia.length;
  const discardedCount = summary.discardedMedia.length;
  return (
    <section className="create-from-archive__group">
      <p className="create-from-archive__group-heading">Ce qui sera créé</p>
      <ul className="create-from-archive__summary">
        <li className="create-from-archive__summary-line">
          Titre : {summary.title}
        </li>
        <li className="create-from-archive__summary-line">
          {summary.nodeCount} {summary.nodeCount > 1 ? "nœuds" : "nœud"}
        </li>
        {retainedCount > 0 ? (
          <li className="create-from-archive__summary-line">
            {retainedCount > 1
              ? `Médias retenus : ${retainedCount} fichiers`
              : "Média retenu : 1 fichier"}
          </li>
        ) : null}
        {discardedCount > 0 ? (
          <li className="create-from-archive__summary-line">
            {discardedCount <= MAX_LISTED_DISCARDED_MEDIA
              ? `Médias écartés : ${summary.discardedMedia.join(", ")}`
              : `Médias écartés : ${discardedCount} fichiers`}
          </li>
        ) : null}
      </ul>
    </section>
  );
}

function FindingItem({
  finding,
}: {
  finding: ImportFinding;
}): React.JSX.Element {
  return (
    <li className="create-from-archive__finding">
      <StateChip
        tone={categoryTone(finding.category)}
        label={categoryLabel(finding.category)}
        className="create-from-archive__finding-chip"
      />
      <span className="create-from-archive__finding-message">
        {finding.message}
      </span>
    </li>
  );
}

function findingKey(finding: ImportFinding): string {
  return `${finding.aspect}-${finding.category}`;
}
