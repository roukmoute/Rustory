import type React from "react";

import { Button, ProgressIndicator, StateChip } from "../../../shared/ui";
import type { ImportFinding } from "../../../shared/ipc-contracts/import-export";
import {
  categoryLabel,
  categoryTone,
  qualityLabel,
  qualityTone,
} from "../lib/recognition-labels";
import type {
  AnalyzedVerdict,
  StoryImportStatus,
} from "../hooks/use-story-import";

import "./ImportArtifactSurface.css";

export interface ImportArtifactSurfaceProps {
  status: StoryImportStatus;
  /** Commit the recognized story (`Importer ce qui est reconnu`). */
  onAccept: () => void;
  /** Abandon the analyzed import (pure frontend, no mutation). */
  onAbandon: () => void;
  /** Re-open the file picker after a failure (`Réessayer`). */
  onRetry: () => void;
  /** Dismiss a terminal status back to idle (`Fermer`). */
  onDismiss: () => void;
}

/**
 * In-context surface for the two-phase local-artifact import, mirroring the
 * `DeviceImportStatusSurface` discipline: never a toast for a problem, never
 * a modal, `role="alert"` for a blocking / failed state, `aria-live="polite"`
 * for the analysis report + success. Renders nothing while idle.
 */
export function ImportArtifactSurface({
  status,
  onAccept,
  onAbandon,
  onRetry,
  onDismiss,
}: ImportArtifactSurfaceProps): React.JSX.Element | null {
  if (status.kind === "idle") return null;

  return (
    <section className="import-artifact" aria-label="Import d'une histoire">
      {/* Polite region mounted while the surface is shown so AT picks up the
          success announcement atomically. */}
      <div
        className="import-artifact__live"
        aria-live="polite"
        aria-atomic="true"
      >
        {status.kind === "imported"
          ? "Histoire importée dans ta bibliothèque"
          : ""}
      </div>

      {status.kind === "analyzing" ? (
        <div className="import-artifact__pending">
          <ProgressIndicator
            mode="indeterminate"
            label="Analyse de l'artefact…"
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

      {status.kind === "importing" ? (
        <div className="import-artifact__pending">
          <ProgressIndicator mode="indeterminate" label="Import en cours…" />
        </div>
      ) : null}

      {status.kind === "imported" ? (
        <div className="import-artifact__success">
          <StateChip
            tone="success"
            label="Histoire importée dans ta bibliothèque"
          />
          <p className="import-artifact__success-title">
            {status.story.title}
          </p>
          <Button variant="quiet" onClick={onDismiss}>
            Fermer
          </Button>
        </div>
      ) : null}

      {status.kind === "failed" ? (
        <div className="import-artifact__alert" role="alert">
          <p className="import-artifact__alert-title">Import impossible</p>
          <p className="import-artifact__alert-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="import-artifact__alert-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="import-artifact__actions">
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

/** The recognition report. An importable verdict is a calm `aria-live`
 *  region with `Importer ce qui est reconnu` / `Abandonner`; a blocked one
 *  is a `role="alert"` with `Abandonner` only (nothing to commit). */
function ReviewReport({
  verdict,
  onAccept,
  onAbandon,
}: {
  verdict: AnalyzedVerdict;
  onAccept: () => void;
  onAbandon: () => void;
}): React.JSX.Element {
  const importable = verdict.importableContent !== undefined;
  const recognized = verdict.findings.filter(
    (f) => f.category === "recognized",
  );
  const attention = verdict.findings.filter(
    (f) => f.category !== "recognized",
  );

  return (
    <div
      className="import-artifact__review"
      role={importable ? undefined : "alert"}
      aria-live={importable ? "polite" : undefined}
    >
      <StateChip
        tone={qualityTone(verdict.quality)}
        label={qualityLabel(verdict.quality)}
      />
      {/* The report NAMES its source (basename only — the PII discipline):
          "sourceName + verdict", the exact mirror of the folder review's
          folderName line. */}
      <p className="import-artifact__source-name">{verdict.sourceName}</p>

      {recognized.length > 0 ? (
        <section className="import-artifact__group">
          <p className="import-artifact__group-heading">
            Ce que Rustory a reconnu
          </p>
          <ul className="import-artifact__findings">
            {recognized.map((finding) => (
              <FindingItem key={findingKey(finding)} finding={finding} />
            ))}
          </ul>
        </section>
      ) : null}

      {attention.length > 0 ? (
        <section className="import-artifact__group">
          <p className="import-artifact__group-heading">Points d'attention</p>
          <ul className="import-artifact__findings">
            {attention.map((finding) => (
              <FindingItem key={findingKey(finding)} finding={finding} />
            ))}
          </ul>
        </section>
      ) : null}

      <div className="import-artifact__actions">
        {importable ? (
          <Button variant="primary" onClick={onAccept}>
            Importer ce qui est reconnu
          </Button>
        ) : null}
        <Button variant="quiet" onClick={onAbandon}>
          Abandonner
        </Button>
      </div>
    </div>
  );
}

function FindingItem({
  finding,
}: {
  finding: ImportFinding;
}): React.JSX.Element {
  return (
    <li className="import-artifact__finding">
      <StateChip
        tone={categoryTone(finding.category)}
        label={categoryLabel(finding.category)}
        className="import-artifact__finding-chip"
      />
      <span className="import-artifact__finding-message">
        {finding.message}
      </span>
    </li>
  );
}

function findingKey(finding: ImportFinding): string {
  return `${finding.aspect}-${finding.category}`;
}
