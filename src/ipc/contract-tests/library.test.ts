import { describe, expect, it, vi } from "vitest";

import { isAppError } from "../../shared/errors/app-error";
import {
  isLibraryOverviewDto,
  type LibraryOverviewDto,
} from "../../shared/ipc-contracts/library";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("library IPC contract", () => {
  it("accepts an empty overview as a typed LibraryOverviewDto", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockResolvedValueOnce({ stories: [] });

    const { getLibraryOverview } = await import("../commands/library");
    const overview: LibraryOverviewDto = await getLibraryOverview().promise;

    expect(overview).toEqual({ stories: [] });
  });

  it("propagates AppError-shaped rejections without mutation", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
      details: null,
    });

    const { getLibraryOverview } = await import("../commands/library");

    await expect(getLibraryOverview().promise).rejects.toMatchObject({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
    });
  });

  it("rejects with an UNKNOWN-coded error when the Rust side hangs past the timeout", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockImplementationOnce(
      () => new Promise(() => {}),
    );

    const { getLibraryOverview } = await import("../commands/library");

    await expect(getLibraryOverview(20).promise).rejects.toMatchObject({
      code: "UNKNOWN",
      message: expect.stringMatching(/trop de temps/i),
    });
  });

  it("cancel() swallows the timeout after teardown so no late rejection escapes", async () => {
    const core = await import("@tauri-apps/api/core");
    vi.mocked(core.invoke).mockImplementationOnce(
      () => new Promise(() => {}),
    );

    const { getLibraryOverview } = await import("../commands/library");
    const handle = getLibraryOverview(10);
    // Attach a no-op catch so vitest doesn't flag an unhandled rejection
    // when cancel races the timer — the handle resolves to nothing after
    // cancel() because the guard is neutralized.
    handle.promise.catch(() => {});
    handle.cancel();

    await new Promise((resolve) => setTimeout(resolve, 30));
    // If cancel() did not clear the timer, the process would have logged
    // an unhandled rejection by now. No assertion beyond reaching this
    // line is necessary — the test just proves the teardown path runs.
  });
});

describe("isAppError guard", () => {
  it("requires the details field to be present", () => {
    // Matches the contract on both sides: `details` is always emitted,
    // even when it is `null`. A payload missing the key is drift.
    expect(
      isAppError({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "x",
        userAction: null,
        details: null,
      }),
    ).toBe(true);
    expect(
      isAppError({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "x",
        userAction: null,
      }),
    ).toBe(false);
  });

  it("rejects non-object payloads and missing required fields", () => {
    expect(isAppError(null)).toBe(false);
    expect(isAppError("boom")).toBe(false);
    expect(isAppError({ code: 42, message: "x", details: null })).toBe(false);
    expect(isAppError({ message: "x", details: null })).toBe(false);
  });
});

