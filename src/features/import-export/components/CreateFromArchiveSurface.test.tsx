import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CreateFromArchiveSurface } from "./CreateFromArchiveSurface";
import type { ArchiveCreationStatus } from "../hooks/use-archive-creation";

// The `media` copies Rust sends for an ARCHIVE: a pack and its archive —
// never the folder flow's wording (the surface renders a finding's message
// verbatim).
const MEDIA_ALL_PRESENT =
  "Tous les fichiers audio et image référencés par le pack sont présents dans l'archive et reconnus.";
const MEDIA_MISSING =
  "Certains fichiers audio ou image référencés par le pack sont introuvables dans l'archive. L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur.";

// The REAL wire shape of a creatable archive verdict (exactly the five
// archive aspects — what the facade guard lets through).
const CLEAN_FINDINGS = [
  {
    aspect: "envelope",
    category: "recognized",
    message: "Le descripteur story.json est présent et lisible.",
  },
  {
    aspect: "title",
    category: "recognized",
    message: "Le titre de l'histoire est valide.",
  },
  {
    aspect: "structure",
    category: "recognized",
    message: "La structure du pack est reconnue et convertie en histoire.",
  },
  {
    aspect: "media",
    category: "recognized",
    message: MEDIA_ALL_PRESENT,
  },
  {
    aspect: "mediaSize",
    category: "recognized",
    message:
      "Aucun fichier audio ou image référencé ne dépasse la taille autorisée.",
  },
] as const;

/** A verdict with `count` retained hash-named media — the shape a real
 *  community pack produces (basenames are content hashes). */
function reviewWithMedia(
  retained: number,
  discarded: string[] = [],
): ArchiveCreationStatus {
  const hashName = (i: number, ext: string) =>
    `${i.toString(16).padStart(40, "0")}.${ext}`;
  const retainedMedia = Array.from({ length: retained }, (_, i) =>
    hashName(i, i % 2 === 0 ? "png" : "mp3"),
  );
  return {
    kind: "review",
    verdict: {
      kind: "analyzed",
      quality: "clean",
      state: "recognized",
      findings: [...CLEAN_FINDINGS],
      creatableSummary: {
        title: "Al Chapone - Al Chapone",
        nodeCount: 18,
        retainedMedia,
        discardedMedia: discarded,
      },
      archiveName: "Al Chapone.zip",
      archivePath: "/home/user/Al Chapone.zip",
    },
  };
}

const noop = () => {};

function renderSurface(status: ArchiveCreationStatus) {
  render(
    <CreateFromArchiveSurface
      status={status}
      onAccept={noop}
      onAbandon={noop}
      onRetry={noop}
      onDismiss={noop}
    />,
  );
}

describe("<CreateFromArchiveSurface /> — media summary", () => {
  it("summarizes many retained media as a COUNT, never the hash-named list", () => {
    renderSurface(reviewWithMedia(27));
    expect(screen.getByText("Médias retenus : 27 fichiers")).toBeInTheDocument();
    // The regression the user reported: no 40-char hash basename ever
    // renders in the summary.
    expect(screen.queryByText(/[0-9a-f]{40}\.(png|mp3)/)).toBeNull();
  });

  it("uses the singular for exactly one retained media", () => {
    renderSurface(reviewWithMedia(1));
    expect(screen.getByText("Média retenu : 1 fichier")).toBeInTheDocument();
  });

  it("omits the retained line entirely when nothing is retained", () => {
    renderSurface(reviewWithMedia(0));
    expect(screen.queryByText(/Médias? retenus?/)).toBeNull();
  });

  it("still NAMES a short discarded list (the actionable part of the report)", () => {
    renderSurface(reviewWithMedia(3, ["cover.bmp", "voix.aiff"]));
    expect(
      screen.getByText("Médias écartés : cover.bmp, voix.aiff"),
    ).toBeInTheDocument();
  });

  it("collapses an overlong discarded list to a count", () => {
    const many = Array.from({ length: 9 }, (_, i) => `rejet-${i}.bmp`);
    renderSurface(reviewWithMedia(2, many));
    expect(screen.getByText("Médias écartés : 9 fichiers")).toBeInTheDocument();
    expect(screen.queryByText(/rejet-0\.bmp/)).toBeNull();
  });

  it("always shows the title and node count verbatim", () => {
    renderSurface(reviewWithMedia(27));
    expect(
      screen.getByText("Titre : Al Chapone - Al Chapone"),
    ).toBeInTheDocument();
    expect(screen.getByText("18 nœuds")).toBeInTheDocument();
  });
});

