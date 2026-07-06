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
  AnalyzedFolderVerdict,
  StructuredCreationStatus,
} from "../hooks/use-structured-creation";

import "./CreateFromFolderSurface.css";

export interface CreateFromFolderSurfaceProps {
  status: StructuredCreationStatus;
  /** Commit the analyzed folder (`Créer l'histoire`). */
  onAccept: () => void;
  /** Abandon the analyzed folder (pure frontend, no mutation). */
  onAbandon: () => void;
  /** Re-open the folder picker after a failure (`Réessayer`). */
  onRetry: () => void;
  /** Dismiss a terminal status back to idle (`Fermer`). */
  onDismiss: () => void;
}

/**
 * In-context surface for the structured-folder creation flow (`Création
 * depuis un dossier`), mirroring the `ImportArtifactSurface` discipline:
 * never a toast for a problem, never a modal, `role="alert"` for a blocked
 * / failed state, `aria-live="polite"` for the report + success. Renders
 * nothing while idle. The absolute folder path NEVER renders — only its
 * basename does.
 */
export function CreateFromFolderSurface({
  status,
  onAccept,
  onAbandon,
  onRetry,
  onDismiss,
}: CreateFromFolderSurfaceProps): React.JSX.Element | null {
  if (status.kind === "idle") return null;

  return (
    <section
      className="create-from-folder"
      aria-label="Création depuis un dossier"
    >
      {/* Polite region mounted while the surface is shown so AT picks up
          the success announcement atomically. */}
      <div
        className="create-from-folder__live"
        aria-live="polite"
        aria-atomic="true"
      >
        {status.kind === "created"
          ? "Histoire créée dans ta bibliothèque"
          : ""}
      </div>

      {status.kind === "analyzing" ? (
        <div className="create-from-folder__pending">
          <ProgressIndicator mode="indeterminate" label="Analyse du dossier…" />
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
        <div className="create-from-folder__pending">
          <ProgressIndicator mode="indeterminate" label="Création en cours…" />
        </div>
      ) : null}

      {status.kind === "created" ? (
        <div className="create-from-folder__success">
          <StateChip
            tone="success"
            label="Histoire créée dans ta bibliothèque"
          />
          <p className="create-from-folder__success-title">
            {status.story.title}
          </p>
          <Button variant="quiet" onClick={onDismiss}>
            Fermer
          </Button>
        </div>
      ) : null}

      {status.kind === "failed" ? (
        <div className="create-from-folder__alert" role="alert">
          <p className="create-from-folder__alert-title">
            Création impossible
          </p>
          <p className="create-from-folder__alert-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="create-from-folder__alert-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="create-from-folder__actions">
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

/** The recognition report. A creatable verdict is a calm `aria-live`
 *  region with the UNIQUE CTA `Créer l'histoire` (the report already says
 *  what will be discarded) then `Abandonner`; a blocked one is a
 *  `role="alert"` with `Abandonner` only (nothing to create). */
function ReviewReport({
  verdict,
  onAccept,
  onAbandon,
}: {
  verdict: AnalyzedFolderVerdict;
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
      className="create-from-folder__review"
      role={creatable ? undefined : "alert"}
      aria-live={creatable ? "polite" : undefined}
    >
      <StateChip
        tone={qualityTone(verdict.quality)}
        label={qualityLabel(verdict.quality)}
      />
      <p className="create-from-folder__folder-name">{verdict.folderName}</p>

      {recognized.length > 0 ? (
        <section className="create-from-folder__group">
          <p className="create-from-folder__group-heading">
            Ce que Rustory a reconnu
          </p>
          <ul className="create-from-folder__findings">
            {recognized.map((finding) => (
              <FindingItem key={findingKey(finding)} finding={finding} />
            ))}
          </ul>
        </section>
      ) : null}

      {attention.length > 0 ? (
        <section className="create-from-folder__group">
          <p className="create-from-folder__group-heading">
            Points d'attention
          </p>
          <ul className="create-from-folder__findings">
            {attention.map((finding) => (
              <FindingItem key={findingKey(finding)} finding={finding} />
            ))}
          </ul>
        </section>
      ) : null}

      {verdict.creatableSummary ? (
        <CreatableSummarySection summary={verdict.creatableSummary} />
      ) : null}

      <div className="create-from-folder__actions">
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

/** What an accepted folder WILL create — the normalized title, the node
 *  count, and the retained/discarded media BY BASENAME (the per-file
 *  detail lives here only; the persisted findings stay aggregated). */
function CreatableSummarySection({
  summary,
}: {
  summary: CreatableSummary;
}): React.JSX.Element {
  return (
    <section className="create-from-folder__group">
      <p className="create-from-folder__group-heading">Ce qui sera créé</p>
      <ul className="create-from-folder__summary">
        <li className="create-from-folder__summary-line">
          Titre : {summary.title}
        </li>
        <li className="create-from-folder__summary-line">
          {summary.nodeCount} {summary.nodeCount > 1 ? "nœuds" : "nœud"}
        </li>
        {summary.retainedMedia.length > 0 ? (
          <li className="create-from-folder__summary-line">
            Médias retenus : {summary.retainedMedia.join(", ")}
          </li>
        ) : null}
        {summary.discardedMedia.length > 0 ? (
          <li className="create-from-folder__summary-line">
            Médias écartés : {summary.discardedMedia.join(", ")}
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
    <li className="create-from-folder__finding">
      <StateChip
        tone={categoryTone(finding.category)}
        label={categoryLabel(finding.category)}
        className="create-from-folder__finding-chip"
      />
      <span className="create-from-folder__finding-message">
        {finding.message}
      </span>
    </li>
  );
}

function findingKey(finding: ImportFinding): string {
  return `${finding.aspect}-${finding.category}`;
}