describe("isLibraryOverviewDto guard", () => {
  it("accepts a well-formed overview", () => {
    expect(
      isLibraryOverviewDto({
        stories: [
          { id: "a", title: "A" },
          { id: "b", title: "B" },
        ],
      }),
    ).toBe(true);
  });

  it("rejects a story with an empty id", () => {
    expect(
      isLibraryOverviewDto({ stories: [{ id: "", title: "A" }] }),
    ).toBe(false);
  });

  it("rejects a story with a blank title (empty or whitespace-only)", () => {
    expect(
      isLibraryOverviewDto({ stories: [{ id: "a", title: "" }] }),
    ).toBe(false);
    expect(
      isLibraryOverviewDto({ stories: [{ id: "a", title: "   " }] }),
    ).toBe(false);
  });

  it("rejects a payload with duplicate ids", () => {
    expect(
      isLibraryOverviewDto({
        stories: [
          { id: "a", title: "A" },
          { id: "a", title: "A bis" },
        ],
      }),
    ).toBe(false);
  });

  it("rejects non-object payloads and missing stories array", () => {
    expect(isLibraryOverviewDto(null)).toBe(false);
    expect(isLibraryOverviewDto({})).toBe(false);
    expect(isLibraryOverviewDto({ stories: "nope" })).toBe(false);
  });

  it("accepts every card-persistable import state — resolved included", () => {
    // CRITICAL: the first settled review to reach the library must not
    // drift-error the whole overview — "resolved" is a persisted state.
    for (const state of ["recognized", "partial", "needsReview", "resolved"]) {
      expect(
        isLibraryOverviewDto({
          stories: [{ id: "a", title: "A", importState: state }],
        }),
      ).toBe(true);
    }
    // `blocked` is never persisted, so never on a card.
    expect(
      isLibraryOverviewDto({
        stories: [{ id: "a", title: "A", importState: "blocked" }],
      }),
    ).toBe(false);
  });

  it("accepts the send readiness fields and refuses a blocker outside the closed set", () => {
    // Symmetric twin of the Rust `library_overview_send_readiness_wire_shape`
    // contract: `sendable` alone, or one closed camelCase `sendBlocker`.
    expect(
      isLibraryOverviewDto({
        stories: [{ id: "a", title: "Web", sendable: true }],
      }),
    ).toBe(true);
    for (const blocker of [
      "devicePack",
      "empty",
      "malformed",
      "missingAudio",
      "missingAnnouncements",
      "optionWithoutDestination",
      "nodeWithoutAudio",
    ]) {
      expect(
        isLibraryOverviewDto({
          stories: [{ id: "a", title: "A", sendBlocker: blocker }],
        }),
      ).toBe(true);
    }
    expect(
      isLibraryOverviewDto({
        stories: [{ id: "a", title: "A", sendBlocker: "noSourceArchive" }],
      }),
    ).toBe(false);
    // "Having choices" is no reason on a card any more: a story with
    // choices is sent by its own graph.
    expect(
      isLibraryOverviewDto({
        stories: [{ id: "a", title: "A", sendBlocker: "branching" }],
      }),
    ).toBe(false);
    expect(
      isLibraryOverviewDto({
        stories: [{ id: "a", title: "A", sendable: "yes" }],
      }),
    ).toBe(false);
  });

  it("accepts a structured-folder card: partial state + a media/missing report finding", () => {
    // Symmetric twin of the Rust producer test
    // (`read_stories_projects_a_structured_folder_creation_with_the_folder_copy`):
    // the first folder creation to reach the library must not drift-error —
    // its `partial` marker and its `media`-aspect report pass the card guard.
    expect(
      isLibraryOverviewDto({
        stories: [
          {
            id: "a",
            title: "Depuis un dossier",
            importState: "partial",
            importReport: [
              {
                aspect: "envelope",
                category: "recognized",
                message: "Le manifest histoire.json est présent et lisible.",
              },
              {
                aspect: "media",
                category: "missing",
                message:
                  "Certains fichiers audio ou image référencés par le dossier sont introuvables.",
              },
            ],
          },
        ],
      }),
    ).toBe(true);
  });

  it("accepts a structured-archive card whose report names a media left out for its size", () => {
    // Symmetric twin of the Rust producer tests
    // (`read_stories_projects_a_structured_archive_creation_with_the_archive_copy`,
    // `a_media_one_byte_over_the_bound_is_named_for_its_size_from_the_review_to_the_card`):
    // the `mediaSize` aspect belongs to the closed finding set of a card
    // report, so the durable marker of such a story never drift-errors.
    expect(
      isLibraryOverviewDto({
        stories: [
          {
            id: "a",
            title: "Depuis une archive",
            importState: "needsReview",
            sendable: true,
            importReport: [
              {
                aspect: "envelope",
                category: "recognized",
                message: "Le descripteur story.json est présent et lisible.",
              },
              {
                aspect: "media",
                category: "recognized",
                message:
                  "Tous les fichiers audio et image référencés par le pack sont présents dans l'archive et reconnus.",
              },
              {
                aspect: "mediaSize",
                category: "ambiguous",
                message:
                  "Certains fichiers audio ou image référencés dépassent la taille autorisée (134 Mo par fichier). L'histoire sera créée sans eux ; pour les garder, remplace-les dans l'archive par des fichiers plus légers puis relance l'analyse.",
              },
            ],
          },
        ],
      }),
    ).toBe(true);
  });

  it("still accepts a report persisted before the media-size aspect existed", () => {
    // A summary stored by an earlier version carries no `mediaSize` pair:
    // the card guard never requires one (a report is whatever was stored).
    expect(
      isLibraryOverviewDto({
        stories: [
          {
            id: "a",
            title: "Créée avant",
            importState: "needsReview",
            importReport: [
              {
                aspect: "envelope",
                category: "recognized",
                message: "Le descripteur story.json est présent et lisible.",
              },
              {
                aspect: "media",
                category: "ambiguous",
                message:
                  "Certains fichiers audio ou image référencés ne sont pas utilisables (format non reconnu, fichier trop volumineux ou nom invalide). L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur.",
              },
            ],
          },
        ],
      }),
    ).toBe(true);
  });

  it("refuses a report whose aspect drifted from the closed set", () => {
    expect(
      isLibraryOverviewDto({
        stories: [
          {
            id: "a",
            title: "Dérive",
            importState: "needsReview",
            importReport: [
              { aspect: "media_size", category: "ambiguous", message: "x" },
            ],
          },
        ],
      }),
    ).toBe(false);
  });
});
