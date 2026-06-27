import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ImportArtifactSurface } from "./ImportArtifactSurface";
import type { StoryImportStatus } from "../hooks/use-story-import";

const IMPORTABLE_CONTENT = {
  title: "Le Soleil",
  structureJson: '{"schemaVersion":1,"nodes":[]}',
  contentChecksum: "a".repeat(64),
  createdAt: "2026-06-20T10:00:00.000Z",
  updatedAt: "2026-06-24T14:15:00.000Z",
};

const REVIEW_PARTIAL: StoryImportStatus = {
  kind: "review",
  verdict: {
    kind: "analyzed",
    quality: "partial",
    state: "needsReview",
    findings: [
      { aspect: "envelope", category: "recognized", message: "Enveloppe valide." },
      { aspect: "title", category: "ambiguous", message: "Titre normalisé." },
    ],
    importableContent: IMPORTABLE_CONTENT,
    sourceName: "histoire.rustory",
    artifactChecksum: "b".repeat(64),
  },
};

const REVIEW_BLOCKED: StoryImportStatus = {
  kind: "review",
  verdict: {
    kind: "analyzed",
    quality: "unusable",
    state: "blocked",
    findings: [
      { aspect: "integrity", category: "blocking", message: "Corruption détectée." },
    ],
    sourceName: "corrompu.rustory",
    artifactChecksum: "c".repeat(64),
  },
};

function noop(): void {}

function renderSurface(
  status: StoryImportStatus,
  overrides: Partial<{
    onAccept: () => void;
    onAbandon: () => void;
    onRetry: () => void;
    onDismiss: () => void;
  }> = {},
) {
  return render(
    <ImportArtifactSurface
      status={status}
      onAccept={overrides.onAccept ?? noop}
      onAbandon={overrides.onAbandon ?? noop}
      onRetry={overrides.onRetry ?? noop}
      onDismiss={overrides.onDismiss ?? noop}
    />,
  );
}

describe("ImportArtifactSurface", () => {
  it("renders nothing while idle", () => {
    const { container } = renderSurface({ kind: "idle" });
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the analysis label while analyzing", () => {
    renderSurface({ kind: "analyzing" });
    expect(screen.getByText("Analyse de l'artefact…")).toBeInTheDocument();
  });

  it("renders a partially-usable verdict with accept + abandon actions", () => {
    renderSurface(REVIEW_PARTIAL);
    expect(screen.getByText("Partiellement exploitable")).toBeInTheDocument();
    expect(screen.getByText("Ce que Rustory a reconnu")).toBeInTheDocument();
    expect(screen.getByText("Points d'attention")).toBeInTheDocument();
    expect(screen.getByText("Titre normalisé.")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Importer ce qui est reconnu" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Abandonner" }),
    ).toBeInTheDocument();
  });

  it("renders a blocked verdict as an alert with only Abandonner", () => {
    renderSurface(REVIEW_BLOCKED);
    expect(screen.getByText("Inexploitable")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Importer ce qui est reconnu" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Abandonner" }),
    ).toBeInTheDocument();
  });

  it("fires onAccept / onAbandon from the review actions", async () => {
    const onAccept = vi.fn();
    const onAbandon = vi.fn();
    renderSurface(REVIEW_PARTIAL, { onAccept, onAbandon });
    await userEvent.click(
      screen.getByRole("button", { name: "Importer ce qui est reconnu" }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Abandonner" }));
    expect(onAccept).toHaveBeenCalledTimes(1);
    expect(onAbandon).toHaveBeenCalledTimes(1);
  });

  it("announces the success and shows the created title", () => {
    renderSurface({
      kind: "imported",
      story: { id: "s1", title: "Le Soleil", importState: "needsReview" },
    });
    // Present twice by design: the visually-hidden aria-live announcement
    // and the visible success chip (mirrors DeviceImportStatusSurface).
    expect(
      screen.getAllByText("Histoire importée dans ta bibliothèque").length,
    ).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("Le Soleil")).toBeInTheDocument();
  });

  it("renders a failure as an alert with Réessayer then Fermer", async () => {
    const onRetry = vi.fn();
    const onDismiss = vi.fn();
    renderSurface(
      {
        kind: "failed",
        error: {
          code: "IMPORT_FAILED",
          message: "Import impossible: fichier illisible.",
          userAction: "Vérifie le fichier puis réessaie.",
          details: null,
        },
      },
      { onRetry, onDismiss },
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(
      screen.getByText("Import impossible: fichier illisible."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Vérifie le fichier puis réessaie."),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
