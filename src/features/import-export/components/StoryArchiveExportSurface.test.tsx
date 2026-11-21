import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AppError } from "../../../shared/errors/app-error";
import type { StoryArchiveExportStatus } from "../hooks/use-story-archive-export";
import {
  StoryArchiveExportSurface,
  formatArchiveSize,
} from "./StoryArchiveExportSurface";

const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";

function setup(status: StoryArchiveExportStatus) {
  const onRetry = vi.fn();
  const onDismiss = vi.fn();
  render(
    <StoryArchiveExportSurface
      status={status}
      onRetry={onRetry}
      onDismiss={onDismiss}
    />,
  );
  return { onRetry, onDismiss };
}

function failed(error: AppError): StoryArchiveExportStatus {
  return { kind: "failed", storyId: STORY_ID, storyTitle: "Les Bestioles", error };
}

function liveRegion(): Element {
  const region = document.querySelector("[aria-live='polite']");
  if (!region) throw new Error("the polite region must stay mounted");
  return region;
}

describe("<StoryArchiveExportSurface />", () => {
  it("shows nothing while idle but keeps its polite region mounted and empty", () => {
    setup({ kind: "idle" });
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("region", { name: "Export en archive" }),
    ).not.toBeInTheDocument();
    expect(liveRegion().textContent).toBe("");
  });

  it("names the story and shows an indeterminate bar before the first tick", () => {
    setup({
      kind: "exporting",
      storyId: STORY_ID,
      storyTitle: "Les Bestioles",
      percent: null,
    });
    expect(
      screen.getByText("Export de « Les Bestioles » en archive"),
    ).toBeInTheDocument();
    const bar = screen.getByRole("progressbar", {
      name: "Exportation en cours…",
    });
    expect(bar).not.toHaveAttribute("aria-valuenow");
    // The user is told the library stays usable.
    expect(
      screen.getByText(/Tu\s+peux continuer à utiliser Rustory/),
    ).toBeInTheDocument();
    // An export in flight is neither announced nor dismissable.
    expect(liveRegion().textContent).toBe("");
    expect(
      screen.queryByRole("button", { name: "Fermer" }),
    ).not.toBeInTheDocument();
  });

  it("shows the real percent once the export streams it", () => {
    setup({
      kind: "exporting",
      storyId: STORY_ID,
      storyTitle: "Les Bestioles",
      percent: 42,
    });
    const bar = screen.getByRole("progressbar", {
      name: "Exportation en cours… 42 %",
    });
    expect(bar).toHaveAttribute("aria-valuenow", "42");
  });

  it("announces the exported archive and says what it carries and where it is", async () => {
    const { onDismiss, onRetry } = setup({
      kind: "exported",
      storyTitle: "Les Bestioles",
      destinationPath: "/home/moi/Les_Bestioles.zip",
      bytesWritten: 12_345_678,
      imageCount: 1,
      audioCount: 2,
    });
    expect(liveRegion().textContent).toBe("Archive exportée : Les Bestioles");
    const panel = screen.getByRole("region", { name: "Export en archive" });
    expect(within(panel).getByText("Archive exportée")).toBeInTheDocument();
    expect(within(panel).getByText("« Les Bestioles »")).toBeInTheDocument();
    expect(
      within(panel).getByText("2 sons et 1 image · 12 Mo"),
    ).toBeInTheDocument();
    expect(
      within(panel).getByText("Exportée vers /home/moi/Les_Bestioles.zip"),
    ).toBeInTheDocument();
    expect(
      within(panel).getByText(/s'importe telle quelle dans un autre Rustory/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onRetry).not.toHaveBeenCalled();
  });

  it("spells a single sound without a picture in the singular", () => {
    setup({
      kind: "exported",
      storyTitle: "Seule",
      destinationPath: "/tmp/Seule.zip",
      bytesWritten: 400_000,
      imageCount: 0,
      audioCount: 1,
    });
    expect(screen.getByText("1 son · moins de 1 Mo")).toBeInTheDocument();
  });

  it("offers another destination when the destination is the problem", async () => {
    const { onRetry, onDismiss } = setup(
      failed({
        code: "EXPORT_DESTINATION_UNAVAILABLE",
        message: "Écriture refusée par le système pour ce dossier.",
        userAction: "Choisis un dossier où tu as les droits en écriture.",
        details: { source: "temp_create", kind: "permission_denied" },
      }),
    );
    const alert = screen.getByRole("alert");
    expect(within(alert).getByText("Exportation échouée")).toBeInTheDocument();
    expect(within(alert).getByText("« Les Bestioles »")).toBeInTheDocument();
    expect(
      within(alert).getByText(
        "Écriture refusée par le système pour ce dossier.",
      ),
    ).toBeInTheDocument();
    expect(
      within(alert).getByText(
        "Choisis un dossier où tu as les droits en écriture.",
      ),
    ).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Choisir un autre emplacement" }),
    );
    expect(onRetry).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("offers no retry when the story itself cannot be exported", () => {
    setup(
      failed({
        code: "EXPORT_FAILED",
        message:
          "Export impossible: un ou plusieurs épisodes de cette histoire n'ont pas d'audio.",
        userAction: "Ajoute un audio à chaque épisode, puis réessaie l'export.",
        details: { source: "story_pack", cause: "missing_audio" },
      }),
    );
    expect(
      screen.getByText(
        "Export impossible: un ou plusieurs épisodes de cette histoire n'ont pas d'audio.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Ajoute un audio à chaque épisode, puis réessaie l'export.",
      ),
    ).toBeInTheDocument();
    // Trying again cannot help: the only action left is to close.
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Fermer",
    ]);
  });

  it("offers no retry for a story that no longer exists", () => {
    setup(
      failed({
        code: "LIBRARY_INCONSISTENT",
        message: "Export impossible: histoire introuvable.",
        userAction: "Retourne à la bibliothèque et recharge la liste.",
        details: { source: "story_missing" },
      }),
    );
    expect(
      screen.queryByRole("button", { name: /Réessayer|emplacement/ }),
    ).not.toBeInTheDocument();
  });

  it("offers a plain retry for any other failure, and tolerates a missing next step", async () => {
    const { onRetry } = setup(
      failed({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "Rustory n'a pas pu lire ta bibliothèque locale.",
        userAction: null,
        details: null,
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    // No empty paragraph for the absent next step.
    const alert = screen.getByRole("alert");
    expect(alert.querySelector(".story-archive-export__hint")).toBeNull();
  });

  it("keeps Fermer after the retry button in tab order", () => {
    setup(
      failed({
        code: "EXPORT_DESTINATION_UNAVAILABLE",
        message: "m",
        userAction: "a",
        details: null,
      }),
    );
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Choisir un autre emplacement",
      "Fermer",
    ]);
  });
});

describe("formatArchiveSize", () => {
  it("spells sizes in decimal units with a French comma, like the device-space copy", () => {
    expect(formatArchiveSize(0)).toBe("moins de 1 Mo");
    expect(formatArchiveSize(499_999)).toBe("moins de 1 Mo");
    expect(formatArchiveSize(500_000)).toBe("1 Mo");
    expect(formatArchiveSize(250_000_000)).toBe("250 Mo");
    expect(formatArchiveSize(999_400_000)).toBe("999 Mo");
    expect(formatArchiveSize(999_500_000)).toBe("1,0 Go");
    expect(formatArchiveSize(1_400_000_000)).toBe("1,4 Go");
    expect(formatArchiveSize(1_400_000_000)).not.toContain(".");
  });
});