// The copies below are the ones Rust sends (the surface renders a finding's
// message verbatim — it never words a size itself).
const SIZE_LEFT_OUT =
  "Certains fichiers audio ou image référencés dépassent la taille autorisée (134 Mo par fichier). L'histoire sera créée sans eux ; pour les garder, remplace-les dans l'archive par des fichiers plus légers puis relance l'analyse.";
const SIZE_BLOCKED =
  "Les fichiers audio et image référencés dépassent ensemble la taille autorisée (34,4 Go). Allège l'archive (moins de médias ou des fichiers plus légers) puis relance l'analyse.";

/** The list item of the report that carries `message`. */
function findingRow(message: string): HTMLElement {
  const row = screen.getByText(message).closest("li");
  if (!row) throw new Error("the finding is not rendered as a list item");
  return row;
}

describe("<CreateFromArchiveSurface /> — media size", () => {
  it("names the size of a media left out, as a point of attention of a creatable pack", () => {
    renderSurface({
      kind: "review",
      verdict: {
        kind: "analyzed",
        quality: "partial",
        state: "needsReview",
        findings: [
          ...CLEAN_FINDINGS.slice(0, 4),
          { aspect: "mediaSize", category: "ambiguous", message: SIZE_LEFT_OUT },
        ],
        creatableSummary: {
          title: "Un long son",
          nodeCount: 2,
          retainedMedia: ["a.mp3"],
          discardedMedia: ["trop.mp3"],
        },
        archiveName: "un-long-son.zip",
        archivePath: "/home/user/un-long-son.zip",
      },
    });

    expect(screen.getByText("Partiellement exploitable")).toBeInTheDocument();
    expect(screen.getByText("Points d'attention")).toBeInTheDocument();
    // The size has its own line, with the bound — and its chip is the one
    // of a media that is there but left out, never `information manquante`.
    const row = findingRow(SIZE_LEFT_OUT);
    expect(within(row).getByText("ambiguïté")).toBeInTheDocument();
    expect(screen.queryByText("information manquante")).toBeNull();
    // The media themselves are neither missing nor of an unknown format.
    expect(
      within(findingRow(MEDIA_ALL_PRESENT)).getByText("reconnu"),
    ).toBeInTheDocument();
    // The story is still creatable, and the left-out file is named.
    expect(screen.getByText("Médias écartés : trop.mp3")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Créer l'histoire" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("blocks on the size with the size as the stated reason — the structure is not blamed", () => {
    renderSurface({
      kind: "review",
      verdict: {
        kind: "analyzed",
        quality: "unusable",
        state: "blocked",
        findings: [
          ...CLEAN_FINDINGS.slice(0, 4),
          { aspect: "mediaSize", category: "blocking", message: SIZE_BLOCKED },
        ],
        archiveName: "collection.zip",
        archivePath: "/home/user/collection.zip",
      },
    });

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Inexploitable");
    expect(alert).toHaveTextContent(SIZE_BLOCKED);
    // ONE blocker, on the size line.
    expect(screen.getAllByText("blocage réel")).toHaveLength(1);
    expect(within(findingRow(SIZE_BLOCKED)).getByText("blocage réel")).toBeInTheDocument();
    expect(
      within(
        findingRow("La structure du pack est reconnue et convertie en histoire."),
      ).getByText("reconnu"),
    ).toBeInTheDocument();
    // Nothing will be created: no summary, no accept — only `Abandonner`.
    expect(screen.queryByText("Ce qui sera créé")).toBeNull();
    expect(screen.queryByRole("button", { name: "Créer l'histoire" })).toBeNull();
    expect(screen.getByRole("button", { name: "Abandonner" })).toBeInTheDocument();
  });
});

describe("<CreateFromArchiveSurface /> — media presence", () => {
  it("words the media of a clean pack for an archive — never for a folder", () => {
    renderSurface(reviewWithMedia(2));

    expect(
      within(findingRow(MEDIA_ALL_PRESENT)).getByText("reconnu"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/dossier/)).toBeNull();
  });

  it("names a media absent from the archive as a missing information of a creatable pack", () => {
    renderSurface({
      kind: "review",
      verdict: {
        kind: "analyzed",
        quality: "partial",
        state: "partial",
        findings: [
          ...CLEAN_FINDINGS.slice(0, 3),
          { aspect: "media", category: "missing", message: MEDIA_MISSING },
          CLEAN_FINDINGS[4],
        ],
        creatableSummary: {
          title: "Sans couverture",
          nodeCount: 2,
          retainedMedia: ["intro.mp3"],
          discardedMedia: ["cover.png"],
        },
        archiveName: "sans-couverture.zip",
        archivePath: "/home/user/sans-couverture.zip",
      },
    });

    expect(screen.getByText("Partiellement exploitable")).toBeInTheDocument();
    expect(screen.getByText("Points d'attention")).toBeInTheDocument();
    // The line speaks of the pack and of its archive, under its own chip.
    expect(
      within(findingRow(MEDIA_MISSING)).getByText("information manquante"),
    ).toBeInTheDocument();
    // An archive is not a folder: no line of its review says so.
    expect(screen.queryByText(/dossier/)).toBeNull();
    // The story is still creatable, and the missing file is named.
    expect(screen.getByText("Médias écartés : cover.png")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Créer l'histoire" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

// The copy Rust sends when a pack references more media than its bound (the
// surface renders it verbatim — it never words a number itself).
const COUNT_BLOCKED =
  "Les fichiers audio et image référencés dépassent le nombre autorisé (8192 fichiers différents). Réduis le nombre de médias de l'archive (par exemple en répartissant le pack sur plusieurs archives) puis relance l'analyse.";

describe("<CreateFromArchiveSurface /> — media count", () => {
  it("blocks on the count with the number as the stated reason — the structure is not blamed", () => {
    // The REAL wire of a pack over its reference bound: the descriptor, its
    // title and its structure are judged; no media was read, so neither
    // media line exists.
    renderSurface({
      kind: "review",
      verdict: {
        kind: "analyzed",
        quality: "unusable",
        state: "blocked",
        findings: [
          ...CLEAN_FINDINGS.slice(0, 3),
          { aspect: "mediaCount", category: "blocking", message: COUNT_BLOCKED },
        ],
        archiveName: "pack-sans-fin.zip",
        archivePath: "/home/user/pack-sans-fin.zip",
      },
    });

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Inexploitable");
    // The limit is named, with the gesture.
    expect(alert).toHaveTextContent("(8192 fichiers différents)");
    expect(alert).toHaveTextContent(COUNT_BLOCKED);
    // ONE blocker, on the count line; the structure reads as recognized.
    expect(screen.getAllByText("blocage réel")).toHaveLength(1);
    expect(
      within(findingRow(COUNT_BLOCKED)).getByText("blocage réel"),
    ).toBeInTheDocument();
    expect(
      within(
        findingRow("La structure du pack est reconnue et convertie en histoire."),
      ).getByText("reconnu"),
    ).toBeInTheDocument();
    // Nothing is said of media that were never read.
    expect(screen.queryByText(CLEAN_FINDINGS[3].message)).toBeNull();
    expect(screen.queryByText(CLEAN_FINDINGS[4].message)).toBeNull();
    expect(screen.queryByText(/taille autorisée/)).toBeNull();
    // Nothing will be created: no summary, no accept — only `Abandonner`.
    expect(screen.queryByText("Ce qui sera créé")).toBeNull();
    expect(screen.queryByRole("button", { name: "Créer l'histoire" })).toBeNull();
    expect(screen.getByRole("button", { name: "Abandonner" })).toBeInTheDocument();
  });
});
