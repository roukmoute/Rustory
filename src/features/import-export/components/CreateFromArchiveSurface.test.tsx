import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CreateFromArchiveSurface } from "./CreateFromArchiveSurface";
import type { ArchiveCreationStatus } from "../hooks/use-archive-creation";

// The REAL wire shape of a creatable archive verdict (exactly the four
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
    message:
      "Tous les fichiers audio et image référencés sont présents et reconnus.",
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
