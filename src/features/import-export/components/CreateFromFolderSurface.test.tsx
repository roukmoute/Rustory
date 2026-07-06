import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CreateFromFolderSurface } from "./CreateFromFolderSurface";
import type { StructuredCreationStatus } from "../hooks/use-structured-creation";

// The REAL wire shapes (exactly the five folder aspects on a creatable
// verdict — what the facade guard lets through).
const CLEAN_FINDINGS = [
  {
    aspect: "envelope",
    category: "recognized",
    message: "Le manifest histoire.json est présent et lisible.",
  },
  {
    aspect: "formatVersion",
    category: "recognized",
    message: "La version de format du manifest est prise en charge.",
  },
  {
    aspect: "title",
    category: "recognized",
    message: "Le titre de l'histoire est valide.",
  },
  {
    aspect: "structure",
    category: "recognized",
    message: "La structure de l'histoire est reconnue.",
  },
  {
    aspect: "media",
    category: "recognized",
    message:
      "Tous les fichiers audio et image référencés par le dossier sont présents et reconnus.",
  },
] as const;

const REVIEW_CLEAN: StructuredCreationStatus = {
  kind: "review",
  verdict: {
    kind: "analyzed",
    quality: "clean",
    state: "recognized",
    findings: [...CLEAN_FINDINGS],
    creatableSummary: {
      title: "Le voyage de Nour",
      nodeCount: 2,
      retainedMedia: ["couverture.png"],
      discardedMedia: [],
    },
    folderName: "mon-dossier",
    folderPath: "/home/user/mon-dossier",
  },
};

const REVIEW_PARTIAL: StructuredCreationStatus = {
  kind: "review",
  verdict: {
    kind: "analyzed",
    quality: "partial",
    state: "partial",
    findings: [
      CLEAN_FINDINGS[0],
      CLEAN_FINDINGS[1],
      CLEAN_FINDINGS[2],
      CLEAN_FINDINGS[3],
      {
        aspect: "media",
        category: "missing",
        message:
          "Certains fichiers audio ou image référencés par le dossier sont introuvables. L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur.",
      },
    ],
    creatableSummary: {
      title: "Sans image",
      nodeCount: 1,
      retainedMedia: [],
      discardedMedia: ["absente.png"],
    },
    folderName: "manque",
    folderPath: "/home/user/manque",
  },
};

const REVIEW_BLOCKED: StructuredCreationStatus = {
  kind: "review",
  verdict: {
    kind: "analyzed",
    quality: "unusable",
    state: "blocked",
    findings: [
      {
        aspect: "envelope",
        category: "blocking",
        message: "Le dossier ne contient pas de manifest histoire.json lisible.",
      },
    ],
    folderName: "casse",
    folderPath: "/home/user/casse",
  },
};

function noop(): void {}

function renderSurface(
  status: StructuredCreationStatus,
  overrides: Partial<{
    onAccept: () => void;
    onAbandon: () => void;
    onRetry: () => void;
    onDismiss: () => void;
  }> = {},
) {
  return render(
    <CreateFromFolderSurface
      status={status}
      onAccept={overrides.onAccept ?? noop}
      onAbandon={overrides.onAbandon ?? noop}
      onRetry={overrides.onRetry ?? noop}
      onDismiss={overrides.onDismiss ?? noop}
    />,
  );
}

describe("CreateFromFolderSurface", () => {
  it("renders nothing while idle", () => {
    const { container } = renderSurface({ kind: "idle" });
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the calm analyzing progress", () => {
    renderSurface({ kind: "analyzing" });
    expect(screen.getByText("Analyse du dossier…")).toBeInTheDocument();
  });

  it("renders a clean verdict with the unique CTA and the folder basename", async () => {
    const onAccept = vi.fn();
    renderSurface(REVIEW_CLEAN, { onAccept });
    expect(screen.getByText("Propre")).toBeInTheDocument();
    expect(screen.getByText("mon-dossier")).toBeInTheDocument();
    // The absolute path NEVER renders.
    expect(
      screen.queryByText(/\/home\/user\/mon-dossier/),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Ce que Rustory a reconnu")).toBeInTheDocument();
    // The UNIQUE accept CTA (no second variant).
    const accept = screen.getByRole("button", { name: "Créer l'histoire" });
    await userEvent.click(accept);
    expect(onAccept).toHaveBeenCalledTimes(1);
    expect(
      screen.getByRole("button", { name: "Abandonner" }),
    ).toBeInTheDocument();
    // A creatable report is polite, never an alert.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders a partial verdict with both groups and stays creatable", () => {
    renderSurface(REVIEW_PARTIAL);
    expect(screen.getByText("Partiellement exploitable")).toBeInTheDocument();
    expect(screen.getByText("Ce que Rustory a reconnu")).toBeInTheDocument();
    expect(screen.getByText("Points d'attention")).toBeInTheDocument();
    expect(screen.getByText("information manquante")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Créer l'histoire" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders the what-will-be-created summary: title, node count, retained media", () => {
    renderSurface(REVIEW_CLEAN);
    expect(screen.getByText("Ce qui sera créé")).toBeInTheDocument();
    expect(
      screen.getByText("Titre : Le voyage de Nour"),
    ).toBeInTheDocument();
    expect(screen.getByText("2 nœuds")).toBeInTheDocument();
    expect(
      screen.getByText("Médias retenus : couverture.png"),
    ).toBeInTheDocument();
    // No discarded media on a clean folder — the line is absent.
    expect(screen.queryByText(/Médias écartés/)).not.toBeInTheDocument();
  });

  it("names the discarded media by basename so the user knows what to fix", () => {
    renderSurface(REVIEW_PARTIAL);
    expect(screen.getByText("Ce qui sera créé")).toBeInTheDocument();
    expect(screen.getByText("Titre : Sans image")).toBeInTheDocument();
    expect(screen.getByText("1 nœud")).toBeInTheDocument();
    expect(
      screen.getByText("Médias écartés : absente.png"),
    ).toBeInTheDocument();
    // No retained media on this folder — the line is absent.
    expect(screen.queryByText(/Médias retenus/)).not.toBeInTheDocument();
  });

  it("renders a blocked verdict as an alert with Abandonner only", async () => {
    const onAbandon = vi.fn();
    renderSurface(REVIEW_BLOCKED, { onAbandon });
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("Inexploitable")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Créer l'histoire" }),
    ).not.toBeInTheDocument();
    // Nothing will be created — the summary group is absent.
    expect(screen.queryByText("Ce qui sera créé")).not.toBeInTheDocument();
    const abandon = screen.getByRole("button", { name: "Abandonner" });
    await userEvent.click(abandon);
    expect(onAbandon).toHaveBeenCalledTimes(1);
  });

  it("shows the creating progress", () => {
    renderSurface({ kind: "creating" });
    expect(screen.getByText("Création en cours…")).toBeInTheDocument();
  });

  it("renders the sober created state with the title and an explicit Fermer", async () => {
    const onDismiss = vi.fn();
    renderSurface(
      {
        kind: "created",
        story: { id: "s1", title: "Le voyage de Nour", importState: "recognized" },
      },
      { onDismiss },
    );
    expect(
      screen.getAllByText("Histoire créée dans ta bibliothèque").length,
    ).toBeGreaterThan(0);
    expect(screen.getByText("Le voyage de Nour")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("renders a failed state as an alert with Réessayer then Fermer", async () => {
    const onRetry = vi.fn();
    renderSurface(
      {
        kind: "failed",
        error: {
          code: "IMPORT_FAILED",
          message: "Création impossible: le dossier n'a pas pu être revalidé.",
          userAction: "Relance l'analyse du dossier puis réessaie.",
          details: null,
        },
      },
      { onRetry },
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Création impossible");
    expect(alert).toHaveTextContent(
      "Création impossible: le dossier n'a pas pu être revalidé.",
    );
    expect(alert).toHaveTextContent(
      "Relance l'analyse du dossier puis réessaie.",
    );
    const buttons = screen.getAllByRole("button");
    expect(buttons[0]).toHaveTextContent("Réessayer");
    expect(buttons[1]).toHaveTextContent("Fermer");
    await userEvent.click(buttons[0]);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
